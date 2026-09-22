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
use std::fs;
use std::env;
use std::path::Path;
use std::process::{Command, Stdio};
use glob::glob;
use crate::defaults;
use crate::app;
use crate::app_config;
use crate::network;
use flakes::config::get_podman_ids_dir;
use flakes::config::get_podman_storage_conf;
use flakes::config::read_storage_conf;
use flakes::io::IO;
use flakes::lookup::Lookup;
use uzers::{get_current_uid, get_current_username};

pub fn pull(uri: &String, usermode: bool) -> i32 {
    /*!
    Call podman pull and prune with the provided uri
    !*/
    info!("Fetching from registry...");
    info!("podman pull {uri}");
    let mut call = setup_podman_call(usermode);
    call.arg("pull")
        .arg(uri);
    let status = match call.status() {
        Ok(status) => {
            if status.success() {
                status
            } else {
                call.status().unwrap()
            }
        }
        Err(_) => {
            call.status().unwrap()
        }
    };
    let status_code = status.code().unwrap();
    if ! status.success() {
        error!("Failed, error message(s) reported");
    } else {
        info!("podman prune");
        let mut prune = setup_podman_call(usermode);
        let _ = prune.arg("image")
            .arg("prune")
            .arg("--force")
            .status();
    }
    status_code
}

pub fn load(oci: &String, usermode: bool) -> i32 {
    /*!
    Call podman load with the provided oci tar file
    !*/
    info!("Loading OCI image...");
    let mut container_archive: String = oci.to_string();
    if !Path::new(oci).exists() {
        let container_archives = oci.to_owned() + "*";
        // glob puts things in alpha sorted order which is expected to give
        // us the highest version of the archive
        for entry in glob(&container_archives)
            .expect("Failed to read glob pattern").flatten() {
                    container_archive = entry.display().to_string()
            }
        }
    info!("podman load -i {container_archive}");
    let mut call = setup_podman_call(usermode);
    call.arg("load")
        .arg("-i")
        .arg(container_archive);
    let status = match call.status() {
        Ok(status) => {
            if status.success() {
                status
            } else {
                call.status().unwrap()
            }
        }
        Err(_) => {
            call.status().unwrap()
        }
    };

    let status_code = status.code().unwrap();
    if ! status.success() {
        error!("Failed, error message(s) reported");
    } else {
        // prune old images
        info!("podman prune");
        let mut prune = setup_podman_call(usermode);
        let _ = prune.arg("image")
            .arg("prune")
            .arg("--force")
            .status();
    }
    status_code
}

pub fn export(
    container: &str, directory: &str, force: bool, usermode: bool
) -> bool {
    /*!
    Export the file system of the given container to a directory

    An existing directory is taken as an export which was done
    before and is left untouched. Only with force the container
    is exported again, in this case the file system is unpacked
    on top of the contents of that directory
    !*/
    let directory_exists = Path::new(directory).exists();
    if directory_exists && ! force {
        error!("Directory '{directory}' already exists");
        return false
    }
    if ! directory_exists {
        info!("Creating {directory}");
        if let Err(error) = fs::create_dir_all(directory) {
            error!("Failed to create {directory}: {error}");
            return false
        }
    }
    info!("Exporting container {container} to {directory}...");
    let exported = export_container(container, directory, usermode);
    if ! exported && ! directory_exists {
        // A directory created for an export which failed must not
        // stay behind. It would let the next call believe the
        // container was exported already
        if let Err(error) = fs::remove_dir_all(directory) {
            error!("Failed to delete {directory}: {error}");
        }
    }
    exported
}

fn export_container(
    container: &str, directory: &str, usermode: bool
) -> bool {
    /*!
    Create a container instance, unpack its file system into
    the given directory and delete the instance afterwards
    !*/
    let instance = format!(
        "{}{}", defaults::PODMAN_EXPORT_NAME_PREFIX, std::process::id()
    );
    if ! create_instance(container, &instance, usermode) {
        return false
    }
    let unpacked = unpack_instance(&instance, directory, usermode);
    // The instance only exists to provide the file system of the
    // container to the export and is deleted in any case
    delete_instance(&instance, false, usermode);
    unpacked
}

fn create_instance(container: &str, instance: &str, usermode: bool) -> bool {
    /*!
    Create the container instance the export reads from

    The instance is not started, it only provides the file
    system of the container
    !*/
    info!("podman create --name {instance} {container}");
    let mut call = setup_podman_call(usermode);
    call.arg("create")
        .arg("--name")
        .arg(instance)
        .arg(container)
        .stdout(Stdio::null());
    match call.status() {
        Ok(status) => {
            if ! status.success() {
                error!("Failed, error message(s) reported");
                return false
            }
            true
        },
        Err(error) => {
            error!("Failed to execute podman create: {error:?}");
            false
        }
    }
}

