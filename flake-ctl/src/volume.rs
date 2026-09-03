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
use std::collections::HashMap;
use std::net::Ipv4Addr;

use crate::app_config::{AppFireCrackerEngine, AppFireCrackerInstance};
use crate::defaults;
use crate::network::{
    boot_arg_name, get_engine_section, get_flake_config_file, get_instance_key,
    get_instance_name, read_flake_config, unset_boot_arg, write_flake_config
};

pub fn add(
    app: &str, volumes: &[String], instance: Option<&String>, usermode: bool
) -> bool {
    /*!
    Add the given volumes to the flake configuration

    A volume is a host path which is provided to the VM through
    NFS. The guest mounts it from the nfs= option of its kernel
    commandline. The server of the volume is the gateway of the
    private network between the host and the VMs, it is read from
    the network setup of the flake.

    Called with an instance selector the volumes are added to the
    section of that instance and are therefore only mounted if
    the application is called with that selector
    !*/
    let volumes = match get_volumes(volumes) {
        Some(volumes) => volumes,
        None => return false
    };
    let config_file = match get_flake_config_file(app, usermode) {
        Some(config_file) => config_file,
        None => return false
    };
    let instance = instance.map(|instance| get_instance_name(instance));
    let mut yaml_config = match read_flake_config(&config_file) {
        Some(yaml_config) => yaml_config,
        None => return false
    };
    let engine_section = match get_engine_section(
        &mut yaml_config, &config_file, "volume"
    ) {
        Some(engine_section) => engine_section,
        None => return false
    };
    let server = match get_nfs_server(
        engine_section, instance.as_deref(), &config_file
    ) {
        Some(server) => server,
        None => return false
    };

    // The volume list of an instance takes the place of the global
    // one, the volumes of the application are not merged into it
    let global_volumes = get_nfs_entries(
        engine_section.boot_args.as_deref().unwrap_or_default()
    );

    let boot_args = match instance.as_deref() {
        Some(instance) => {
            if ! global_volumes.is_empty() {
                warn!("The volumes of {instance} take the place of:");
                for entry in &global_volumes {
                    warn!("  {entry}");
                }
            }
            let instances = engine_section.instance
                .get_or_insert_with(HashMap::new);
            let instance_key = get_instance_key(instances, instance);
            let instance_section = instances.entry(instance_key)
                .or_insert(AppFireCrackerInstance { boot_args: None });
            instance_section.boot_args.get_or_insert_with(Vec::new)
        },
        None => engine_section.boot_args.get_or_insert_with(Vec::new)
    };

    let mut entries = get_nfs_entries(boot_args);
    for volume in &volumes {
        let entry = format!("{server}:{}", volume.paths);
        match entries.iter().position(
            |configured| get_entry_paths(configured) == Some(&volume.paths)
        ) {
            Some(position) => {
                if entries[position] == entry {
                    info!("Keeping volume {entry}");
                } else {
                    info!("Replacing volume {} by {entry}", entries[position]);
                    entries[position] = entry;
                }
            },
            None => {
                info!("Adding volume {entry}");
                entries.push(entry);
            }
        }
    }
    set_nfs_boot_arg(boot_args, &entries);

    if ! write_flake_config(&config_file, &yaml_config) {
        return false
    }
    info!("Updated {config_file}");
    info!("The host path of a volume has to be exported, see:");
    info!("  flake-ctl firecracker volume export --path <PATH>");
    true
}

