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
use flakes::helper::helper_call;
use flakes::bubblewrap::{split_options, split_persist_dirs};
use flakes::registration;

use lazy_static::lazy_static;
use uzers::{get_current_uid, get_current_username};

use std::env;
use std::fs;
use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitCode};

lazy_static! {
    /// Name of the user calling the program
    static ref CALLING_USER: String = get_current_username()
        .map(|user| user.to_string_lossy().to_string())
        .unwrap_or_default();
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
    of the rootfs on the host. The overlay and the tmpfs which keeps
    what the application writes are mounted in the namespace of the
    sandbox, they are not visible on the host and are gone along
    with the application.

    The pilot mounts them itself in a user namespace of its own,
    see overlay::mount(). A rootfs or a layer with mount points
    below it cannot be overlaid in a user namespace, e.g the root
    of the host. The helper creates the mount namespace of such a
    sandbox, mounts the overlay in it and starts the pilot there
    with the privileges of the caller, which then calls bwrap, see
    overlay::has_mounts_below() and start_in_mounted_root()

    The helper also creates the sandbox of a flake whose mounts
    persist on the host, see --persist-mount. Only root can call
    such a flake
    !*/
    set_overlay_root_var();
    match prepare_sandbox(program_name) {
        Ok(call) => run_sandbox(sandbox_id_file, call),
        Err(error) => {
            // Without a root filesystem there is no sandbox and
            // therefore also no instance of it
            delete_sandbox_id_file(sandbox_id_file);
            Err(error)
        }
    }
}

pub fn start_in_mounted_root(program_name: &str) -> Result<ExitCode, FlakeError> {
    /*!
    Replace this pilot by bwrap, which creates the sandbox on the
    root filesystem mounted by the helper

    The helper runs this pilot for a flake with mount points below
    its root filesystem, see prepare_sandbox(). It
    takes the rootfs and the layers from the registration, mounts
    them in the mount namespace of the sandbox and starts the pilot
    in it, with the privileges of the caller. The instance of the
    flake is recorded by the pilot which called the helper.

    For a flake whose mounts persist on the host the helper passes
    the info and the block pipe of bwrap, see get_helper_pipes()
    !*/
    set_overlay_root_var();
    let RuntimeSection { bubblewrap, .. } = config().runtime();
    let (_, _, engine_arguments) = get_engine_arguments(bubblewrap)?;
    let mut call = Command::new(defaults::BWRAP_PATH);
    call.args(get_helper_pipes()?);
    add_sandbox_arguments(&mut call, program_name, engine_arguments);
    if Lookup::is_debug() {
        debug!("{:?} {:?}", call.get_program(), call.get_args());
    }
    Err(call.exec().into())
}

pub fn is_mounted_by_helper() -> bool {
    /*!
    Check if this pilot was started by the helper in the mount
    namespace of a sandbox whose root filesystem it has mounted,
    see start_in_mounted_root()

    The variable is not trusted for anything but the choice to call
    bwrap right away. A caller who sets it gets a sandbox without
    a root filesystem, with no other privileges than its own
    !*/
    env::var_os(defaults::SANDBOX_MOUNTED_VAR).is_some()
}

fn get_helper_pipes() -> Result<Vec<String>, FlakeError> {
    /*!
    Provide the bwrap options which pass the info and the block pipe
    of the helper to bwrap, see defaults::SANDBOX_INFO_FD_VAR

    The helper waits for a sandbox whose mounts persist on the host.
    bwrap reports the process ID of the sandbox to the info pipe and
    holds the application back until the helper writes to the block
    pipe. Like SANDBOX_MOUNTED_VAR the variables are not trusted for
    anything else, they only name descriptors of the caller
    !*/
    let mut options = Vec::new();
    for (name, option) in [
        (defaults::SANDBOX_INFO_FD_VAR, defaults::BWRAP_INFO_FD_OPTION),
        (defaults::SANDBOX_BLOCK_FD_VAR, defaults::BWRAP_BLOCK_FD_OPTION)
    ] {
        if let Ok(fd) = env::var(name) {
            if fd.parse::<u32>().is_err() {
                return Err(FlakeError::IOError {
                    kind: "Invalid environment".to_string(),
                    message: format!("{name}={fd}: a descriptor number is required")
                })
            }
            options.push(option.to_string());
            options.push(fd);
        }
    }
    Ok(options)
}

