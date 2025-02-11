//
// Copyright (c) 2022 Elektrobit Automotive GmbH
// Copyright (c) 2023 Marcus Schäfer
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
use crate::config::{RuntimeSection, config};

use atty::Stream;

use flakes::user::{User, mkdir};
use flakes::lookup::Lookup;
use flakes::io::IO;
use flakes::error::FlakeError;
use flakes::command::{CommandError, CommandExtTrait};
use flakes::container::Container;
use flakes::config::get_podman_ids_dir;

use std::io;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::env;
use std::fs;
use std::io::{Write, Read};
use std::fs::File;
use std::io::Seek;
use std::io::SeekFrom;

use spinoff::{Spinner, spinners, Color};
use tempfile::tempfile;
use regex::Regex;

use users::{get_current_username};

pub fn create(
    program_name: &String
) -> Result<(String, String), FlakeError> {
    /*!
    Create container for later execution of program_name.
    The container name and all other settings to run the program
    inside of the container are taken from the config file(s)

    CONTAINER_FLAKE_DIR/
       ├── program_name.d
       │   └── other.yaml
       └── program_name.yaml

    All commandline options will be passed to the program_name
    called in the container. An example program config file
    looks like the following:

    container:
      name: name
      target_app_path: path/to/program/in/container
      host_app_path: path/to/program/on/host

      # Optional base container to use with a delta 'container: name'
      # If specified the given 'container: name' is expected to be
      # an overlay for the specified base_container. podman-pilot
      # combines the 'container: name' with the base_container into
      # one overlay and starts the result as a container instance
      #
      # Default: not_specified
      base_container: name

      # Optional additional container layers on top of the
      # specified base container
      layers:
        - name_A
        - name_B

      runtime:
        # Resume the container from previous execution.
        # If the container is still running, the app will be
        # executed inside of this container instance.
        #
        # Default: false
        resume: true|false

        # Attach to the container if still running, rather than
        # executing the app again. Only makes sense for interactive
        # sessions like a shell running as app in the container.
        #
        # Default: false
        attach: true|false

        podman:
          - --storage-opt size=10G
          - -ti

    include:
      tar:
        - tar-archive-file-name-to-include
      path:
        - file-or-directory-to-include

    Calling this method returns a vector including the
    container ID and and the name of the container ID
    file.
    !*/
    // Read optional @NAME pilot argument to differentiate
    // simultaneous instances of the same container application
    let (name, _): (Vec<_>, Vec<_>) = env::args().skip(1).partition(|arg| arg.starts_with('@'));

    // setup container ID file name
    let suffix = name.first().map(String::as_str).unwrap_or("");

    // setup app command path name to call
    let target_app_path = get_target_app_path(program_name);

    // get runtime section
    let RuntimeSection { resume, attach, podman, .. } = config().runtime();

    // provisioning needs root permissions for mount
    // make sure we have them for this session
    let root_user = User::from("root");
    let mut root = root_user.run("true");
    root.status()?;

    mkdir(defaults::FLAKES_REGISTRY, "777", User::ROOT)?;

    let current_user = get_current_username().unwrap();
    let user = User::from(current_user.to_str().unwrap());

    let container_cid_file = format!(
        "{}/{}{suffix}_{}.cid",
        get_podman_ids_dir(), program_name, current_user.to_str().unwrap()
    );

    let container_runroot = format!(
        "{}/{}",
        defaults::FLAKES_REGISTRY_RUNROOT, current_user.to_str().unwrap()
    );

    mkdir(&container_runroot, "777", User::ROOT)?;

    let mut app = user.run("podman");
    app.arg("create")
        .arg("--cidfile").arg(&container_cid_file);

    // Make sure CID dir exists
    init_cid_dir()?;

    env::set_var("CONTAINERS_STORAGE_CONF", defaults::FLAKES_STORAGE);
    env::set_var("XDG_RUNTIME_DIR", &container_runroot);

    let _ = Container::podman_setup_run_permissions();

    // Check early return condition in resume mode
    if Path::new(&container_cid_file).exists() && gc_cid_file(&container_cid_file, user)? && (resume || attach) {
        // resume or attach mode is active and container exists
        // report ID value and its ID file name
        let cid = fs::read_to_string(&container_cid_file)?;
        return Ok((cid, container_cid_file));
    }

    // Garbage collect occasionally
    gc(user)?;

    // Sanity check
    if Path::new(&container_cid_file).exists() {
        return Err(FlakeError::AlreadyRunning);
    }

    // create the container with configured runtime arguments
    let var_pattern = Regex::new(r"%([A-Z]+)").unwrap();
    for arg in podman.iter().flatten().flat_map(|x| x.splitn(2, ' ')) {
        let mut arg_value = arg.to_string();
        while var_pattern.captures(&arg_value.clone()).is_some() {
            for capture in var_pattern.captures_iter(&arg_value.clone()) {
                // replace %VAR placeholder(s) with the respective
                // environment variable value if possible.
                // If not possible replace by the variable name
                let var_name = capture.get(1).unwrap().as_str();
                let var_value = env::var(var_name)
                    .unwrap_or(format!("${}", var_name));
                arg_value = arg_value.replace(
                    &format!("%{}", var_name), &var_value
                );
            }
        }
        app.arg(arg_value);
    };

    // set default runtime arguments if none configured
    let has_runtime_args = podman
        .as_ref().map(|p| !p.is_empty()).unwrap_or_default();
    if !has_runtime_args {
        app.arg("--tty").arg("--interactive");
    }

    if target_app_path != "/" {
        if resume {
            app.arg("--entrypoint").arg("sleep");
        } else {
            app.arg("--entrypoint").arg(target_app_path.clone());
        }
    }

    // setup container name to use
    app.arg(config().container.base_container.unwrap_or(config().container.name));

    // setup entry point
    if resume {
        // create the container with a sleep entry point
        // to keep it in running state
        // sleep "forever" ... I will be dead by the time this sleep ends ;)
        // keeps the container in running state to accept podman exec for
        // running the app multiple times with different arguments
        // Note: This requires the sleep program to be found in the container
        if target_app_path != "/" {
            app.arg("4294967295d");
        } else {
            // If the target_app_path is set to / this means the
            // container configured entry point is called. Such a
            // setup cannot be used as resume flake because we
            // don't know the entry point command to exec
            return Err(FlakeError::UnknownCommand)
        }
    } else {
        for arg in Lookup::get_run_cmdline(Vec::new(), false) {
            app.arg(arg);
        }
    }
    
    // create container
    if Lookup::is_debug() {
        debug!("{:?}", app.get_args());
    }
    let pilot_options = Lookup::get_pilot_run_options();
    let mut spinner = None;
    if ! pilot_options.contains_key("%silent") {
        spinner = Some(
            Spinner::new_with_stream(
                spinners::Line, "Launching flake...",
                Color::Yellow, spinoff::Streams::Stderr
            )
        );
    }

    let mut ignore_sync_error = false;
    if pilot_options.contains_key("%ignore_sync_error") {
        ignore_sync_error = true
    }

    match run_podman_creation(app, ignore_sync_error) {
        Ok(cid) => {
            if let Some(spinner) = spinner {
                spinner.success("Launching flake");
            }
            Ok((cid, container_cid_file))
        },
        Err(err) => {
            if let Some(spinner) = spinner {
                spinner.fail("Flake launch has failed");
            }
            Err(err)            
        },
    }
}

