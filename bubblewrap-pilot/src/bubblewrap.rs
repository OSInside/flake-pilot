//
// Copyright (c) 2026 Marcus Schäfer
//
// This file is part of flake-pilot
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
//
use crate::defaults;
use crate::overlay;
use crate::config::{RuntimeSection, config, usermode};

use flakes::user::User;
use flakes::lookup::Lookup;
use flakes::io::IO;
use flakes::error::FlakeError;
use flakes::config::get_bubblewrap_ids_dir;

use lazy_static::lazy_static;
use regex::Regex;
use uzers::get_current_username;

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, ExitCode};

lazy_static! {
    /// Name of the user calling the program
    static ref CALLING_USER: String = get_current_username()
        .map(|user| user.to_string_lossy().to_string())
        .unwrap_or_default();

    /// Reference of an environment variable in a runtime option
    static ref VAR_PATTERN: Regex = Regex::new(r"%([A-Z]+)").unwrap();
}

pub fn create(program_name: &str) -> Result<String, FlakeError> {
    /*!
    Create the meta data of a new sandbox for program_name

    The root filesystem of the sandbox and all other settings to
    run the program in it are taken from the config file(s)

    FLAKE_DIR/
       ├── program_name.d
       │   └── other.yaml
       └── program_name.yaml

    All commandline options will be passed to the program_name
    called in the sandbox. An example program config file
    looks like the following:

    sandbox:
      name: path/to/rootfs/on/host
      target_app_path: path/to/program/in/sandbox
      host_app_path: path/to/program/on/host

      runtime:
        # Pilot options which are always effective for this
        # application. The same options can be given at call
        # time and then take precedence over the setting here
        pilot_options:
          - "%interactive"

        bubblewrap:
          - --ro-bind /etc/resolv.conf /etc/resolv.conf
          - --dev /dev
          - --proc /proc
          - --tmpfs /tmp
          - --unshare-pid
          - --die-with-parent

    A sandbox exists as long as the program running in it. It is
    created per registered flake command or, if the application
    is called with @NAME arguments, per command instance. The
    name of the meta data file which represents the instance is
    returned by this method
    !*/
    // A sandbox provides no entry point like a container image
    // does. The command to call in it has to be known
    if get_target_app_path(program_name) == "/" {
        return Err(FlakeError::IOError {
            kind: "Unknown command".to_string(),
            message: "A sandbox has no entry point, \
                please specify a target_app_path".to_string()
        })
    }

    // The root of the sandbox is a directory on the host
    let rootfs = config().sandbox.name;
    if ! Path::new(rootfs).is_dir() {
        return Err(FlakeError::IOError {
            kind: "Invalid sandbox root".to_string(),
            message: format!(
                "The root filesystem of the sandbox, {rootfs}, \
                is not a directory"
            )
        })
    }

    // Read optional @NAME pilot arguments to differentiate
    // simultaneous instances of the same sandbox application
    for instance_name in env::args()
        .skip(1).filter(|arg| arg.starts_with('@'))
    {
        if ! Lookup::is_safe_instance_name(&instance_name) {
            return Err(FlakeError::IOError {
                kind: "Invalid instance name".to_string(),
                message: format!(
                    "The instance name {instance_name} contains characters \
                    which are not allowed"
                )
            })
        }
    }

    let sandbox_ids_dir = get_ids_dir()?;
    let sandbox_id_file = get_sandbox_id_file(&sandbox_ids_dir, program_name);

    // There is one sandbox per command or command instance
    if Path::new(&sandbox_id_file).exists() {
        if sandbox_running(&sandbox_id_file)? {
            return Err(FlakeError::AlreadyRunning)
        }
        // The sandbox is gone but its meta data file was left
        // behind, e.g the pilot was killed before it could
        // delete it
        fs::remove_file(&sandbox_id_file)?;
    }

    // Garbage collect the meta data files of the other instances
    gc(&sandbox_ids_dir);

    // Create the meta data file of this instance. The process ID
    // of the sandbox is written to it when it gets started
    IO::create_meta_file(&sandbox_id_file)?
        .write_all(defaults::SANDBOX_NOT_STARTED.as_bytes())?;

    Ok(sandbox_id_file)
}

