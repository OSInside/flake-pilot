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
use crate::app_config::AppFireCrackerEngine;
use crate::cli::ListFormat;
use crate::network::{get_effective_boot_args, get_network_info, NetworkInfo};
use crate::volume::{get_volume_info, VolumeInfo};
use crate::{app_config, defaults, output};
use glob::glob;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use flakes::config::{
    get_bubblewrap_ids_dir, get_firecracker_ids_dir,
    get_podman_ids_dir, read_storage_conf
};
use flakes::defaults::FLAKES_DIR_USER;
use flakes::podman::{container_ids, is_known_container};
use flakes::registration;
use uzers::{get_current_uid, get_user_by_uid};
use uzers::os::unix::UserExt;

// InstanceInfo represents one flake instance as it is
// presented by the show command
#[derive(Debug, Serialize)]
pub struct InstanceInfo {
    pub name: String,
    pub user: String,
    pub id: String,
    pub status: String,
    pub image: Option<String>,
    pub config: Option<String>,
    // The setup of a VM instance is provided along with the
    // information above, a container instance provides none
    #[serde(flatten)]
    pub vm: Option<VmInfo>,
}

// VmInfo is the part of a VM instance which does not exist
// for a container instance
#[derive(Debug, Default, Serialize)]
pub struct VmInfo {
    /// Network the VM is connected to. An instance which is not
    /// connected to a network provides none
    pub network: Option<NetworkInfo>,
    /// NFS volumes attached to the VM, an empty list if the
    /// instance mounts no volume
    pub volumes: Vec<VolumeInfo>,
}

pub fn show(engine: &str, usermode: bool, format: ListFormat) {
    /*!
    Print all instances of the given engine in the
    requested output format
    !*/
    let instances = instance_list(engine, usermode);
    match format {
        ListFormat::Table => show_as_table(engine, &instances, usermode),
        ListFormat::Json => output::print_json(&instances),
        ListFormat::Csv => show_as_csv(engine, &instances),
    }
}

pub fn instance_list(engine: &str, usermode: bool) -> Vec<InstanceInfo> {
    /*!
    Read the details of all instances of the given engine

    The instances are found through the meta data files the
    pilots create for them. They are named after the flake and
    stored in the private directory of the user the instance
    belongs to, e.g /tmp/flakes/1000/myapp.vmid
    !*/
    let mut instances: Vec<InstanceInfo> = Vec::new();
    let mut podman_state = PodmanState::new();
    for (uid, meta_dir) in meta_dirs(&ids_dir(engine, usermode)) {
        let glob_pattern = format!(
            "{}/*.{}", meta_dir, id_extension(engine)
        );
        let meta_files = match glob(&glob_pattern) {
            Ok(meta_files) => meta_files,
            Err(error) => {
                error!("Error while traversing {meta_dir}: {error:?}");
                continue
            }
        };
        let mut meta_file_names: Vec<String> = meta_files
            .flatten().map(|path| path.display().to_string()).collect();
        meta_file_names.sort();
        for meta_file in meta_file_names {
            if let Some(instance) = instance_details(
                &meta_file, uid, engine, usermode, &mut podman_state
            ) {
                instances.push(instance)
            }
        }
    }
    instances
}

pub fn running_instances(
    engine: &str, flakes: &[String], usermode: bool
) -> Vec<InstanceInfo> {
    /*!
    Provide the instances of the given flakes which are running

    The instances of a flake are the application itself and the
    ones which were started with an @NAME instance selector. An
    instance whose status cannot be read, e.g a rootless
    container of another user, is not reported as running
    !*/
    instance_list(engine, usermode).into_iter()
        .filter(|instance| instance.status == defaults::INSTANCE_RUNNING)
        .filter(
            |instance| flakes.iter().any(
                |flake| flake == flake_name(&instance.name)
            )
        )
        .collect()
}

fn flake_name(instance_name: &str) -> &str {
    /*!
    Provide the name of the flake the given instance belongs to

    The instance name is the name of the flake plus the @NAME
    selectors the application was called with
    !*/
    instance_name.split('@').next().unwrap_or(instance_name)
}