fn run_podman_creation(
    mut app: Command, ignore_sync_error: bool
) -> Result<String, FlakeError> {
    /*!
    Create and provision container prior start
    !*/
    let RuntimeSection { resume, .. } = config().runtime();

    let root_user = User::from("root");

    let output: Output = match app.perform() {
        Ok(output) => {
            output
        }
        Err(error) => {
            let error_pattern = Regex::new(r".*(not permitted|permission denied).*").unwrap();
            if error_pattern.captures(&format!("{:?}", error.base)).is_some() {
                // On permission error, fix permissions and try again
                // This is an expensive operation depending on the storage size
                let _ = Container::podman_setup_permissions();
                app.perform()?
            } else if resume {
                // Cleanup potentially left over container instance from an
                // inconsistent state, e.g powerfail
                if Lookup::is_debug() {
                    debug!("Force cleanup container instance...");
                }
                let error_pattern = Regex::new(r"in use by (.*)\.").unwrap();
                if let Some(captures) = error_pattern.captures(&format!("{:?}", error.base)) {
                    let cid = captures.get(1).unwrap().as_str();
                    call_instance("rm_force", cid, "none", root_user)?;
                }
                app.perform()?
            } else {
                return Err(FlakeError::CommandError(error))
            }
        }
    };

    let cid = String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned();

    let is_delta_container = config().container.base_container.is_some();
    let check_host_dependencies = config().container.check_host_dependencies;
    let has_includes = !config().tars().is_empty() || !config().paths().is_empty();

    let mut provisioning_failed = None;

    if is_delta_container || check_host_dependencies {
        if Lookup::is_debug() {
            debug!("Mounting instance for provisioning workload");
        }
        let instance_mount_point = match mount_container(&cid, false) {
            Ok(mount_point) => {
                mount_point
            },
            Err(error) => {
                call_instance("rm", &cid, "none", root_user)?;
                return Err(error);
            }
        };

        // lookup and sync host dependencies from systemfiles script
        let mut ignore_missing = false;
        let system_files = tempfile()?;
        match build_system_dependencies(
            &instance_mount_point, defaults::SYSTEM_HOST_DEPENDENCIES,
            &system_files, root_user
        ) {
            Ok(_) => {
                if Lookup::is_debug() {
                    debug!("Syncing system dependencies...");
                }
                match sync_host(
                    &instance_mount_point, &system_files,
                    root_user, ignore_missing,
                    defaults::SYSTEM_HOST_DEPENDENCIES
                ) {
                    Ok(_) => { },
                    Err(error) => {
                        if ! ignore_sync_error {
                            provisioning_failed = Some(error)
                        }
                    }
                }
            },
            Err(error) => {
                if ! ignore_sync_error {
                    provisioning_failed = Some(error)
                }
            }
        }

        // lookup and sync host dependencies from removed data
        if provisioning_failed.is_none() {
            ignore_missing = true;
            let removed_files = tempfile()?;
            update_removed_files(&instance_mount_point, &removed_files)?;
            sync_host(
                &instance_mount_point, &removed_files,
                root_user, ignore_missing,
                defaults::HOST_DEPENDENCIES
            )?;
        }

        if is_delta_container && provisioning_failed.is_none() {
            // Create tmpfile to hold accumulated removed data from layers
            let removed_files = tempfile()?;
            if Lookup::is_debug() {
                debug!("Provisioning delta container...");
            }
            let layers = config().layers();
            let layers = layers.iter()
                .inspect(|layer| if Lookup::is_debug() { debug!("Adding layer: [{layer}]") })
                .chain(Some(&config().container.name));

            if Lookup::is_debug() {
                debug!(
                    "Adding main app [{}] to layer list",
                    config().container.name
                );
            }

            for layer in layers {
                if Lookup::is_debug() {
                    debug!("Syncing delta dependencies [{layer}]...");
                }
                let app_mount_point = mount_container(layer, true)?;
                update_removed_files(&app_mount_point, &removed_files)?;
                IO::sync_data(
                    &format!("{}/", app_mount_point),
                    &format!("{}/", instance_mount_point),
                    [].to_vec(),
                    root_user
                )?;

                let _ = umount_container(layer, true);
            }
            if Lookup::is_debug() {
                debug!("Syncing layer host dependencies...");
            }
            sync_host(
                &instance_mount_point, &removed_files,
                root_user, ignore_missing,
                defaults::HOST_DEPENDENCIES
            )?;
        }

        if has_includes && provisioning_failed.is_none() {
            if Lookup::is_debug() {
                debug!("Syncing includes...");
            }
            match IO::sync_includes(
                &instance_mount_point, config().tars(),
                config().paths(), root_user
            ) {
                Ok(_) => { },
                Err(error) => {
                    provisioning_failed = Some(error);
                }
            }
        }

        let _ = umount_container(&cid, false);
    }

    if let Some(provisioning_failed) = provisioning_failed {
        call_instance("rm", &cid, "none", root_user)?;
        return Err(provisioning_failed);
    }

    Ok(cid)
}