pub fn start(
    program_name: &str, sandbox_id_file: &str
) -> Result<ExitCode, FlakeError> {
    /*!
    Create the sandbox and call the application inside of it

    The root filesystem of the sandbox is provided as an overlay
    of the rootfs on the host. The overlay only exists as long as
    the application and is deleted with it. What the application
    writes is kept in the tmpfs of the instance for the next
    sandbox of the instance, unless the %discard pilot option is
    given, see overlay::umount()
    !*/
    let instance = get_instance_name(program_name);

    // The root of the sandbox is referenced as %OVERLAYROOT in
    // the options which create it
    env::set_var(
        defaults::OVERLAY_ROOT_VAR,
        overlay::get_dir(&instance, defaults::OVERLAY_MERGED_NAME)
    );
    let RuntimeSection { bubblewrap, .. } = config().runtime();
    let (layers, engine_arguments) = match get_engine_arguments(bubblewrap) {
        Ok(arguments) => arguments,
        Err(error) => {
            delete_sandbox_id_file(sandbox_id_file);
            return Err(error)
        }
    };

    let overlay_root = match overlay::mount(
        &instance, config().sandbox.name, &layers,
        get_tmpfs_size().as_deref(), get_sandbox_user()
    ) {
        Ok(overlay_root) => overlay_root,
        Err(error) => {
            // Without a root filesystem there is no sandbox and
            // therefore also no instance of it
            delete_sandbox_id_file(sandbox_id_file);
            return Err(error)
        }
    };

    let result = run_sandbox(
        program_name, sandbox_id_file, &overlay_root, engine_arguments
    );

    // The overlay of the rootfs exists as long as the sandbox
    overlay::umount(&instance, discard_requested());

    result
}

fn run_sandbox(
    program_name: &str, sandbox_id_file: &str, overlay_root: &str,
    engine_arguments: Vec<String>
) -> Result<ExitCode, FlakeError> {
    /*!
    Call the application in the sandbox

    bwrap sets up the new root system and replaces itself with
    the application. The process recorded as the instance of the
    flake is therefore the application running in the sandbox.
    Its exit code becomes the exit code of the pilot
    !*/
    let mut call = Command::new(defaults::BWRAP_PATH);
    let sandbox_options = get_sandbox_options(
        overlay_root, engine_arguments, &get_sandbox_workdir()
    );
    for option in sandbox_options {
        call.arg(option);
    }
    call.arg(get_target_app_path(program_name));
    for arg in Lookup::get_run_cmdline(Vec::new(), false) {
        call.arg(arg);
    }
    if Lookup::is_debug() {
        debug!("{:?} {:?}", call.get_program(), call.get_args());
    }

    let spawned = call.spawn();
    if spawned.is_err() {
        delete_sandbox_id_file(sandbox_id_file);
    }
    let mut child = match spawned {
        Ok(child) => child,
        // The sandbox is created by the bwrap program
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(FlakeError::IOError {
                kind: "Sandbox engine not found".to_string(),
                message: format!(
                    "{} is required to run this flake, please install it",
                    defaults::BWRAP_PATH
                )
            })
        },
        Err(error) => return Err(error.into())
    };

    let pid = child.id();
    if Lookup::is_debug() {
        debug!("Sandbox process: {pid}");
    }
    write_sandbox_id_file(sandbox_id_file, pid);

    let status = child.wait();

    // The sandbox only exists as long as its process. Once it is
    // gone the meta data of the instance is deleted with it
    delete_sandbox_id_file(sandbox_id_file);

    match status?.code() {
        Some(code) => Ok(ExitCode::from(code as u8)),
        // the application was terminated by a signal
        None => Ok(ExitCode::FAILURE)
    }
}