fn unpack_instance(instance: &str, directory: &str, usermode: bool) -> bool {
    /*!
    Unpack the file system of the given container instance
    into directory

    'podman export' provides the file system as a tar stream
    which is read by tar unpacking it into the directory
    !*/
    let tool = defaults::TAR_TOOL;
    info!("podman export {instance} | {tool} -x -C {directory}");
    let mut call = setup_podman_call(usermode);
    call.arg("export")
        .arg(instance)
        .stdout(Stdio::piped());
    let mut export = match call.spawn() {
        Ok(export) => export,
        Err(error) => {
            error!("Failed to execute podman export: {error:?}");
            return false
        }
    };
    // The stream of the export is handed over to tar. Without it
    // there is nothing to unpack
    let export_stream = match export.stdout.take() {
        Some(export_stream) => export_stream,
        None => {
            error!("Failed to read the output of podman export");
            let _ = export.wait();
            return false
        }
    };
    let mut unpack = Command::new(tool);
    unpack.arg("-x")
        .arg("-C")
        .arg(directory)
        .stdin(Stdio::from(export_stream));
    let unpack_status = match unpack.status() {
        Ok(unpack_status) => unpack_status,
        Err(error) => {
            error!("Failed to execute {tool}: {error:?}");
            // The export writes into a pipe nobody reads anymore,
            // this lets it terminate
            let _ = export.wait();
            return false
        }
    };
    let export_status = match export.wait() {
        Ok(export_status) => export_status,
        Err(error) => {
            error!("Failed to wait for podman export: {error:?}");
            return false
        }
    };
    if ! export_status.success() {
        error!("Failed, error message(s) reported");
        return false
    }
    if ! unpack_status.success() {
        error!("{tool} failed: {unpack_status:?}");
        return false
    }
    true
}

fn delete_instance(instance: &str, force: bool, usermode: bool) -> bool {
    /*!
    Delete the given container instance

    An instance which is still running is only deleted if the
    deletion is forced
    !*/
    let force_option = if force { " --force" } else { "" };
    info!("podman rm{force_option} {instance}");
    let mut call = setup_podman_call(usermode);
    call.arg("rm");
    if force {
        call.arg("--force");
    }
    call.arg(instance)
        .stdout(Stdio::null());
    match call.status() {
        Ok(status) => {
            if ! status.success() {
                error!("Failed to delete container instance {instance}");
                return false
            }
            true
        },
        Err(error) => {
            error!("Failed to execute podman rm: {error:?}");
            false
        }
    }
}

pub fn reset(
    app: &str, instance: Option<&String>, usermode: bool
) -> bool {
    /*!
    Stop and delete the container instance of a resume flake

    An application registered with the resume option keeps its
    container instance in running state such that the next call
    of the application is done inside of that instance. This
    command deletes the instance which lets the next call start
    from a freshly created container.

    Called without an instance selector the container of the
    application itself is deleted. Instances started with a
    '@NAME' selector each run in their own container and are
    addressed by providing that selector
    !*/
    let config_file = match network::get_flake_config_file(app, usermode) {
        Some(config_file) => config_file,
        None => return false
    };
    // Only a resume flake keeps a container instance behind which
    // could be reset. The engine of the flake runs as the
    // configured user, its instances live in the podman storage
    // of that user
    let runas = match get_resume_runas(&config_file) {
        Some(runas) => runas,
        None => return false
    };
    let podman_usermode = runas != "root";

    let cid_file = match get_cid_file(app, instance, usermode) {
        Some(cid_file) => cid_file,
        None => return false
    };
    if fs::symlink_metadata(&cid_file).is_err() {
        info!("No container instance of {app} found");
        return true
    }
    let cid = match read_cid_file(&cid_file) {
        Some(cid) => cid,
        None => return false
    };
    match container_exists(&cid, podman_usermode) {
        Some(true) => {
            if ! stop_instance(&cid, podman_usermode) {
                return false
            }
            if ! delete_instance(&cid, true, podman_usermode) {
                return false
            }
        },
        Some(false) => {
            info!("Container {cid} does not exist (anymore)");
        },
        // The state of the container could not be read, deleting
        // the meta data of an instance which might still be
        // around would orphan it
        None => return false
    }
    // The meta data file is only valid along with the instance
    // it was written for
    info!("Deleting {cid_file}");
    if let Err(error) = fs::remove_file(&cid_file) {
        error!("Failed to delete {cid_file}: {error:?}");
        return false
    }
    true
}