fn instance_details(
    meta_file: &str, uid: u32, engine: &str, usermode: bool,
    podman_state: &mut PodmanState
) -> Option<InstanceInfo> {
    /*!
    Read the details of the instance the given meta data
    file belongs to
    !*/
    let name = instance_name(meta_file, engine)?;
    let id = read_meta_file(meta_file)?;
    let config = flake_config_file(&name, uid, usermode);
    let FlakeDetails { image, runas, vm } = match config {
        Some(ref config_file) => flake_details(config_file, engine, &name),
        None => FlakeDetails::default()
    };
    let status = if engine == defaults::PODMAN_ENGINE {
        podman_state.status(&id, uid, runas.as_deref(), config.is_some())
    } else if engine == defaults::BUBBLEWRAP_ENGINE {
        sandbox_status(&id)
    } else {
        vm_status(&id)
    };
    // Every VM instance provides a setup, no matter if its flake
    // configuration could be read or not
    let vm = if engine == defaults::FIRECRACKER_ENGINE {
        Some(vm.unwrap_or_default())
    } else {
        None
    };
    Some(
        InstanceInfo {
            name, user: user_name(uid), id, status, image, config, vm
        }
    )
}

fn meta_dirs(ids_dir: &str) -> Vec<(u32, String)> {
    /*!
    Provide the per user meta data directories below ids_dir

    Each user stores the meta data of its instances in a private
    directory named by the user ID. Directories which cannot be
    read, e.g because they belong to another user, are skipped
    !*/
    let mut meta_dirs: Vec<(u32, String)> = Vec::new();
    if ! Path::new(ids_dir).is_dir() {
        // No instance was ever created on this system
        return meta_dirs
    }
    let entries = match fs::read_dir(ids_dir) {
        Ok(entries) => entries,
        Err(error) => {
            error!("Failed to read: {ids_dir}: {error:?}");
            return meta_dirs
        }
    };
    for entry in entries.flatten() {
        let dir_name = entry.file_name().to_string_lossy().to_string();
        match dir_name.parse::<u32>() {
            Ok(uid) => {
                let meta_dir = entry.path();
                if meta_dir.is_dir() {
                    meta_dirs.push((uid, meta_dir.display().to_string()));
                }
            },
            // Not a per user directory
            Err(_) => continue
        }
    }
    meta_dirs.sort();
    meta_dirs
}

fn instance_name(meta_file: &str, engine: &str) -> Option<String> {
    /*!
    Provide the instance name from the given meta data file name

    The instance name is the name of the flake plus an optional
    @NAME suffix which allows to run more than one instance of
    the same flake
    !*/
    let meta_basename = Path::new(meta_file).file_name()?.to_str()?;
    meta_basename
        .strip_suffix(&format!(".{}", id_extension(engine)))
        .map(|name| name.to_string())
}

fn read_meta_file(meta_file: &str) -> Option<String> {
    /*!
    Read the instance ID from the given meta data file

    The meta data file is expected to be a regular file. A
    symbolic link placed there by somebody else would cause
    the read of an unexpected target and is not followed
    !*/
    match fs::symlink_metadata(meta_file) {
        Ok(attributes) => {
            if attributes.file_type().is_symlink() {
                error!("Ignoring symbolic link: {meta_file}");
                return None
            }
        },
        Err(error) => {
            error!("Failed to read: {meta_file}: {error:?}");
            return None
        }
    }
    match fs::read_to_string(meta_file) {
        Ok(id) => Some(id.trim().to_string()),
        Err(error) => {
            error!("Failed to read: {meta_file}: {error:?}");
            None
        }
    }
}

fn flake_config_file(
    name: &str, uid: u32, usermode: bool
) -> Option<String> {
    /*!
    Provide the flake config file for the given instance name

    The instance name refers to the flake it was created from.
    The flake is either registered system wide or in the flakes
    directory of the user the instance belongs to
    !*/
    let flake = flake_name(name);
    let config_file = registration::config_file(flake, usermode);
    if Path::new(&config_file).exists() {
        return Some(config_file)
    }
    if let Some(home) = user_home(uid) {
        let user_config_file = format!(
            "{home}/{FLAKES_DIR_USER}/{flake}.yaml"
        );
        if Path::new(&user_config_file).exists() {
            return Some(user_config_file)
        }
    }
    None
}

// FlakeDetails is the information the show command reads
// from the configuration of a flake
#[derive(Default)]
struct FlakeDetails {
    /// Name of the image the instance was created from
    image: Option<String>,
    /// Name of the user the engine runs as
    runas: Option<String>,
    /// Setup of a VM instance, None for a container instance
    vm: Option<VmInfo>,
}