pub fn start(program_name: &str, cid: &str) -> Result<(), FlakeError> {
    /*!
    Start container with the given container ID
    !*/
    let RuntimeSection { resume, attach, .. } = config().runtime();

    let pilot_options = Lookup::get_pilot_run_options();
    let current_user = get_current_username().unwrap();
    let user = User::from(current_user.to_str().unwrap());

    let is_running = container_running(cid, user)?;
    let is_created = container_exists(cid, user)?;
    let mut is_removed = false;

    if is_running {
        if attach {
            // 1. Attach to running container
            call_instance("attach", cid, program_name, user)?;
        } else {
            // 2. Execute app in running container
            call_instance("exec", cid, program_name, user)?;
        }
    } else if resume {
        // 3. Startup resume type container and execute app
        call_instance("start", cid, program_name, user)?;
        call_instance("exec", cid, program_name, user)?;
    } else {
        // 4. Startup container
        call_instance("start", cid, program_name, user)?;
        if ! attach || ! is_created {
            call_instance("rm_force", cid, program_name, user)?;
            is_removed = true
        }
    };

    if pilot_options.contains_key("%remove") && ! is_removed {
        call_instance("rm_force", cid, program_name, user)?;
    };
    Ok(())
}

pub fn get_target_app_path(program_name: &str) -> String {
    /*!
    setup application command path name

    This is either the program name specified at registration
    time or the configured target application from the flake
    configuration file
    !*/
    config().container.target_app_path.unwrap_or(program_name).to_owned()
}