fn get_resume_runas(config_file: &str) -> Option<String> {
    /*!
    Provide the user the engine of the given flake runs as

    The flake is required to be a container application which is
    registered with the resume option. Only such a flake keeps
    its container instance running after the application ended
    !*/
    let app_conf = match app_config::AppConfig::init_from_file(
        Path::new(config_file)
    ) {
        Ok(app_conf) => app_conf,
        Err(error) => {
            error!("Failed to load or parse {config_file}: {error:?}");
            return None
        }
    };
    let container_conf = match app_conf.container {
        Some(container_conf) => container_conf,
        None => {
            error!("{config_file} is not a container registration");
            return None
        }
    };
    let runtime_conf = container_conf.runtime;
    if runtime_conf.as_ref()
        .and_then(|runtime_conf| runtime_conf.resume) != Some(true)
    {
        error!("{config_file} is not registered with 'resume: true'");
        error!("Only a resume flake keeps a container instance running");
        return None
    }
    Some(
        runtime_conf.and_then(|runtime_conf| runtime_conf.runas)
            .unwrap_or_else(|| "root".to_string())
    )
}

fn get_cid_file(
    app: &str, instance: Option<&String>, usermode: bool
) -> Option<String> {
    /*!
    Provide the path of the container ID file which podman-pilot
    wrote for the given flake instance

    The file is named after the application plus the '@NAME'
    instance selector it was called with and is stored in the
    private meta data directory of the calling user
    !*/
    let mut meta_name = network::get_app_basename(app)?;
    if let Some(instance) = instance {
        let instance = network::get_instance_name(instance);
        if ! Lookup::is_safe_instance_name(&instance) {
            error!(
                "The instance name {instance} contains characters \
                which are not allowed"
            );
            return None
        }
        meta_name.push_str(&instance);
    }
    Some(
        format!(
            "{}/{}/{}.{}",
            get_podman_ids_dir(usermode), get_current_uid(),
            meta_name, defaults::PODMAN_ID_EXTENSION
        )
    )
}

fn read_cid_file(cid_file: &str) -> Option<String> {
    /*!
    Read the container ID from the given container ID file

    The file is stored in a directory which is shared with the
    other users of the system. A symbolic link placed there by
    somebody else is not followed, it would cause the read of
    an unexpected target
    !*/
    if let Err(error) = IO::no_symlink(cid_file) {
        error!("{error}");
        return None
    }
    let cid = match fs::read_to_string(cid_file) {
        Ok(cid) => cid.trim().to_string(),
        Err(error) => {
            error!("Failed to read {cid_file}: {error:?}");
            return None
        }
    };
    if cid.is_empty() {
        error!("No container ID found in {cid_file}");
        return None
    }
    Some(cid)
}