pub fn get_target_app_path(program_name: &str) -> String {
    /*!
    setup application command path name

    This is either the program name specified at registration
    time or the configured target application from the flake
    configuration file
    !*/
    config().sandbox.target_app_path.unwrap_or(program_name).to_owned()
}

pub fn get_engine_arguments(
    options: Option<Vec<&str>>
) -> Result<(Vec<String>, Vec<String>), FlakeError> {
    /*!
    Read the bwrap options of the flake and provide the layers of
    the root of the sandbox along with the other options

    If the flake configures no options at all, a default setup is
    used: the standard pseudo filesystems, a writable /tmp, the
    application in namespaces and a session of its own, and a
    cleared environment. A variable reference in the format %NAME
    is replaced by the value of the environment variable of that
    name.

    With --clearenv, from the defaults or from the configuration,
    the variables of the caller listed in SANDBOX_ENVIRONMENT are
    set again, unless the configuration sets them with --setenv.

    A source configured with --overlay-src is a layer stacked on
    top of the rootfs, in the order the sources are given. The
    layers are part of the overlay created on the host, see
    overlay::mount(), they are provided apart from the options
    which are passed to bwrap
    !*/
    let configured_options = options.unwrap_or_default();
    let engine_options = if configured_options.is_empty() {
        defaults::BWRAP_OPTIONS.to_vec()
    } else {
        configured_options
    };

    // An option can be configured together with its value(s) in
    // one entry, e.g "--ro-bind /etc /etc". bwrap expects them
    // as separate arguments
    let mut arguments: Vec<String> = Vec::new();
    for option in engine_options.iter().flat_map(|x| x.split_whitespace()) {
        arguments.push(expand_variables(option)?);
    }

    let mut layers: Vec<String> = Vec::new();
    let mut engine_arguments: Vec<String> = Vec::new();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == defaults::BWRAP_OVERLAY_SRC_OPTION {
            if let Some(layer) = arguments.next() {
                layers.push(layer);
            }
        } else {
            engine_arguments.push(argument);
        }
    }

    // A cleared environment gets the variables of the allow list
    // again. They are added after all other options and thus after
    // --clearenv. Their values are passed on as they are and are
    // not split
    if engine_arguments.iter().any(|argument| argument == defaults::BWRAP_CLEARENV_OPTION) {
        let configured: Vec<String> = engine_arguments.windows(2)
            .filter(|pair| pair[0] == defaults::BWRAP_SETENV_OPTION)
            .map(|pair| pair[1].clone())
            .collect();
        for name in defaults::SANDBOX_ENVIRONMENT {
            if configured.iter().any(|configured| configured == name) {
                continue
            }
            if let Some(value) = env::var_os(name) {
                engine_arguments.push(defaults::BWRAP_SETENV_OPTION.to_string());
                engine_arguments.push(name.to_string());
                engine_arguments.push(value.to_string_lossy().to_string());
            }
        }
    }
    Ok((layers, engine_arguments))
}

pub fn get_sandbox_options(
    overlay_root: &str, engine_arguments: Vec<String>, workdir: &str
) -> Vec<String> {
    /*!
    Construct the bwrap options which create the sandbox

    The root of the new root system is always the overlay of the
    rootfs created on the host for the instance, mounted writable.
    The other options, see get_engine_arguments(), are added after
    it. The given workdir is the directory the application is
    called in, unless the flake configures one
    !*/
    let mut sandbox_options = vec![
        defaults::BWRAP_BIND_OPTION.to_string(),
        overlay_root.to_string(),
        defaults::SANDBOX_ROOT.to_string()
    ];
    sandbox_options.extend(engine_arguments);

    // Without a directory to change into, bwrap keeps the working
    // directory of the caller. That directory usually does not
    // exist in the new root system
    if ! sandbox_options.iter().any(
        |option| option == defaults::BWRAP_CHDIR_OPTION
    ) {
        sandbox_options.push(defaults::BWRAP_CHDIR_OPTION.to_string());
        sandbox_options.push(workdir.to_string());
    }
    sandbox_options
}