fn set_overlay_root_var() {
    /*!
    Provide the root of the sandbox, it is referenced as
    %OVERLAYROOT in the options which create the sandbox
    !*/
    env::set_var(
        defaults::OVERLAY_ROOT_VAR,
        overlay::get_dir(defaults::OVERLAY_MERGED_NAME)
    );
}

fn prepare_sandbox(program_name: &str) -> Result<Command, FlakeError> {
    /*!
    Provide the root filesystem of the sandbox and the call which
    creates the sandbox on it

    A flake with mount points below its root filesystem is handed
    over to the helper as a whole, see its bubblewrap-pilot command.
    The helper takes the rootfs, the layers and the bwrap options
    from the registration and only passes the arguments of the
    caller on to the application. This applies to a system wide
    flake, whose registration belongs to root and which a sudo rule
    can grant as a single application, as well as to a flake of
    the caller
    !*/
    let RuntimeSection { bubblewrap, .. } = config().runtime();
    let (layers, persist_dirs, engine_arguments) =
        get_engine_arguments(bubblewrap)?;
    let rootfs = config().sandbox.name;
    let tmpfs_size = get_tmpfs_size();

    // Mounts can only be attached to the mount namespace of the
    // host with root privileges, by the helper
    check_persist_dirs(&persist_dirs, &engine_arguments)?;
    if ! persist_dirs.is_empty() && get_current_uid() != 0 {
        return Err(FlakeError::IOError {
            kind: "Permission denied".to_string(),
            message: format!(
                "The flake lets mounts persist on the host, see {}, \
                only root can call it",
                defaults::BWRAP_PERSIST_MOUNT_OPTION
            )
        })
    }

    if persist_dirs.is_empty() && ! overlay::has_mounts_below(rootfs, &layers)? {
        overlay::mount(
            rootfs, &layers, tmpfs_size.as_deref(), get_sandbox_user()
        )?;
        let mut call = Command::new(defaults::BWRAP_PATH);
        add_sandbox_arguments(&mut call, program_name, engine_arguments);
        return Ok(call)
    }
    if Lookup::is_debug() {
        if persist_dirs.is_empty() {
            debug!("Mount points below the root filesystem, using the helper");
        } else {
            debug!("Mounts persist on the host, using the helper");
        }
    }
    overlay::create_user_dir(get_sandbox_user())?;
    // The application comes first, a sudo rule can name it right
    // after the command, see the reference sudoers file. The helper
    // takes the directories whose mounts persist from the
    // registration as well
    let mut call = helper_call("bubblewrap-pilot");
    call.arg("--app").arg(registration::program_abs_path());
    if let Some(size) = tmpfs_size {
        call.arg("--size").arg(size);
    }
    if let Ok(term) = env::var("TERM") {
        call.arg("--term").arg(term);
    }
    // The arguments of the application follow, unchanged
    call.arg("--").args(env::args().skip(1));
    Ok(call)
}

fn add_sandbox_arguments(
    call: &mut Command, program_name: &str, engine_arguments: Vec<String>
) {
    /*!
    Complete the given call by the bwrap options which create the
    sandbox, followed by the application and its arguments
    !*/
    call.args(get_sandbox_options(
        &overlay::get_dir(defaults::OVERLAY_MERGED_NAME), engine_arguments,
        &get_sandbox_workdir()
    ));
    call.arg(get_target_app_path(program_name));
    call.args(Lookup::get_run_cmdline(Vec::new(), false));
}