fn flake_details(
    config_file: &str, engine: &str, name: &str
) -> FlakeDetails {
    /*!
    Read the details of the flake the given instance belongs to

    This is the name of the image the instance was created from
    and the user the engine runs as. A VM instance also provides
    the network and the volumes attached to it
    !*/
    let mut details = FlakeDetails::default();
    let app_conf = match app_config::AppConfig::init_from_file(
        Path::new(config_file)
    ) {
        Ok(app_conf) => app_conf,
        Err(error) => {
            error!(
                "Ignoring error on load or parse flake config {config_file}: {error:?}"
            );
            return details
        }
    };
    if engine == defaults::PODMAN_ENGINE {
        if let Some(container_conf) = app_conf.container {
            details.image = Some(container_conf.name);
            details.runas = container_conf.runtime
                .and_then(|runtime| runtime.runas);
        }
        return details
    }
    if engine == defaults::BUBBLEWRAP_ENGINE {
        if let Some(sandbox_conf) = app_conf.sandbox {
            details.image = Some(sandbox_conf.name);
            details.runas = sandbox_conf.runtime
                .and_then(|runtime| runtime.runas);
        }
        return details
    }
    if let Some(vm_conf) = app_conf.vm {
        details.image = Some(vm_conf.name);
        let mut engine_section = None;
        if let Some(runtime) = vm_conf.runtime {
            details.runas = runtime.runas;
            engine_section = runtime.firecracker;
        }
        details.vm = Some(vm_details(engine_section.as_ref(), name));
    }
    details
}

fn vm_details(
    engine_section: Option<&AppFireCrackerEngine>, name: &str
) -> VmInfo {
    /*!
    Read the network and the volumes attached to the given
    VM instance

    Both are configured as options of the kernel commandline of
    the VM. The options which are in effect for the instance are
    read the same way the pilot does when it creates the VM
    !*/
    let engine_section = match engine_section {
        Some(engine_section) => engine_section,
        None => return VmInfo::default()
    };
    let boot_args = get_effective_boot_args(
        engine_section, instance_selector(name)
    );
    VmInfo {
        network: get_network_info(&boot_args, name),
        volumes: get_volume_info(&boot_args)
    }
}

fn instance_selector(name: &str) -> Option<&str> {
    /*!
    Provide the @NAME selector the given instance was started with

    The instance name is the name of the flake plus the selectors
    the application was called with. An instance of the
    application itself provides none
    !*/
    name.find('@').map(|position| &name[position..])
}

fn vm_status(vmid: &str) -> String {
    /*!
    Provide the status of the VM with the given VM ID

    The VM ID file contains the process ID of the firecracker
    process
    !*/
    process_status(vmid, &[defaults::FIRECRACKER_PROCESS_NAME])
}

fn sandbox_status(sandbox_id: &str) -> String {
    /*!
    Provide the status of the sandbox with the given sandbox ID

    The sandbox ID file contains the process ID of the bwrap
    process. If the sandbox is created for another user than
    the calling one, bwrap is called through sudo and the
    process ID belongs to that sudo call
    !*/
    process_status(
        sandbox_id,
        &[defaults::BUBBLEWRAP_PROCESS_NAME, defaults::SUDO_PROCESS_NAME]
    )
}

fn process_status(id: &str, process_names: &[&str]) -> String {
    /*!
    Provide the status of the instance with the given ID

    The ID is expected to be the process ID of the instance.
    A process ID of zero indicates an instance which was
    created but never started. The name of the process is
    checked too, to not report a process which just reuses the
    ID of an already terminated instance as running
    !*/
    let pid = match id.parse::<u32>() {
        Ok(pid) => pid,
        Err(_) => return defaults::INSTANCE_UNKNOWN.to_string()
    };
    if pid == 0 {
        return defaults::INSTANCE_STOPPED.to_string()
    }
    let process_name_file = format!("{}/{}/comm", defaults::PROC_DIR, pid);
    match fs::read_to_string(process_name_file) {
        Ok(process_name) => {
            if process_names.contains(&process_name.trim()) {
                defaults::INSTANCE_RUNNING.to_string()
            } else {
                defaults::INSTANCE_STOPPED.to_string()
            }
        },
        Err(_) => defaults::INSTANCE_STOPPED.to_string()
    }
}

// PodmanState provides the container IDs podman reports as
// running. The information is read from podman only once per
// storage setup, the system wide one and the rootless one of
// the calling user
struct PodmanState {
    running: HashMap<bool, Option<Vec<String>>>
}

impl PodmanState {
    fn new() -> Self {
        PodmanState { running: HashMap::new() }
    }