fn expand_variables(option: &str) -> Result<String, FlakeError> {
    /*!
    Resolve the variable references of the given bwrap argument

    A reference in the format %NAME is replaced by the value of
    the environment variable of that name. A variable which is
    not set in the environment is provided as a shell style
    variable reference, $NAME
    !*/
    let mut option_value = option.to_string();
    // The value of a variable can contain another %VAR reference.
    // Expansion is therefore done repeatedly but only up to a
    // fixed limit. Without it a variable value referencing itself
    // would keep this loop running forever
    let mut expansion_count = 0;
    while VAR_PATTERN.captures(&option_value.clone()).is_some() {
        expansion_count += 1;
        if expansion_count > defaults::VAR_EXPANSION_LIMIT {
            return Err(FlakeError::IOError {
                kind: "Invalid runtime option".to_string(),
                message: format!("Too many variable expansions in: {option}")
            })
        }
        for capture in VAR_PATTERN.captures_iter(&option_value.clone()) {
            // replace %VAR placeholder(s) with the respective
            // environment variable value if possible.
            // If not possible replace by the variable name
            let var_name = capture.get(1).unwrap().as_str();
            let var_value = env::var(var_name)
                .unwrap_or(format!("${var_name}"));
            option_value = option_value.replace(
                &format!("%{var_name}"), &var_value
            );
        }
    }
    Ok(option_value)
}

fn get_sandbox_workdir() -> String {
    /*!
    Provide the working directory of the application in the sandbox

    The root of the sandbox is used unless another directory is
    requested through the %chdir pilot option. A directory
    configured as a bubblewrap option of the flake takes
    precedence over both of them
    !*/
    let pilot_options = Lookup::get_pilot_run_options(
        config().pilot_options()
    );
    match pilot_options.get(defaults::PILOT_CHDIR_OPTION) {
        Some(workdir) if ! workdir.is_empty() => workdir.to_string(),
        _ => defaults::SANDBOX_WORKDIR.to_string()
    }
}

fn get_tmpfs_size() -> Option<String> {
    /*!
    Provide the size of the tmpfs which keeps the writable layer of
    the instance, as requested with the %tmpfs_size pilot option.
    Without it the limit configured system wide applies, if any
    !*/
    Lookup::get_pilot_run_options(config().pilot_options())
        .get(defaults::PILOT_TMPFS_SIZE_OPTION)
        .filter(|size| ! size.is_empty())
        .cloned()
}

fn discard_requested() -> bool {
    /*!
    Check if the writable layer of the instance is discarded when
    the sandbox ends, see the %discard pilot option
    !*/
    Lookup::get_pilot_run_options(config().pilot_options())
        .contains_key(defaults::PILOT_DISCARD_OPTION)
}

pub fn sandbox_running(sandbox_id_file: &str) -> Result<bool, FlakeError> {
    /*!
    Check if the sandbox of the given meta data file still exists

    The meta data file contains the process ID of the sandbox.
    A sandbox which was created but never started is recorded
    with a process ID of zero
    !*/
    IO::no_symlink(sandbox_id_file)?;
    let sandbox_id = fs::read_to_string(sandbox_id_file)?;
    Ok(process_running(sandbox_id.trim()))
}