pub fn remove(
    app: &str, volumes: &[String], instance: Option<&String>, usermode: bool
) -> bool {
    /*!
    Delete the given volumes from the flake configuration

    A volume is matched by its host and guest path, no matter
    which server it is provided from. Sections which became empty
    are dropped to leave the configuration as it was before the
    volumes were added
    !*/
    let volumes = match get_volumes(volumes) {
        Some(volumes) => volumes,
        None => return false
    };
    let config_file = match get_flake_config_file(app, usermode) {
        Some(config_file) => config_file,
        None => return false
    };
    let instance = instance.map(|instance| get_instance_name(instance));
    let mut yaml_config = match read_flake_config(&config_file) {
        Some(yaml_config) => yaml_config,
        None => return false
    };
    let engine_section = match get_engine_section(
        &mut yaml_config, &config_file, "volume"
    ) {
        Some(engine_section) => engine_section,
        None => return false
    };

    match instance.as_deref() {
        Some(instance) => delete_instance_volumes(
            engine_section, instance, &volumes
        ),
        None => {
            if let Some(boot_args) = engine_section.boot_args.as_mut() {
                delete_volumes(boot_args, &volumes);
            } else {
                report_unconfigured(&volumes);
            }
        }
    }

    if let Some(true) = engine_section.boot_args.as_ref().map(Vec::is_empty) {
        engine_section.boot_args = None;
    }
    if let Some(true) = engine_section.instance.as_ref().map(HashMap::is_empty)
    {
        engine_section.instance = None;
    }

    if ! write_flake_config(&config_file, &yaml_config) {
        return false
    }
    info!("Updated {config_file}");
    true
}

// Volume is a host path provided to a VM through NFS
struct Volume {
    /// The 'HOST_PATH:GUEST_PATH' pair of the volume
    paths: String
}

fn get_volumes(volumes: &[String]) -> Option<Vec<Volume>> {
    /*!
    Read the given volume specifications

    An invalid specification lets the entire command fail, the
    configuration is not touched in that case
    !*/
    volumes.iter().map(|volume| get_volume(volume)).collect()
}

fn get_volume(volume: &str) -> Option<Volume> {
    /*!
    Read the host and the guest path of the given volume

    A volume is specified as 'HOST_PATH:GUEST_PATH'. The guest
    path is the last element of the specification, everything in
    front of it is the path on the host
    !*/
    let (host_path, guest_path) = match volume.rsplit_once(':') {
        Some(paths) => paths,
        None => {
            error!("Invalid volume {volume:?}");
            error!("Expected format: /some/local/path:/some/guest/path");
            return None
        }
    };
    for path in [host_path, guest_path] {
        if ! path.starts_with('/') {
            error!(
                "Volume path {path:?} must be specified with an absolute path"
            );
            return None
        }
        // The volume becomes part of a kernel commandline option
        // which is one word of a comma separated list
        if path.contains(char::is_whitespace)
            || path.contains(char::is_control)
        {
            error!("Volume path {path:?} contains unsupported characters");
            return None
        }
        if path.contains(defaults::NFS_VOLUME_DELIMITER) {
            error!(
                "Volume path {path:?} must not contain {:?}",
                defaults::NFS_VOLUME_DELIMITER
            );
            return None
        }
    }
    Some(Volume { paths: format!("{host_path}:{guest_path}") })
}

fn delete_instance_volumes(
    engine_section: &mut AppFireCrackerEngine, instance: &str,
    volumes: &[Volume]
) {
    /*!
    Delete the given volumes from the given instance

    An instance section which provides nothing else is deleted
    along with the volumes
    !*/
    let instances = match engine_section.instance.as_mut() {
        Some(instances) => instances,
        None => return report_unconfigured(volumes)
    };
    let instance_key = if instances.contains_key(instance) {
        instance.to_string()
    } else {
        instance.trim_start_matches('@').to_string()
    };
    let instance_section = match instances.get_mut(&instance_key) {
        Some(instance_section) => instance_section,
        None => return report_unconfigured(volumes)
    };
    let is_empty = match instance_section.boot_args.as_mut() {
        Some(boot_args) => {
            delete_volumes(boot_args, volumes);
            boot_args.is_empty()
        },
        None => {
            report_unconfigured(volumes);
            true
        }
    };
    if is_empty {
        instances.remove(&instance_key);
    }
}

fn delete_volumes(boot_args: &mut Vec<String>, volumes: &[Volume]) {
    /*!
    Delete the given volumes from the given boot_args
    !*/
    let mut entries = get_nfs_entries(boot_args);
    for volume in volumes {
        let configured = entries.len();
        entries.retain(
            |entry| get_entry_paths(entry) != Some(&volume.paths)
        );
        if entries.len() == configured {
            info!("No volume {} configured", volume.paths);
        } else {
            info!("Deleting volume {}", volume.paths);
        }
    }
    set_nfs_boot_arg(boot_args, &entries);
}