fn container_exists(cid: &str, usermode: bool) -> Option<bool> {
    /*!
    Check if the container with the given ID is known to podman

    A container which is not known is reported as false. If the
    lookup itself failed no statement about the container can be
    made and none is provided
    !*/
    let mut call = setup_podman_call(usermode);
    call.arg("ps")
        .arg("--all")
        .arg("--format").arg("{{.ID}}");
    let output = match call.output() {
        Ok(output) => output,
        Err(error) => {
            error!("Failed to execute podman ps: {error:?}");
            return None
        }
    };
    if ! output.status.success() {
        error!(
            "Failed to read the list of containers: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return None
    }
    Some(is_known_container(cid, &String::from_utf8_lossy(&output.stdout)))
}

fn is_known_container(cid: &str, ps_output: &str) -> bool {
    /*!
    Look up the given container ID in the output of a
    'podman ps --all --format {{.ID}}' call
    !*/
    ps_output.lines()
        .filter(|known_cid| ! known_cid.is_empty())
        // podman reports the container IDs abbreviated
        .any(|known_cid| cid.starts_with(known_cid))
}

fn stop_instance(cid: &str, usermode: bool) -> bool {
    /*!
    Stop the container instance of a resume flake

    The instance is kept in running state by a sleep process
    which podman-pilot starts as the entry point of the
    container. As long as this process is alive the container
    stays up, it is therefore killed before podman is asked to
    stop the instance
    !*/
    for pid in get_resume_pids(cid, usermode) {
        kill_process(cid, &pid, usermode);
    }
    info!("podman stop {cid}");
    let mut call = setup_podman_call(usermode);
    call.arg("stop")
        .arg(cid)
        .stdout(Stdio::null());
    match call.status() {
        Ok(status) => {
            if ! status.success() {
                error!("Failed to stop container instance {cid}");
                return false
            }
            true
        },
        Err(error) => {
            error!("Failed to execute podman stop: {error:?}");
            false
        }
    }
}

fn get_resume_pids(cid: &str, usermode: bool) -> Vec<String> {
    /*!
    Provide the IDs of the sleep processes which keep the given
    container instance in running state

    The IDs are the ones inside of the container because this is
    where the processes are killed. A container which provides
    no such process, e.g because it is not running anymore,
    provides an empty list
    !*/
    let sleep_process = defaults::PODMAN_RESUME_PROCESS_NAME;
    info!("podman top {cid} pid comm");
    let mut call = setup_podman_call(usermode);
    call.arg("top")
        .arg(cid)
        .arg("pid")
        .arg("comm");
    let output = match call.output() {
        Ok(output) => output,
        Err(error) => {
            error!("Failed to execute podman top: {error:?}");
            return Vec::new()
        }
    };
    if ! output.status.success() {
        error!(
            "Failed to read the processes of {cid}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Vec::new()
    }
    let pids = get_pids_of_process(
        &String::from_utf8_lossy(&output.stdout), sleep_process
    );
    if pids.is_empty() {
        warn!("No {sleep_process} process found in {cid}");
    }
    pids
}

fn get_pids_of_process(top_output: &str, process_name: &str) -> Vec<String> {
    /*!
    Read the IDs of the given process from the output of a
    'podman top CID pid comm' call

    The output is a table of the processes of the container with
    the process ID in its first and the process name in its
    second column
    !*/
    let mut pids: Vec<String> = Vec::new();
    // The first line of the output is the header of the table
    for process in top_output.lines().skip(1) {
        let mut columns = process.split_whitespace();
        if let (Some(pid), Some(command)) = (columns.next(), columns.next()) {
            if command == process_name {
                pids.push(pid.to_string())
            }
        }
    }
    pids
}

fn kill_process(cid: &str, pid: &str, usermode: bool) -> bool {
    /*!
    Kill the process with the given ID inside of the container

    Killing the process which keeps the container alive lets the
    container terminate. The call is therefore allowed to fail,
    this happens if the container is gone before podman could
    report the result of the exec
    !*/
    let kill = defaults::KILL_TOOL;
    info!("podman exec {cid} {kill} -9 {pid}");
    let mut call = setup_podman_call(usermode);
    call.arg("exec")
        .arg(cid)
        .arg(kill)
        .arg("-9")
        .arg(pid)
        .stdout(Stdio::null());
    match call.status() {
        Ok(status) => {
            if ! status.success() {
                warn!("Failed to kill process {pid} in {cid}");
                return false
            }
            true
        },
        Err(error) => {
            warn!("Failed to execute podman exec: {error:?}");
            false
        }
    }
}

pub fn rm(container: &String, usermode: bool) {
    /*!
    Call podman image rm with force option to remove all running containers
    !*/
    info!("Removing image and all running containers...");
    info!("podman rm -f {container}");

    let mut call = setup_podman_call(usermode);
    call.arg("image")
        .arg("rm")
        .arg("-f")
        .arg(container);
    let status = match call.status() {
        Ok(status) => {
            if ! status.success() {
                status
            } else {
                call.status().unwrap()
            }
        }
        Err(_) => {
            call.status().unwrap()
        }
    };
    if ! status.success() {
        error!("Failed, error message(s) reported");
    }
}

pub fn mount_container(container_name: &str) -> String {
    /*!
    Mount container and return mount point,
    or an empty string in the error case
    !*/
    let mut call = setup_podman_call(false);
    call.arg("image")
        .arg("mount")
        .arg(container_name);
    let output = match call.output() {
        Ok(output) => {
            output
        }
        Err(_) => {
            call.output().unwrap()
        }
    };
    if output.status.success() {
        return String::from_utf8_lossy(&output.stdout)
            .strip_suffix('\n').unwrap().to_string()
    }
    error!(
        "Failed to mount container image: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    "".to_string()
}

pub fn umount_container(container_name: &str) -> i32 {
    /*!
    Umount container image
    !*/
    let mut call = setup_podman_call(false);
    call.arg("image")
        .arg("umount")
        .arg(container_name);
    let output = match call.output() {
        Ok(output) => {
            output
        }
        Err(_) => {
            call.output().unwrap()
        }
    };
    output.status.code().unwrap()
}

pub fn purge_container(container: &str, usermode: bool) {
    /*!
    Iterate over all yaml config files and find those connected
    to the container. Delete all app registrations for this
    container and also delete the container from the local
    registry
    !*/
    for registration in app::image_flakes(
        container, defaults::PODMAN_ENGINE, usermode
    ) {
        app::remove(
            &registration.host_app_path,
            defaults::PODMAN_PILOT,
            usermode,
            false,
            false
        );
    }
    rm(&container.to_string(), usermode);
}

pub fn print_container_info(container: &str) {
    /*!
    Print app info file

    Lookup container_base_name.yaml file in the root of the
    specified container and print the file if it is present
    !*/
    let container_basename = Path::new(
        container
    ).file_name().unwrap().to_str().unwrap();
    let image_mount_point = mount_container(container);
    if image_mount_point.is_empty() {
        return
    }
    let info_file = format!(
        "{image_mount_point}/{container_basename}.yaml"
    );
    if Path::new(&info_file).exists() {
        match fs::read_to_string(&info_file) {
            Ok(data) => {
                println!(
                    "{}", String::from_utf8_lossy(data.as_bytes())
                );
            },
            Err(error) => {
                // info_file file exists but could not be read
                error!("Error reading {info_file}: {error:?}");
            }
        }
    } else {
        error!("No info file {container_basename}.yaml found in container: {container}"
        );
    }
    umount_container(container);
}

pub fn setup_podman_call(usermode: bool) -> Command {
    let storage = read_storage_conf(usermode).unwrap();
    let calling_user_name = get_current_username().unwrap();
    let container_runroot = format!(
        "{}/{}",
        storage.get("runroot").unwrap(),
        calling_user_name.to_str().unwrap()
    );
    env::set_var("CONTAINERS_STORAGE_CONF", get_podman_storage_conf(usermode));
    env::set_var("XDG_RUNTIME_DIR", &container_runroot);
    let mut call = Command::new("sudo");
    // Only the variables set above are handed over to the podman
    // call. Passing the complete environment of the caller to a
    // process running as root allows to influence that process in
    // ways the sudo rule for it never intended
    call.arg(format!(
        "--preserve-env={}",
        ["CONTAINERS_STORAGE_CONF", "XDG_RUNTIME_DIR"].join(",")
    ));
    if usermode {
        call.arg("--user").arg(calling_user_name);
    }
    call.arg(defaults::PODMAN_PATH);
    call
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_export_into_existing_directory() {
        // The export of a directory which exists is refused
        // unless it is forced. No container is touched in
        // this case
        let directory = tempdir().unwrap();
        assert!(! export(
            "name", directory.path().to_str().unwrap(), false, true
        ));
    }

    #[test]
    fn test_get_cid_file() {
        let meta_dir = format!("/tmp/flakes/{}", get_current_uid());
        // the instance of the application itself
        assert_eq!(
            Some(format!("{meta_dir}/myapp.cid")),
            get_cid_file("/usr/bin/myapp", None, false)
        );
        // an instance started with a '@NAME' selector. For
        // convenience the selector is accepted without its
        // leading '@' marker
        assert_eq!(
            Some(format!("{meta_dir}/myapp@one.cid")),
            get_cid_file("/usr/bin/myapp", Some(&"one".to_string()), false)
        );
        assert_eq!(
            Some(format!("{meta_dir}/myapp@one.cid")),
            get_cid_file("/usr/bin/myapp", Some(&"@one".to_string()), false)
        );
    }

    #[test]
    fn test_get_cid_file_of_invalid_app() {
        // the application has to be given as an absolute path
        assert_eq!(None, get_cid_file("myapp", None, false));
        // an instance name which is not safe to be used in a
        // file name is refused
        assert_eq!(
            None,
            get_cid_file("/usr/bin/myapp", Some(&"../one".to_string()), false)
        );
    }

    #[test]
    fn test_is_known_container() {
        let ps_output = "8c9a3b1d4e5f\n1a2b3c4d5e6f\n";
        // podman reports the container IDs abbreviated, the ID
        // file of an instance holds the complete one
        assert!(is_known_container("1a2b3c4d5e6f78901234", ps_output));
        assert!(! is_known_container("deadbeefcafe12345678", ps_output));
        // an empty list must not match any container
        assert!(! is_known_container("1a2b3c4d5e6f78901234", "\n"));
    }

    #[test]
    fn test_get_pids_of_process() {
        let top_output = "\
PID         COMMAND
1           catatonit
7           sleep
21          bash
";
        assert_eq!(
            vec!["7".to_string()], get_pids_of_process(top_output, "sleep")
        );
        // a container without the process provides no ID, the
        // same applies to output which carries no process at all
        assert!(get_pids_of_process(top_output, "sleepy").is_empty());
        assert!(get_pids_of_process("PID         COMMAND\n", "sleep")
            .is_empty());
    }
}