pub fn call_instance(
    action: &str, cid: &str, program_name: &str, user: User
) -> Result<(), FlakeError> {
    /*!
    Call container ID based podman commands
    !*/
    let RuntimeSection { resume, .. } = config().runtime();

    let pilot_options = Lookup::get_pilot_run_options();
    let mut interactive = false;
    if pilot_options.contains_key("%interactive") {
        interactive = true;
    }

    let mut call = user.run("podman");
    if action == "rm" || action == "rm_force" {
        call.stdout(Stdio::null());
        call.arg("rm").arg("--force");
    } else {
        call.arg(action);
    }
    if action == "exec" {
        call.arg("--interactive");
        call.arg("--tty");
    }
    if action == "start" && ! resume {
        call.arg("--attach");
    } else if action == "start" {
        // start detached, we are not interested in the
        // start output in this case
        call.stdout(Stdio::null());
    }
    call.arg(cid);
    if action == "exec" {
        call.arg(
            get_target_app_path(program_name)
        );
        for arg in Lookup::get_run_cmdline(Vec::new(), false) {
            call.arg(arg);
        }
    }
    if Lookup::is_debug() {
        debug!("{:?}", call.get_args());
    }
    if interactive || atty::is(Stream::Stdout) {
        call.status()?;
    } else {
        match call.output() {
            Ok(output) => {
                let _ = io::stdout().write_all(&output.stdout);
                let _ = io::stderr().write_all(&output.stderr);
            },
            Err(_) => {
                let _ = Container::podman_setup_permissions();
                call.output()?;
            }
        };
    }
    Ok(())
}

pub fn mount_container(
    container_name: &str, as_image: bool
) -> Result<String, FlakeError> {
    /*!
    Mount container and return mount point
    !*/
    let root_user = User::from("root");
    if as_image && ! container_image_exists(container_name, root_user)? {
        pull(container_name, root_user)?;
    }
    let mut call = root_user.run("podman");
    if as_image {
        call.arg("image").arg("mount").arg(container_name);
    } else {
        call.arg("mount").arg(container_name);
    }
    if Lookup::is_debug() {
        debug!("{:?}", call.get_args());
    }
    let output = call.perform()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
}

pub fn umount_container(
    mount_point: &str, as_image: bool
) -> Result<(), FlakeError> {
    /*!
    Umount container image
    !*/
    let root_user = User::from("root");
    let mut call = root_user.run("podman");
    call.stderr(Stdio::null());
    call.stdout(Stdio::null());
    if as_image {
        call.arg("image").arg("umount").arg(mount_point);
    } else {
        call.arg("umount").arg(mount_point);
    }
    if Lookup::is_debug() {
        debug!("{:?}", call.get_args());
    }
    call.perform()?;
    Ok(())
}

pub fn sync_host(
    target: &String, mut removed_files: &File, user: User,
    ignore_missing: bool, from: &str
) -> Result<(), FlakeError> {
    /*!
    Sync files/dirs specified in target/from, from the running
    host to the target path
    !*/
    let mut removed_files_contents = String::new();
    let files_from = format!("{}/{}", &target, from);
    removed_files.seek(SeekFrom::Start(0))?;
    removed_files.read_to_string(&mut removed_files_contents)?;

    if removed_files_contents.is_empty() {
        if Lookup::is_debug() {
            debug!("There are no host dependencies to resolve");
        }
        return Ok(())
    }

    File::create(&files_from)?.write_all(removed_files_contents.as_bytes())?;

    let mut call = user.run("rsync");
    call.arg("-av");
    if ignore_missing {
        call.arg("--ignore-missing-args");
    }
    call.arg("--files-from").arg(&files_from)
        .arg("/")
        .arg(format!("{}/", &target));
    if Lookup::is_debug() {
        debug!("{:?}", call.get_args());
    }
    match call.output() {
        Ok(output) => {
            if Lookup::is_debug() {
                debug!("{}", String::from_utf8_lossy(&output.stdout));
                debug!("{}", String::from_utf8_lossy(&output.stderr));
            }
            if ! output.status.success() && ! ignore_missing {
                return Err(
                    FlakeError::IOError {
                        kind: "rsync transfer incomplete".to_string(),
                        message: "Please run with PILOT_DEBUG=1 for details".to_string()
                    }
                );
            }
        }
        Err(error) => {
            return Err(flakes::error::FlakeError::IO(error))
        }
    }
    Ok(())
}