fn report_unconfigured(volumes: &[Volume]) {
    /*!
    Tell that none of the given volumes is configured
    !*/
    for volume in volumes {
        info!("No volume {} configured", volume.paths);
    }
}

fn get_nfs_entries(boot_args: &[String]) -> Vec<String> {
    /*!
    Provide the volumes configured in the given boot_args

    The volumes are provided as a comma separated list of the
    nfs= option. More than one of these options is folded into
    one list
    !*/
    boot_args.iter()
        .filter(
            |boot_arg| boot_arg_name(boot_arg) == defaults::NFS_VOLUME_BOOT_ARG
        )
        .filter_map(|boot_arg| boot_arg.split_once('='))
        .flat_map(
            |(_, volumes)| volumes.split(defaults::NFS_VOLUME_DELIMITER)
        )
        .map(str::trim)
        .filter(|entry| ! entry.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn get_entry_paths(entry: &str) -> Option<&str> {
    /*!
    Provide the 'HOST_PATH:GUEST_PATH' pair of a volume entry

    An entry is specified as 'NAME_OR_IP:HOST_PATH:GUEST_PATH',
    the server it is provided from is its first element
    !*/
    entry.split_once(':').map(|(_, paths)| paths)
}

fn set_nfs_boot_arg(boot_args: &mut Vec<String>, entries: &[String]) {
    /*!
    Write the given volumes to the nfs= option of the given
    boot_args

    All volumes are kept in one option, sci reads them from a
    single kernel commandline variable. The option keeps the
    place it had and is deleted if no volume is left
    !*/
    let position = boot_args.iter().position(
        |boot_arg| boot_arg_name(boot_arg) == defaults::NFS_VOLUME_BOOT_ARG
    );
    unset_boot_arg(boot_args, defaults::NFS_VOLUME_BOOT_ARG);
    if entries.is_empty() {
        return
    }
    let boot_arg = format!(
        "{}={}",
        defaults::NFS_VOLUME_BOOT_ARG,
        entries.join(&defaults::NFS_VOLUME_DELIMITER.to_string())
    );
    match position {
        Some(position) if position < boot_args.len() => {
            boot_args.insert(position, boot_arg)
        },
        _ => boot_args.push(boot_arg)
    }
}

fn get_nfs_server(
    engine_section: &AppFireCrackerEngine, instance: Option<&str>,
    config_file: &str
) -> Option<Ipv4Addr> {
    /*!
    Provide the address the volumes are provided from

    The volumes are exported by the host which the guest reaches
    through the gateway of the private network. The address of
    that gateway is part of the network setup of the flake and is
    therefore read from its configuration
    !*/
    let gateway = instance
        .and_then(|instance| get_instance_boot_args(engine_section, instance))
        .and_then(get_boot_args_gateway)
        .or_else(
            || engine_section.boot_args.as_deref().and_then(
                get_boot_args_gateway
            )
        );
    if gateway.is_none() {
        error!("No gateway address found in {config_file}");
        error!("The address is read from the rd.route= boot option");
        error!(
            "Please run 'flake-ctl firecracker network add --app <APP>' first"
        );
    }
    gateway
}

fn get_instance_boot_args<'a>(
    engine_section: &'a AppFireCrackerEngine, instance: &str
) -> Option<&'a [String]> {
    /*!
    Provide the boot_args of the given instance
    !*/
    let instances = engine_section.instance.as_ref()?;
    let instance_section = instances.get(instance).or_else(
        || instances.get(instance.trim_start_matches('@'))
    )?;
    instance_section.boot_args.as_deref()
}