fn run_sandbox(
    sandbox_id_file: &str, mut call: Command
) -> Result<ExitCode, FlakeError> {
    /*!
    Call the application in the sandbox

    The given call creates the sandbox. bwrap sets up the new root
    system and replaces itself with the application. The process
    recorded as the instance of the flake is therefore the
    application running in the sandbox, or the sudo process the
    helper runs in, or the helper itself if it lets mounts persist
    on the host. The exit code of the application becomes the exit
    code of the pilot
    !*/
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
                    call.get_program().to_string_lossy()
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

/// Layers of the root of the sandbox, directories whose mounts
/// persist on the host and the options which are passed to bwrap
type EngineArguments = (Vec<String>, Vec<String>, Vec<String>);

pub fn get_engine_arguments(
    options: Option<Vec<&str>>
) -> Result<EngineArguments, FlakeError> {
    /*!
    Read the bwrap options of the flake and provide the layers of
    the root of the sandbox and the directories whose mounts
    persist on the host, along with the other options

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
    layers are part of the overlay created by the pilot, see
    overlay::mount(), they are provided apart from the options
    which are passed to bwrap.

    A directory configured with --persist-mount is handed to the
    helper, see check_persist_dirs(). It is not passed to bwrap
    either
    !*/
    let configured_options = options.unwrap_or_default();
    let engine_options = if configured_options.is_empty() {
        defaults::BWRAP_OPTIONS.to_vec()
    } else {
        configured_options
    };

    let (layers, arguments) = split_options(
        &engine_options, |name| env::var(name).ok()
    ).map_err(|message| FlakeError::IOError {
        kind: "Invalid runtime option".to_string(),
        message
    })?;
    let (persist_dirs, mut engine_arguments) = split_persist_dirs(arguments);

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
    Ok((layers, persist_dirs, engine_arguments))
}

pub fn check_persist_dirs(
    persist_dirs: &[String], engine_arguments: &[String]
) -> Result<(), FlakeError> {
    /*!
    Check the directories whose mounts persist on the host

    A mount the application creates at or below such a directory
    is attached to the host at the same location when the sandbox
    ends, by the helper. The directory is an absolute path, the
    same on the host and in the sandbox, and not the root itself.

    The mounts bwrap creates for the sandbox must not end up on
    the host. No bwrap option may therefore mount at or below such
    a directory, see BWRAP_MOUNT_OPTIONS
    !*/
    for persist_dir in persist_dirs {
        let directory = Path::new(persist_dir);
        if ! directory.is_absolute() || directory == Path::new("/") {
            return Err(FlakeError::IOError {
                kind: "Invalid runtime option".to_string(),
                message: format!(
                    "{} {persist_dir}: an absolute path below / is required",
                    defaults::BWRAP_PERSIST_MOUNT_OPTION
                )
            })
        }
        for (index, argument) in engine_arguments.iter().enumerate() {
            let destination = defaults::BWRAP_MOUNT_OPTIONS.iter()
                .find(|(option, _)| option == argument)
                .and_then(|(_, position)| engine_arguments.get(index + position));
            if let Some(destination) = destination {
                if Path::new(destination).starts_with(directory) {
                    return Err(FlakeError::IOError {
                        kind: "Invalid runtime option".to_string(),
                        message: format!(
                            "{argument} mounts {destination}, at or below \
                            {persist_dir} whose mounts persist on the host"
                        )
                    })
                }
            }
        }
    }
    Ok(())
}

pub fn get_sandbox_options(
    overlay_root: &str, engine_arguments: Vec<String>, workdir: &str
) -> Vec<String> {
    /*!
    Construct the bwrap options which create the sandbox

    The root of the new root system is always the overlay of the
    rootfs created by the pilot for the sandbox, mounted writable.
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
    the sandbox, as requested with the %tmpfs_size pilot option.
    Without it the limit configured system wide applies, if any
    !*/
    Lookup::get_pilot_run_options(config().pilot_options())
        .get(defaults::PILOT_TMPFS_SIZE_OPTION)
        .filter(|size| ! size.is_empty())
        .cloned()
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
    sandbox as running. A sandbox created through the helper is
    recorded with the sudo process the helper runs in, or with the
    helper itself for root whose mounts persist on the host
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
                || process_name == defaults::SUDO_PROCESS_NAME
                || process_name == defaults::HELPER_PROCESS_NAME
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
    myapp@one. The meta data file of the instance is named after it
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