pub fn init_cid_dir() -> Result<(), FlakeError> {
    /*!
    Create meta data directory structure
    !*/
    if ! Path::new(&get_podman_ids_dir()).is_dir() {
        mkdir(&get_podman_ids_dir(), "777", User::ROOT)?;
    }
    Ok(())
}

pub fn container_exists(cid: &str, user: User) -> Result<bool, FlakeError> {
    /*!
    Check if container exists according to the specified cid
    !*/
    let mut exists = user.run("podman");
    exists.arg("container").arg("exists").arg(cid);
    if Lookup::is_debug() {
        debug!("{:?}", exists.get_args());
    }
    let output = match exists.output() {
        Ok(output) => {
            output
        }
        Err(error) => {
            let error_pattern = Regex::new(r".*(not permitted|permission denied).*").unwrap();
            if error_pattern.captures(&format!("{:?}", error)).is_some() {
                // On permission error, fix permissions and try again
                // This is an expensive operation depending on the storage size
                let _ = Container::podman_setup_permissions();
                exists.output()?
            } else {
                return Err(
                    FlakeError::IOError {
                        kind: "call failed".to_string(),
                        message: format!("{:?}", error)
                    }
                );
            }
        }
    };
    if output.status.success() {
        return Ok(true)
    }
    Ok(false)
}

pub fn container_running(cid: &str, user: User) -> Result<bool, CommandError> {
    /*!
    Check if container with specified cid is running
    !*/
    let mut running_status = false;
    let mut running = user.run("podman");
    running.arg("ps")
        .arg("--format").arg("{{.ID}}");
    if Lookup::is_debug() {
        debug!("{:?}", running.get_args());
    }
    let output: Output = match running.perform() {
        Ok(output) => {
            output
        }
        Err(error) => {
            let error_pattern = Regex::new(r".*(not permitted|permission denied).*").unwrap();
            if error_pattern.captures(&format!("{:?}", error.base)).is_some() {
                // On permission error, fix permissions and try again
                // This is an expensive operation depending on the storage size
                let _ = Container::podman_setup_permissions();
                running.perform()?
            } else {
                return Err(error)
            }
        }
    };
    let mut running_cids = String::new();
    running_cids.push_str(
        &String::from_utf8_lossy(&output.stdout)
    );
    for running_cid in running_cids.lines() {
        if cid.starts_with(running_cid) {
            running_status = true;
            break
        }
    }
    Ok(running_status)
}

pub fn container_image_exists(name: &str, user: User) -> Result<bool, FlakeError> {
    /*!
    Check if container image is present in local registry
    !*/
    let mut exists = user.run("podman");
    exists.arg("image").arg("exists").arg(name);
    if Lookup::is_debug() {
        debug!("{:?}", exists.get_args());
    }
    let output: Output = match exists.output() {
        Ok(output) => {
            output
        }
        Err(error) => {
            let error_pattern = Regex::new(r".*(not permitted|permission denied).*").unwrap();
            if error_pattern.captures(&format!("{:?}", error)).is_some() {
                // On permission error, fix permissions and try again
                // This is an expensive operation depending on the storage size
                let _ = Container::podman_setup_permissions();
                exists.output()?
            } else {
                return Err(
                    FlakeError::IOError {
                        kind: "call failed".to_string(),
                        message: format!("{:?}", error)
                    }
                );
            }
        }
    };
    if output.status.success() {
        return Ok(true)
    }
    Ok(false)
}