fn get_boot_args_gateway(boot_args: &[String]) -> Option<Ipv4Addr> {
    /*!
    Read the gateway address from the rd.route= option

    The option is written in the 'rd.route=GATEWAY/PREFIX::DEVICE'
    format, a route without a prefix length is accepted as well
    !*/
    boot_args.iter()
        .filter_map(|boot_arg| boot_arg.strip_prefix("rd.route="))
        .filter_map(
            |route_option| route_option.split(['/', ':']).next()
        )
        .find_map(|address| address.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::{
        delete_volumes, get_boot_args_gateway, get_entry_paths, get_nfs_entries,
        get_volume, get_volumes, set_nfs_boot_arg
    };

    fn boot_args(boot_args: &[&str]) -> Vec<String> {
        boot_args.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn test_get_volume() {
        assert_eq!(
            "/host:/guest", get_volume("/host:/guest").unwrap().paths
        );
        // the guest path is the last element
        assert_eq!(
            "/host:with:colon:/guest",
            get_volume("/host:with:colon:/guest").unwrap().paths
        );
        // no separator
        assert!(get_volume("/host").is_none());
        // relative paths
        assert!(get_volume("host:/guest").is_none());
        assert!(get_volume("/host:guest").is_none());
        // empty guest path
        assert!(get_volume("/host:").is_none());
        // characters which break the volume list
        assert!(get_volume("/host:/guest dir").is_none());
        assert!(get_volume("/host,dir:/guest").is_none());
    }

    #[test]
    fn test_get_volumes_fails_for_one_invalid_volume() {
        let volumes = boot_args(&["/host:/guest", "invalid"]);
        assert!(get_volumes(&volumes).is_none());
    }

    #[test]
    fn test_get_nfs_entries() {
        let configured = boot_args(&[
            "ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off",
            "nfs=172.16.0.1:/host:/guest, 172.16.0.1:/other:/mnt",
            "nfs=172.16.0.1:/more:/data"
        ]);
        assert_eq!(
            vec![
                "172.16.0.1:/host:/guest",
                "172.16.0.1:/other:/mnt",
                "172.16.0.1:/more:/data"
            ],
            get_nfs_entries(&configured)
        );
        assert!(get_nfs_entries(&boot_args(&["nfs="])).is_empty());
    }

    #[test]
    fn test_get_entry_paths() {
        assert_eq!(
            Some("/host:/guest"), get_entry_paths("172.16.0.1:/host:/guest")
        );
        assert_eq!(None, get_entry_paths("no_separator"));
    }

    #[test]
    fn test_set_nfs_boot_arg_keeps_the_place_of_the_option() {
        let mut configured = boot_args(&[
            "nfs=172.16.0.1:/host:/guest", "nameserver=8.8.8.8"
        ]);
        set_nfs_boot_arg(
            &mut configured,
            &boot_args(&["172.16.0.1:/host:/guest", "172.16.0.1:/other:/mnt"])
        );
        assert_eq!(
            boot_args(&[
                "nfs=172.16.0.1:/host:/guest,172.16.0.1:/other:/mnt",
                "nameserver=8.8.8.8"
            ]),
            configured
        );
    }

    #[test]
    fn test_set_nfs_boot_arg_folds_and_appends() {
        let mut configured = boot_args(&["nameserver=8.8.8.8"]);
        set_nfs_boot_arg(
            &mut configured, &boot_args(&["172.16.0.1:/host:/guest"])
        );
        assert_eq!(
            boot_args(&["nameserver=8.8.8.8", "nfs=172.16.0.1:/host:/guest"]),
            configured
        );
    }

    #[test]
    fn test_delete_volumes() {
        let mut configured = boot_args(&[
            "nfs=172.16.0.1:/host:/guest,172.16.0.1:/other:/mnt",
            "nameserver=8.8.8.8"
        ]);
        delete_volumes(
            &mut configured, &get_volumes(&boot_args(&["/other:/mnt"])).unwrap()
        );
        assert_eq!(
            boot_args(&["nfs=172.16.0.1:/host:/guest", "nameserver=8.8.8.8"]),
            configured
        );
        // the option is deleted with the last volume
        delete_volumes(
            &mut configured,
            &get_volumes(&boot_args(&["/host:/guest"])).unwrap()
        );
        assert_eq!(boot_args(&["nameserver=8.8.8.8"]), configured);
    }

    #[test]
    fn test_get_boot_args_gateway() {
        assert_eq!(
            Some("172.16.0.1".parse().unwrap()),
            get_boot_args_gateway(&boot_args(&[
                "ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off",
                "rd.route=172.16.0.1/24::eth0"
            ]))
        );
        assert_eq!(
            None,
            get_boot_args_gateway(&boot_args(&["rd.route=default::eth0"]))
        );
        assert_eq!(None, get_boot_args_gateway(&boot_args(&["ip=dhcp"])));
    }
}