    fn status(
        &mut self, cid: &str, uid: u32, runas: Option<&str>, has_config: bool
    ) -> String {
        /*!
        Provide the status of the container with the given cid

        The container is looked up in the podman storage the
        flake uses. Without a flake config this storage is
        unknown. A rootless container of another user cannot be
        looked up either because its storage belongs to that
        user. In both cases the status stays unknown
        !*/
        if ! has_config {
            return defaults::INSTANCE_UNKNOWN.to_string()
        }
        let usermode = runas.unwrap_or("root") != "root";
        if usermode && uid != get_current_uid() {
            return defaults::INSTANCE_UNKNOWN.to_string()
        }
        match self.running_containers(usermode) {
            Some(running_cids) => {
                if is_known_container(cid, running_cids) {
                    defaults::INSTANCE_RUNNING.to_string()
                } else {
                    defaults::INSTANCE_STOPPED.to_string()
                }
            },
            None => defaults::INSTANCE_UNKNOWN.to_string()
        }
    }

    fn running_containers(&mut self, usermode: bool) -> Option<&Vec<String>> {
        /*!
        Ask podman for the IDs of the running containers

        Reading the podman storage setup can fail, e.g if there
        is no rootless setup for the calling user. In this case
        the running containers cannot be looked up
        !*/
        self.running.entry(usermode).or_insert_with(|| {
            if let Err(error) = read_storage_conf(usermode) {
                error!("Failed to read podman storage setup: {error:?}");
                return None
            }
            match container_ids(usermode, false) {
                Ok(container_ids) => Some(container_ids),
                Err(error) => {
                    error!("Failed to read running containers: {error}");
                    None
                }
            }
        }).as_ref()
    }
}

fn ids_dir(engine: &str, usermode: bool) -> String {
    /*!
    Provide the directory the meta data files of the
    given engine are stored below
    !*/
    if engine == defaults::PODMAN_ENGINE {
        get_podman_ids_dir(usermode)
    } else if engine == defaults::BUBBLEWRAP_ENGINE {
        get_bubblewrap_ids_dir(usermode)
    } else {
        get_firecracker_ids_dir(usermode)
    }
}

fn id_extension(engine: &str) -> &'static str {
    /*!
    Provide the file name extension of the meta data
    files of the given engine
    !*/
    if engine == defaults::PODMAN_ENGINE {
        defaults::PODMAN_ID_EXTENSION
    } else if engine == defaults::BUBBLEWRAP_ENGINE {
        defaults::BUBBLEWRAP_ID_EXTENSION
    } else {
        defaults::FIRECRACKER_ID_EXTENSION
    }
}

fn user_name(uid: u32) -> String {
    /*!
    Name of the user with the given user ID
    !*/
    match get_user_by_uid(uid) {
        Some(user) => user.name().to_string_lossy().to_string(),
        None => uid.to_string()
    }
}

fn user_home(uid: u32) -> Option<String> {
    /*!
    Home directory of the user with the given user ID
    !*/
    get_user_by_uid(uid).map(
        |user| user.home_dir().to_string_lossy().to_string()
    )
}