pub fn pull(uri: &str, user: User) -> Result<(), FlakeError> {
    /*!
    Call podman pull and prune with the provided uri
    !*/
    let mut pull = user.run("podman");
    pull.arg("pull").arg(uri);
    if Lookup::is_debug() {
        debug!("{:?}", pull.get_args());
    }
    match pull.perform() {
        Ok(output) => {
            output
        }
        Err(error) => {
            let error_pattern = Regex::new(r".*(not permitted|permission denied).*").unwrap();
            if error_pattern.captures(&format!("{:?}", error.base)).is_some() {
                let _ = Container::podman_setup_permissions();
                pull.perform()?
            } else {
                return Err(FlakeError::CommandError(error))
            }
        }
    };
    let mut prune = user.run("podman");
    prune.arg("image").arg("prune").arg("--force");
    match prune.status() {
        Ok(status) => { if Lookup::is_debug() { debug!("{:?}", status) }},
        Err(error) => { if Lookup::is_debug() { debug!("{:?}", error) }}
    }
    Ok(())
}

pub fn build_system_dependencies(
    target: &String, dependency_file: &str, mut file: &File, user: User
) -> Result<bool, FlakeError> {
    /*!
    Check if container provides a /systemfiles script which
    contains code to build up a list of files that needs
    to be provisioned from the host
    !*/
    let system_deps = format!("{}/{}", &target, dependency_file);
    if Path::new(&system_deps).exists() {
        if Lookup::is_debug() {
            debug!("Calling system deps generator: {}", system_deps);
        }
        let mut call = user.run("sh");
        call.arg(system_deps);
        if Lookup::is_debug() {
            debug!("{:?}", call.get_args());
        }
        match call.output() {
            Ok(output) => {
                if output.status.success() {
                    file.write_all(&output.stdout)?;
                    return Ok(true);
                } else {
                    if Lookup::is_debug() {
                        debug!("{}", String::from_utf8_lossy(&output.stdout));
                        debug!("{}", String::from_utf8_lossy(&output.stderr));
                    }
                    return Err(
                        FlakeError::IOError {
                            kind: "system deps generator failed".to_string(),
                            message: "Please run with PILOT_DEBUG=1 for details".to_string()
                        }
                    );
                }
            },
            Err(error) => {
                return Err(
                    FlakeError::IOError {
                        kind: "call failed".to_string(),
                        message: format!("{:?}", error)
                    }
                );
            }
        };
    }
    Ok(false)
}

pub fn update_removed_files(
    target: &String, mut accumulated_file: &File
) -> Result<(), std::io::Error> {
    /*!
    Take the contents of the given removed_file and append it
    to the accumulated_file
    !*/
    let host_deps = format!("{}/{}", &target, defaults::HOST_DEPENDENCIES);
    if Path::new(&host_deps).exists() {
        if Lookup::is_debug() {
            debug!("Adding host deps from {}", host_deps);
        }
        let data = fs::read_to_string(&host_deps)?;
        // The subsequent rsync call logs enough information
        // Let's keep this for convenience debugging
        // if Lookup::is_debug() {
        //     debug!("Adding host deps...");
        //     debug!("{}", &String::from_utf8_lossy(data.as_bytes()));
        // }
        accumulated_file.write_all(data.as_bytes())?;
    }
    Ok(())
}

pub fn gc_cid_file(
    container_cid_file: &String, user: User
) -> Result<bool, FlakeError> {
    /*!
    Check if container exists according to the specified
    container_cid_file. Garbage cleanup the container_cid_file
    if no longer present. Return a true value if the container
    exists, in any other case return false.
    !*/
    let cid = fs::read_to_string(container_cid_file)?;

    if container_exists(&cid, user)? {
        Ok(true)
    } else {
        fs::remove_file(container_cid_file)?;
        Ok(false)
    }
}

pub fn gc(user: User) -> Result<(), FlakeError> {
    /*!
    Garbage collect CID files for which no container exists anymore
    !*/
    let mut cid_file_names: Vec<String> = Vec::new();
    let mut cid_file_count: i32 = 0;
    let paths;
    match fs::read_dir(get_podman_ids_dir()) {
        Ok(result) => { paths = result },
        Err(error) => {
            return Err(FlakeError::IOError {
                kind: format!("{:?}", error.kind()),
                message: format!("fs::read_dir failed on {}: {}",
                    get_podman_ids_dir(), error
                )
            })
        }
    };
    for path in paths {
        cid_file_names.push(format!("{}", path?.path().display()));
        cid_file_count += 1;
    }
    if cid_file_count > defaults::GC_THRESHOLD {
        for container_cid_file in cid_file_names {
            let _ = gc_cid_file(&container_cid_file, user);
        }
    }
    Ok(())
}