fn process_running(sandbox_id: &str) -> bool {
    /*!
    Check if the process of the given sandbox ID is alive

    The name of the process is checked too, to not report a
    process which just reuses the ID of an already terminated
    sandbox as running
    !*/
    let pid = match sandbox_id.parse::<u32>() {
        Ok(pid) => pid,
        Err(_) => return false
    };
    if pid == 0 {
        return false
    }
    let process_name_file = format!("{}/{}/comm", defaults::PROC_DIR, pid);
    match fs::read_to_string(process_name_file) {
        Ok(process_name) => {
            let process_name = process_name.trim();
            process_name == defaults::BWRAP
        },
        Err(_) => false
    }
}

fn get_ids_dir() -> Result<String, FlakeError> {
    /*!
    Create the meta data directory structure and return the
    private directory of the calling user to store the meta
    data files of its instances in
    !*/
    IO::private_dir(&get_bubblewrap_ids_dir(usermode()))
}

fn get_sandbox_id_file(sandbox_ids_dir: &str, program_name: &str) -> String {
    /*!
    Provide the name of the meta data file of this instance

    There is one sandbox per registered flake command or, if the
    application is called with @NAME arguments, per command
    instance, e.g myapp@one.bwrapid
    !*/
    format!(
        "{}/{}.{}",
        sandbox_ids_dir, get_instance_name(program_name),
        defaults::SANDBOX_ID_EXTENSION
    )
}

pub fn get_instance_name(program_name: &str) -> String {
    /*!
    Provide the name of this instance

    The name is the name of the flake command, extended by the
    @NAME arguments the application was called with, e.g
    myapp@one. All data which belongs to one instance, its meta
    data file as well as the directories of its root filesystem,
    is named after it
    !*/
    format!("{}{}", program_name, Lookup::get_instance_name())
}

fn get_sandbox_user() -> User<'static> {
    /*!
    Provide the user the sandbox is created for

    bubblewrap creates the sandbox through user namespaces and
    needs no privileges. The sandbox therefore always belongs to
    the calling user. A system wide flake can be called by any
    user, a flake registered by a user is called by that user
    !*/
    User::from(CALLING_USER.as_str())
}

fn write_sandbox_id_file(sandbox_id_file: &str, pid: u32) {
    /*!
    Record the process ID of the sandbox

    The instance information is meta data. A failure to write it
    is reported but does not stop the application from running
    !*/
    match IO::create_meta_file(sandbox_id_file) {
        Ok(mut id_file) => {
            if let Err(error) = id_file.write_all(pid.to_string().as_bytes()) {
                error!("Failed to write {sandbox_id_file}: {error}");
            }
        },
        Err(error) => {
            error!("Failed to create {sandbox_id_file}: {error}");
        }
    }
}

fn delete_sandbox_id_file(sandbox_id_file: &str) {
    /*!
    Delete the meta data file of the instance
    !*/
    if let Err(error) = fs::remove_file(sandbox_id_file) {
        if Lookup::is_debug() {
            debug!("Failed to delete {sandbox_id_file}: {error}");
        }
    }
}

fn gc(sandbox_ids_dir: &str) {
    /*!
    Garbage collect the meta data files of sandboxes which
    no longer exist

    A meta data file is deleted by the pilot which created it.
    If that process was killed the file stays behind. Errors are
    only logged, a file which cannot be collected does not
    prevent the application from being called
    !*/
    let entries = match fs::read_dir(sandbox_ids_dir) {
        Ok(entries) => entries,
        Err(error) => {
            if Lookup::is_debug() {
                debug!("Failed to read {sandbox_ids_dir}: {error}");
            }
            return
        }
    };
    let id_extension = format!(".{}", defaults::SANDBOX_ID_EXTENSION);
    for entry in entries.flatten() {
        let sandbox_id_file = entry.path().display().to_string();
        if ! sandbox_id_file.ends_with(&id_extension) {
            continue
        }
        if let Ok(false) = sandbox_running(&sandbox_id_file) {
            if Lookup::is_debug() {
                debug!("Garbage collecting {sandbox_id_file}");
            }
            delete_sandbox_id_file(&sandbox_id_file);
        }
    }
}