fn show_as_table(engine: &str, instances: &[InstanceInfo], usermode: bool) {
    /*!
    Print instances as human readable table with a headline
    !*/
    println!(
        "Flake {} instances in {}", engine, ids_dir(engine, usermode)
    );
    println!();
    if instances.is_empty() {
        println!("No instance(s) found");
        return;
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    for instance in instances {
        let mut row = vec![
            instance.name.to_string(),
            instance.user.to_string(),
            short_id(&instance.id),
            instance.status.to_string(),
            output::column_value(instance.image.as_ref()),
            output::column_value(instance.config.as_ref()),
        ];
        if engine == defaults::FIRECRACKER_ENGINE {
            row.extend(
                vm_values(instance.vm.as_ref()).iter()
                    .map(|value| output::column_value(value.as_ref()))
            );
        }
        rows.push(row);
    }
    let columns: &[&str] = if engine == defaults::FIRECRACKER_ENGINE {
        &defaults::FLAKE_SHOW_VM_COLUMNS
    } else {
        &defaults::FLAKE_SHOW_COLUMNS
    };
    output::print_table(columns, &rows);
}

fn vm_values(vm: Option<&VmInfo>) -> Vec<Option<String>> {
    /*!
    Provide the address, the TAP device and the volumes of a VM
    instance in the order of the columns of the show command.
    A value which is not configured is provided as None
    !*/
    let network = vm.and_then(|vm| vm.network.as_ref());
    vec![
        network.and_then(|network| network.address)
            .map(|address| address.to_string()),
        network.map(|network| network.tap.to_string()),
        vm.and_then(|vm| volume_list(&vm.volumes))
    ]
}

fn volume_list(volumes: &[VolumeInfo]) -> Option<String> {
    /*!
    Table representation of the volumes attached to a VM. The
    volumes are listed the way they are configured, an instance
    without a volume provides no value
    !*/
    if volumes.is_empty() {
        return None
    }
    Some(
        volumes.iter()
            .map(VolumeInfo::to_string)
            .collect::<Vec<String>>()
            .join(&defaults::NFS_VOLUME_DELIMITER.to_string())
    )
}

fn short_id(id: &str) -> String {
    /*!
    Table representation of an instance ID. Like podman does,
    the container ID is shown abbreviated
    !*/
    if id.is_empty() {
        return defaults::FLAKE_LIST_NO_VALUE.to_string()
    }
    match id.char_indices().nth(defaults::FLAKE_SHOW_ID_LEN) {
        Some((offset, _)) => id[..offset].to_string(),
        None => id.to_string()
    }
}

fn show_as_csv(engine: &str, instances: &[InstanceInfo]) {
    /*!
    Print instances as comma separated values, machine readable.
    Values which could not be read from the flake config are
    printed as empty fields
    !*/
    let mut rows: Vec<Vec<String>> = Vec::new();
    for instance in instances {
        let mut row = vec![
            instance.name.to_string(),
            instance.user.to_string(),
            instance.id.to_string(),
            instance.status.to_string(),
            instance.image.as_deref().unwrap_or_default().to_string(),
            instance.config.as_deref().unwrap_or_default().to_string(),
        ];
        if engine == defaults::FIRECRACKER_ENGINE {
            row.extend(
                vm_values(instance.vm.as_ref()).into_iter()
                    .map(|value| value.unwrap_or_default())
            );
        }
        rows.push(row);
    }
    output::print_csv(&rows);
}

#[cfg(test)]
mod tests {
    use crate::network::NetworkInfo;
    use crate::volume::VolumeInfo;

    use super::{
        flake_name, instance_selector, vm_values, InstanceInfo, VmInfo
    };

    fn volume(server: &str, host_path: &str, guest_path: &str) -> VolumeInfo {
        VolumeInfo {
            server: server.to_string(),
            host_path: host_path.to_string(),
            guest_path: guest_path.to_string()
        }
    }

    fn instance_info(vm: Option<VmInfo>) -> InstanceInfo {
        InstanceInfo {
            name: "myapp".to_string(),
            user: "root".to_string(),
            id: "42".to_string(),
            status: "running".to_string(),
            image: Some("leap".to_string()),
            config: Some("/usr/share/flakes/myapp.yaml".to_string()),
            vm
        }
    }

    #[test]
    fn test_instance_selector() {
        assert_eq!(None, instance_selector("myapp"));
        assert_eq!(Some("@one"), instance_selector("myapp@one"));
        // more than one selector is passed on as it was given
        assert_eq!(Some("@one@two"), instance_selector("myapp@one@two"));
    }

    #[test]
    fn test_flake_name() {
        assert_eq!("myapp", flake_name("myapp"));
        assert_eq!("myapp", flake_name("myapp@one"));
        assert_eq!("myapp", flake_name("myapp@one@two"));
    }

    #[test]
    fn test_vm_values() {
        let vm = VmInfo {
            network: Some(
                NetworkInfo {
                    address: Some("172.16.0.2".parse().unwrap()),
                    tap: "tap-myapp".to_string()
                }
            ),
            volumes: vec![
                volume("172.16.0.1", "/host", "/guest"),
                volume("172.16.0.1", "/other", "/mnt")
            ]
        };
        assert_eq!(
            vec![
                Some("172.16.0.2".to_string()),
                Some("tap-myapp".to_string()),
                Some(
                    "172.16.0.1:/host:/guest,172.16.0.1:/other:/mnt".to_string()
                )
            ],
            vm_values(Some(&vm))
        );
        // a VM without a network and without volumes provides
        // no value, the same as an instance without a config
        assert_eq!(vec![None, None, None], vm_values(Some(&VmInfo::default())));
        assert_eq!(vec![None, None, None], vm_values(None));
    }

    #[test]
    fn test_serialize_instance() {
        // the setup of a VM instance is provided along with the
        // information all instances provide
        let json = serde_json::to_string(
            &instance_info(Some(VmInfo::default()))
        ).unwrap();
        assert!(json.contains(r#""network":null"#));
        assert!(json.contains(r#""volumes":[]"#));
        // a container instance provides none of it
        let json = serde_json::to_string(&instance_info(None)).unwrap();
        assert!(! json.contains("network"));
        assert!(! json.contains("volumes"));
    }
}
