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

use flakes::user::{User, chmod, mkdir};
use flakes::lookup::Lookup;
use flakes::io::IO;
use flakes::error::FlakeError;
use flakes::command::{CommandError, CommandExtTrait};
use flakes::container::Container;
use flakes::config::get_podman_ids_dir;

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

use tempfile::NamedTempFile;

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
        # Run the container engine as a user other than the
        # default target user root. The user may be either
        # a user name or a numeric user-ID (UID) prefixed
        # with the ‘#’ character (e.g. #0 for UID 0). The call
        # of the container engine is performed by sudo.
        # The behavior of sudo can be controlled via the
        # file /etc/sudoers
        runas: root

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
          - --rm
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

    let container_cid_file = format!("{}/{}{suffix}.cid", get_podman_ids_dir(), program_name);

    // setup app command path name to call
    let target_app_path = get_target_app_path(program_name);

    // get runtime section
    let RuntimeSection { runas, resume, attach, podman } = config().runtime();

    // provisioning needs root permissions for mount
    // make sure we have them for this session
    let root_user = User::from("root");
    let mut root = root_user.run("true");
    root.status()?;

    let user = User::from(runas);

    let mut app = user.run("podman");
    app.arg("create")
        .arg("--cidfile").arg(&container_cid_file);

    // Make sure CID dir exists
    init_cid_dir()?;

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
    let has_runtime_args = podman.as_ref().map(|p| !p.is_empty()).unwrap_or_default();
    app.args(podman.iter().flatten().flat_map(|x| x.splitn(2, ' ')));

    if !has_runtime_args {
        if !resume {
            app.arg("--rm");
        }
        app.arg("--tty").arg("--interactive");
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
        app.arg("sleep").arg("4294967295d");
    } else {
        if target_app_path != "/" {
            app.arg(target_app_path);
        }
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
    
    match run_podman_creation(app) {
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

fn run_podman_creation(mut app: Command) -> Result<String, FlakeError> {
    /*!
    Create and provision container prior start
    !*/
    let RuntimeSection { runas, resume, .. } = config().runtime();

    let user = User::from(runas);

    let output: Output = match app.perform() {
        Ok(output) => {
            output
        }
        Err(error) => {
            if resume {
                // Cleanup potentially left over container instance from an
                // inconsistent state, e.g powerfail
                if Lookup::is_debug() {
                    debug!("Force cleanup container instance...");
                }
                let error_pattern = Regex::new(r"in use by (.*)\.").unwrap();
                if let Some(captures) = error_pattern.captures(&format!("{:?}", error.base)) {
                    let cid = captures.get(1).unwrap().as_str();
                    call_instance("rm_force", cid, "none", user)?;
                }
                app.perform()?
            } else {
                return Err(FlakeError::CommandError(error))
            }
        }
    };

    let cid = String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned();

    let is_delta_container = config().container.base_container.is_some();
    let has_includes = !config().tars().is_empty() || !config().paths().is_empty();

    let mut provisioning_failed = None;

    if Lookup::is_debug() {
        debug!("Mounting instance for provisioning workload");
    }
    let instance_mount_point = match mount_container(&cid, user, false) {
        Ok(mount_point) => {
            mount_point
        },
        Err(error) => {
            call_instance("rm", &cid, "none", user)?;
            return Err(error);
        }
    };

    if has_host_dependencies(&instance_mount_point) {
        let removed_files = tempfile()?;
        if Lookup::is_debug() {
            debug!("Syncing host dependencies...");
        }
        update_removed_files(&instance_mount_point, &removed_files)?;
        sync_host(&instance_mount_point, &removed_files, user)?;
    }

    if is_delta_container {
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
            debug!("Adding main app [{}] to layer list", config().container.name);
        }

        for layer in layers {
            if Lookup::is_debug() {
                debug!("Syncing delta dependencies [{layer}]...");
            }
            let app_mount_point = mount_container(layer, user, true)?;
            update_removed_files(&app_mount_point, &removed_files)?;
            IO::sync_data(
                &format!("{}/", app_mount_point),
                &format!("{}/", instance_mount_point),
                [].to_vec(),
                user
            )?;

            let _ = umount_container(layer, user, true);
        }
        if Lookup::is_debug() {
            debug!("Syncing layer host dependencies...");
        }
        sync_host(&instance_mount_point, &removed_files, user)?;
    }

    if has_includes {
        if Lookup::is_debug() {
            debug!("Syncing includes...");
        }
        match IO::sync_includes(
            &instance_mount_point, config().tars(), config().paths(), user
        ) {
            Ok(_) => { },
            Err(error) => {
                provisioning_failed = Some(error);
            }
        }
    }

    let _ = umount_container(&cid, user, false);

    if let Some(provisioning_failed) = provisioning_failed {
        call_instance("rm", &cid, "none", user)?;
        return Err(provisioning_failed);
    }

    Ok(cid)
}

pub fn start(program_name: &str, cid: &str) -> Result<(), FlakeError> {
    /*!
    Start container with the given container ID
    !*/
    let RuntimeSection { runas, resume, attach, .. } = config().runtime();
    
    let user = User::from(runas);

    let is_running = container_running(cid, user)?;

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
    let args: Vec<String> = env::args().collect();

    let RuntimeSection { resume, .. } = config().runtime();

    let mut call = user.run("podman");
    if action == "rm" || action == "rm_force" {
        call.stderr(Stdio::null());
        call.stdout(Stdio::null());
    }
    if action == "rm_force" {
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
        for arg in &args[1..] {
            if ! arg.starts_with('@') {
                call.arg(arg);
            }
        }
    }
    if Lookup::is_debug() {
        debug!("{:?}", call.get_args());
    }
    call.status()?;
    Ok(())
}

pub fn mount_container(
    container_name: &str, user: User, as_image: bool
) -> Result<String, FlakeError> {
    /*!
    Mount container and return mount point
    !*/
    if as_image && ! container_image_exists(container_name, user)? {
        pull(container_name, user)?;
    }
    let storage_conf = NamedTempFile::new().unwrap();
    let user_name = user.get_name();
    let mut call;
    if user_name != "root" {
        let root_user = User::from("root");
        call = root_user.run("bash");
        let _ = Container::podman_write_custom_storage_config(&storage_conf);
        if as_image {
            call.arg("-c").arg(format!(
                "export CONTAINERS_STORAGE_CONF={}; {} image mount {}",
                    storage_conf.path().display(),
                    defaults::PODMAN_PATH,
                    container_name
                )
            );
        } else {
            call.arg("-c").arg(format!(
                "export CONTAINERS_STORAGE_CONF={}; {} mount {}",
                    storage_conf.path().display(),
                    defaults::PODMAN_PATH,
                    container_name
                )
            );
        }
    } else {
        call = user.run("podman");
        if as_image {
            call.arg("image").arg("mount").arg(container_name);
        } else {
            call.arg("mount").arg(container_name);
        }
    }
    if Lookup::is_debug() {
        debug!("{:?}", call.get_args());
    }
    let output = call.perform()?;
    if user_name != "root" {
        Container::podman_fix_storage_permissions(&user_name)?;
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
}

pub fn umount_container(
    mount_point: &str, user: User, as_image: bool
) -> Result<(), FlakeError> {
    /*!
    Umount container image
    !*/
    let storage_conf = NamedTempFile::new().unwrap();
    let user_name = user.get_name();
    let mut call;
    if user_name != "root" {
        let root_user = User::from("root");
        call = root_user.run("bash");
        call.stderr(Stdio::null());
        call.stdout(Stdio::null());
        let _ = Container::podman_write_custom_storage_config(&storage_conf);
        if as_image {
            call.arg("-c").arg(format!(
                "export CONTAINERS_STORAGE_CONF={}; {} image umount {}",
                    storage_conf.path().display(),
                    defaults::PODMAN_PATH,
                    mount_point
                )
            );
        } else {
            call.arg("-c").arg(format!(
                "export CONTAINERS_STORAGE_CONF={}; {} umount {}",
                    storage_conf.path().display(),
                    defaults::PODMAN_PATH,
                    mount_point
                )
            );
        }
    } else {
        call = user.run("podman");
        call.stderr(Stdio::null());
        call.stdout(Stdio::null());
        if as_image {
            call.arg("image").arg("umount").arg(mount_point);
        } else {
            call.arg("umount").arg(mount_point);
        }
    }
    if Lookup::is_debug() {
        debug!("{:?}", call.get_args());
    }
    call.perform()?;
    if user_name != "root" {
        Container::podman_fix_storage_permissions(&user_name)?;
    }
    Ok(())
}

pub fn sync_host(
    target: &String, mut removed_files: &File, user: User
) -> Result<(), FlakeError> {
    /*!
    Sync files/dirs specified in target/defaults::HOST_DEPENDENCIES
    from the running host to the target path
    !*/
    let mut removed_files_contents = String::new();
    let host_deps = format!("{}/{}", &target, defaults::HOST_DEPENDENCIES);
    removed_files.seek(SeekFrom::Start(0))?;
    removed_files.read_to_string(&mut removed_files_contents)?;

    if removed_files_contents.is_empty() {
        if Lookup::is_debug() {
            debug!("There are no host dependencies to resolve");
        }
        return Ok(())
    }

    File::create(&host_deps)?.write_all(removed_files_contents.as_bytes())?;

    let mut call = user.run("rsync");
    call.arg("-av")
        .arg("--ignore-missing-args")
        .arg("--files-from").arg(&host_deps)
        .arg("/")
        .arg(format!("{}/", &target));
    if Lookup::is_debug() {
        debug!("{:?}", call.get_args());
    }

    call.perform()?;
    Ok(())
}

pub fn init_cid_dir() -> Result<(), FlakeError> {
    /*!
    Create meta data directory structure
    !*/
    if ! Path::new(&get_podman_ids_dir()).is_dir() {
        chmod(defaults::CONTAINER_DIR, "755", User::ROOT)?;
        mkdir(&get_podman_ids_dir(), "777", User::ROOT)?;
    }
    Ok(())
}

pub fn container_running(cid: &str, user: User) -> Result<bool, CommandError> {
    /*!
    Check if container with specified cid is running
    !*/
    let mut running_status = false;
    let mut running = user.run("podman");
    running.arg("ps").arg("--format").arg("{{.ID}}");
    if Lookup::is_debug() {
        debug!("{:?}", running.get_args());
    }

    let output = running.perform()?;
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

pub fn container_image_exists(name: &str, user: User) -> Result<bool, std::io::Error> {
    /*!
    Check if container image is present in local registry
    !*/
    let mut exists = user.run("podman");
    exists.arg("image").arg("exists").arg(name);
    if Lookup::is_debug() {
        debug!("{:?}", exists.get_args());
    }
    Ok(exists.status()?.success())
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

    pull.perform()?;

    let mut prune = user.run("podman");
    prune.arg("image").arg("prune").arg("--force");
    match prune.status() {
        Ok(status) => { if Lookup::is_debug() { debug!("{:?}", status) }},
        Err(error) => { if Lookup::is_debug() { debug!("{:?}", error) }}
    }
    Ok(())
}

pub fn has_host_dependencies(target: &String) -> bool {
    /*!
    Check if container provides a /removed file which indicates
    there are files that needs to be provisioned from the host
    !*/
    let host_deps = format!("{}/{}", &target, defaults::HOST_DEPENDENCIES);
    if Path::new(&host_deps).exists() {
        return true
    }
    false
}

pub fn update_removed_files(
    target: &String, mut accumulated_file: &File
) -> Result<(), std::io::Error> {
    /*!
    Take the contents of the given removed_file and append it
    to the accumulated_file
    !*/
    let host_deps = format!("{}/{}", &target, defaults::HOST_DEPENDENCIES);
    if Lookup::is_debug() {
        debug!("Looking up host deps from {}", host_deps);
    }
    if Path::new(&host_deps).exists() {
        let data = fs::read_to_string(&host_deps)?;
        if Lookup::is_debug() {
            debug!("Adding host deps...");
            debug!("{}", &String::from_utf8_lossy(data.as_bytes()));
        }
        accumulated_file.write_all(data.as_bytes())?;
    }
    Ok(())
}

pub fn gc_cid_file(container_cid_file: &String, user: User) -> Result<bool, FlakeError> {
    /*!
    Check if container exists according to the specified
    container_cid_file. Garbage cleanup the container_cid_file
    if no longer present. Return a true value if the container
    exists, in any other case return false.
    !*/
    let cid = fs::read_to_string(container_cid_file)?;
    let mut exists = user.run("podman");
        exists.arg("container").arg("exists").arg(&cid);

    if !exists.status()?.success() {
        fs::remove_file(container_cid_file)?;
        Ok(false)
    } else {
        Ok(true)
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
