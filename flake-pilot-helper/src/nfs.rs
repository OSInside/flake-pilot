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
//! NFS exports of host directories for the firecracker VMs
//!
//! A VM has no way to share a directory with the host directly.
//! A host directory is exported through NFS to exactly the VM of
//! a flake instance instead, identified by its address in the
//! network state.
//!
use std::fs;
use std::io;
use std::net::Ipv4Addr;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;

use flakes::defaults;
use flakes::network::{
    get_flake_allocations, is_vm_address, lock_network_state,
    read_network_state, Allocation
};

use crate::caller::Caller;
use crate::network::replace_file;
use crate::run::run_ok;
use crate::validate;

pub fn export(
    caller: &Caller, path: &str, config_file: &str, instance: Option<&str>
) -> bool {
    /*!
    Export the given host path via NFS to the VM of a flake

    The export is limited to the VM the given flake instance runs
    in, which is identified by the address allocated to it in the
    network state. Exporting the same path for further flakes or
    instances adds their VMs to the export.

    Only a path owned by a user can be exported, and only by that
    user or by root. Every access of a VM is mapped to that user,
    no matter which user, including root, performs it inside of the
    VM. A VM therefore gets the permissions of the owner of the path
    on it and nothing more
    !*/
    if ! validate_volume_export_path(path, true) {
        return false
    }
    let owner = match get_volume_owner(caller, path) {
        Some(owner) => owner,
        None => return false
    };
    let guest = match get_guest_address(caller, config_file, instance) {
        Ok(guest) => guest,
        Err(error) => {
            error!("{error}");
            return false
        }
    };
    let update = NfsExportUpdate::Add {
        guest, options: nfs_export_options(&owner)
    };
    if ! update_nfs_exports(path, update) {
        return false
    }
    if nfs_server_is_running() {
        reload_nfs_exports()
    } else {
        start_nfs_server()
    }
}

pub fn release(
    caller: &Caller, path: &str, config_file: Option<&str>,
    instance: Option<&str>
) -> bool {
    /*!
    Remove the given host path from the NFS exports

    Called with a flake configuration only the VM of that flake
    instance is removed from the export. The export itself is
    deleted once no VM is left. Without a flake configuration the
    export is deleted.

    An export maps every access to the owner of the path, recorded
    in its anonuid option. Only that user and root may release it.
    This also holds for a path which does not exist anymore. An
    export without a recorded owner, written by a former version
    for the whole network of the VMs, is only released by root
    !*/
    if ! validate_volume_export_path(path, false) {
        return false
    }
    let exports = match read_nfs_exports() {
        Some(exports) => exports,
        None => return false
    };
    let is_exported = exports.lines().any(
        |line| is_flake_pilot_nfs_export(line, path)
    );
    match get_export_owner(&exports, path) {
        Some(owner) if ! caller.may_act_for(owner) => {
            error!(
                "The NFS export for {path} belongs to the user ID {owner}"
            );
            return false
        },
        None if is_exported && ! caller.is_root() => {
            error!(
                "The NFS export for {path} has no recorded owner, \
                only root can release it"
            );
            return false
        },
        _ => { }
    }
    let update = match config_file {
        Some(config_file) => match get_guest_address(
            caller, config_file, instance
        ) {
            Ok(guest) => NfsExportUpdate::RemoveGuest(guest),
            Err(error) => {
                error!("{error}");
                return false
            }
        },
        None => NfsExportUpdate::RemoveAll
    };
    match get_updated_nfs_exports(&exports, path, update) {
        Some(updated_exports) => {
            write_nfs_exports(&updated_exports) && restart_nfs_server()
        },
        None => true
    }
}

fn get_guest_address(
    caller: &Caller, config_file: &str, instance: Option<&str>
) -> Result<Ipv4Addr, String> {
    /*!
    Provide the address of the VM the given flake instance runs in

    The address is the one allocated to the instance in the network
    state. The ip= option in the flake configuration is not used,
    the configuration of a user flake belongs to that user and could
    name the VM of another user. A user flake is only run by its
    owner, an allocation of another user does not count for it
    !*/
    let flake = validate::flake_config(caller, config_file)?;
    let instance = instance.map(validate::instance_selector).transpose()?;
    let _lock = lock_network_state(false).ok_or(
        format!("Failed to lock {}", defaults::NETWORK_STATE)
    )?;
    let state = read_network_state().ok_or(
        format!("Failed to read {}", defaults::NETWORK_STATE)
    )?;
    let allocations: Vec<&Allocation> = get_flake_allocations(
        &state, &flake.path, instance.as_deref()
    ).into_iter()
        .filter(|allocation| flake.is_system() || allocation.uid == flake.owner)
        .collect();
    let instance_text = instance.as_deref()
        .map(|instance| format!(" instance {instance}"))
        .unwrap_or_default();
    let allocation = match allocations[..] {
        [allocation] => allocation,
        [] => return Err(format!(
            "No address is allocated to {}{instance_text} in {}, \
            please connect it with 'flake-ctl firecracker network add' first",
            flake.path, defaults::NETWORK_STATE
        )),
        _ => return Err(format!(
            "More than one address is allocated to {}{instance_text} in {}, \
            please connect it again", flake.path, defaults::NETWORK_STATE
        ))
    };
    let address = allocation.address;
    match state.get_network() {
        Some(network) if is_vm_address(&network, &address) => { },
        _ => return Err(format!(
            "Guest address {address} allocated to {} is not a VM address \
            of the recorded network", flake.path
        ))
    }
    info!(
        "Guest address {address} of {} (user ID {})",
        allocation.tap, allocation.uid
    );
    Ok(address)
}

fn get_export_owner(exports: &str, path: &str) -> Option<u32> {
    /*!
    Provide the user the managed NFS export of the given path maps
    every access to, the anonuid of its clients
    !*/
    let managed_entries: Vec<&str> = exports.lines().filter(
        |line| is_flake_pilot_nfs_export(line, path)
    ).collect();
    get_nfs_export_clients(&managed_entries, path).iter().find_map(
        |client| client.options.split(',').find_map(
            |option| option.strip_prefix("anonuid=")?.parse().ok()
        )
    )
}

const FLAKE_PILOT_NFS_EXPORT_MARKER: &str =
    "# flake-pilot firecracker volume";

// VolumeOwner is the user every access of a VM to an exported
// volume is mapped to
struct VolumeOwner {
    uid: u32,
    gid: u32
}

fn get_volume_owner(caller: &Caller, path: &str) -> Option<VolumeOwner> {
    /*!
    Provide the owner of the given volume path

    The path is required to be owned by the caller. Called by
    root, a path owned by any user other than root is accepted.
    A path owned by root is refused, the VMs would get the
    permissions of root on it. The path has to be given the way
    the NFS server resolves it, a path which leads through a
    symbolic link is refused
    !*/
    let resolved_path = match fs::canonicalize(path) {
        Ok(resolved_path) => resolved_path,
        Err(error) => {
            error!("Failed to resolve volume path {path:?}: {error}");
            return None
        }
    };
    if resolved_path != Path::new(path) {
        error!("Volume path {path:?} leads through a symbolic link");
        error!("Please use the resolved path {}", resolved_path.display());
        return None
    }
    let metadata = match fs::metadata(&resolved_path) {
        Ok(metadata) => metadata,
        Err(error) => {
            error!("Failed to read volume path {path:?}: {error}");
            return None
        }
    };
    let owner = VolumeOwner { uid: metadata.uid(), gid: metadata.gid() };
    if owner.uid == 0 {
        error!("Volume path {path:?} is owned by root");
        error!("Only a path owned by a user other than root can be exported");
        return None
    }
    if ! caller.may_act_for(owner.uid) {
        error!("Volume path {path:?} is not owned by the calling user");
        return None
    }
    Some(owner)
}

fn nfs_export_options(owner: &VolumeOwner) -> String {
    /*!
    Provide the NFS export options for a volume of the given owner
    !*/
    format!(
        "{},anonuid={},anongid={}",
        defaults::NFS_EXPORT_OPTIONS, owner.uid, owner.gid
    )
}

fn validate_volume_export_path(path: &str, must_exist: bool) -> bool {
    /*!
    Validate the path used for an NFS volume export operation
    !*/
    if ! path.starts_with('/') {
        error!("Path {path:?} must be specified with an absolute path");
        return false
    }
    if path.chars().any(char::is_control) {
        error!("Path {path:?} contains unsupported control characters");
        return false
    }
    if must_exist {
        let volume_path = Path::new(path);
        if ! volume_path.exists() {
            error!("Volume path {path:?} does not exist");
            return false
        }
        if ! volume_path.is_dir() {
            error!("Volume path {path:?} is not a directory");
            return false
        }
    }
    true
}

// NfsExportUpdate is a change of the managed NFS export of a path
enum NfsExportUpdate {
    /// Allow the VM of the given address with the given options
    Add { guest: Ipv4Addr, options: String },
    /// Remove the VM of the given address
    RemoveGuest(Ipv4Addr),
    /// Delete the export
    RemoveAll
}

// NfsExportClient is a VM the managed NFS export of a path is
// allowed for
#[derive(Debug, PartialEq)]
struct NfsExportClient {
    guest: Ipv4Addr,
    options: String
}

fn update_nfs_exports(path: &str, update: NfsExportUpdate) -> bool {
    /*!
    Change the flake-pilot managed NFS export entry of the path
    !*/
    let exports = match read_nfs_exports() {
        Some(exports) => exports,
        None => return false
    };
    match get_updated_nfs_exports(&exports, path, update) {
        Some(updated_exports) => write_nfs_exports(&updated_exports),
        None => true
    }
}

fn get_updated_nfs_exports(
    exports: &str, path: &str, update: NfsExportUpdate
) -> Option<String> {
    /*!
    Provide the content of the exports file with the given change
    of the managed NFS export entry of the path applied. None is
    returned if the content stays the same

    The entry lists every VM it is allowed for as a client of its
    own. A client which is not the address of a single VM, as
    written by former versions for the whole private network, is
    dropped
    !*/
    let managed_entries: Vec<&str> = exports.lines().filter(
        |line| is_flake_pilot_nfs_export(line, path)
    ).collect();
    let mut clients = get_nfs_export_clients(&managed_entries, path);
    match update {
        NfsExportUpdate::Add { guest, options } => {
            clients.retain(|client| client.guest != guest);
            clients.push(NfsExportClient { guest, options: options.clone() });
            // All VMs access the volume as its owner, an owner
            // which has changed applies to all of them
            for client in clients.iter_mut() {
                client.options = options.clone()
            }
        },
        NfsExportUpdate::RemoveGuest(guest) => {
            if ! clients.iter().any(|client| client.guest == guest) {
                info!("{guest} is not a client of the NFS export for {path}");
            }
            clients.retain(|client| client.guest != guest)
        },
        NfsExportUpdate::RemoveAll => clients.clear()
    }
    let desired_entry = if clients.is_empty() {
        None
    } else {
        Some(nfs_export_entry(path, &clients))
    };
    match desired_entry {
        Some(ref desired_entry) if managed_entries.len() == 1
            && managed_entries[0].trim() == *desired_entry =>
        {
            info!("Keeping existing NFS export for {path}");
            return None
        },
        None if managed_entries.is_empty() => {
            info!("No flake-pilot NFS export entry found for {path}");
            return None
        },
        _ => { }
    }

    let mut updated_lines: Vec<String> = exports.lines().filter(
        |line| ! is_flake_pilot_nfs_export(line, path)
    ).map(ToOwned::to_owned).collect();
    match desired_entry {
        Some(desired_entry) => {
            info!("Exporting {path} through NFS...");
            updated_lines.push(desired_entry);
        },
        None => info!("Releasing NFS export for {path}...")
    }

    let mut updated_exports = updated_lines.join("\n");
    if ! updated_exports.is_empty() {
        updated_exports.push('\n');
    }
    Some(updated_exports)
}

fn get_nfs_export_clients(
    managed_entries: &[&str], path: &str
) -> Vec<NfsExportClient> {
    /*!
    Read the VMs the given managed NFS export entries allow
    !*/
    let mut clients: Vec<NfsExportClient> = Vec::new();
    for entry in managed_entries {
        let entry = entry.trim()
            .trim_end_matches(FLAKE_PILOT_NFS_EXPORT_MARKER);
        for client_spec in entry.split_whitespace().skip(1) {
            let (client, options) = client_spec.split_once('(')
                .unwrap_or((client_spec, ""));
            match client.parse::<Ipv4Addr>() {
                Ok(guest) if ! clients.iter().any(
                    |known| known.guest == guest
                ) => clients.push(NfsExportClient {
                    guest, options: options.trim_end_matches(')').to_string()
                }),
                Ok(_) => { },
                Err(_) => warn!(
                    "Dropping NFS export of {path} for {client}, \
                    only the address of a single VM is allowed"
                )
            }
        }
    }
    clients
}

fn read_nfs_exports() -> Option<String> {
    /*!
    Read the /etc/exports file
    !*/
    match fs::read_to_string(defaults::NFS_EXPORTS_FILE) {
        Ok(exports) => Some(exports),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Some(String::new()),
        Err(error) => {
            error!(
                "Failed to read {}: {error}",
                defaults::NFS_EXPORTS_FILE
            );
            None
        }
    }
}

fn write_nfs_exports(exports: &str) -> bool {
    /*!
    Write the /etc/exports file, see network::replace_file()
    !*/
    replace_file(defaults::NFS_EXPORTS_FILE, exports, 0o644)
}

fn nfs_export_entry(path: &str, clients: &[NfsExportClient]) -> String {
    /*!
    Provide the managed NFS export line for the given path
    !*/
    let clients: Vec<String> = clients.iter().map(
        |client| format!("{}({})", client.guest, client.options)
    ).collect();
    format!(
        "{} {} {}",
        escape_nfs_export_path(path),
        clients.join(" "),
        FLAKE_PILOT_NFS_EXPORT_MARKER
    )
}

fn escape_nfs_export_path(path: &str) -> String {
    /*!
    Escape whitespace in the path for /etc/exports
    !*/
    path.replace('\\', "\\\\")
        .replace(' ', "\\040")
        .replace('\t', "\\011")
}

fn is_flake_pilot_nfs_export(line: &str, path: &str) -> bool {
    /*!
    Check if the line is the managed flake-pilot export for path
    !*/
    let trimmed = line.trim();
    trimmed.starts_with(&format!("{} ", escape_nfs_export_path(path)))
        && trimmed.ends_with(FLAKE_PILOT_NFS_EXPORT_MARKER)
}

fn nfs_server_is_running() -> bool {
    /*!
    Check if the NFS server systemd service is active
    !*/
    let mut call = Command::new(defaults::SYSTEMCTL_TOOL);
    call.arg("is-active")
        .arg("--quiet")
        .arg(defaults::NFS_SERVER_SERVICE);
    match call.status() {
        Ok(status) => status.success(),
        Err(error) => {
            error!(
                "Failed to query {}: {error:?}",
                defaults::NFS_SERVER_SERVICE
            );
            false
        }
    }
}

fn start_nfs_server() -> bool {
    /*!
    Start the NFS server service
    !*/
    info!("Starting {}...", defaults::NFS_SERVER_SERVICE);
    let mut call = Command::new(defaults::SYSTEMCTL_TOOL);
    call.arg("start").arg(defaults::NFS_SERVER_SERVICE);
    run_ok(&mut call, &format!("start {}", defaults::NFS_SERVER_SERVICE))
}

fn restart_nfs_server() -> bool {
    /*!
    Restart the NFS server service
    !*/
    info!("Restarting {}...", defaults::NFS_SERVER_SERVICE);
    let mut call = Command::new(defaults::SYSTEMCTL_TOOL);
    call.arg("restart").arg(defaults::NFS_SERVER_SERVICE);
    run_ok(&mut call, &format!("restart {}", defaults::NFS_SERVER_SERVICE))
}

fn reload_nfs_exports() -> bool {
    /*!
    Reload the NFS exports of the running server
    !*/
    info!("Reloading NFS exports...");
    let mut call = Command::new(defaults::NFS_EXPORTFS_TOOL);
    call.arg("-ra");
    run_ok(&mut call, "reload NFS exports")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use uzers::get_current_uid;

    const OPTIONS: &str =
        "rw,sync,no_subtree_check,all_squash,anonuid=1000,anongid=100";

    fn add(guest: &str) -> NfsExportUpdate {
        NfsExportUpdate::Add {
            guest: guest.parse().unwrap(), options: OPTIONS.to_string()
        }
    }

    fn entry(path: &str, guests: &[&str]) -> String {
        let clients: Vec<String> = guests.iter().map(
            |guest| format!("{guest}({OPTIONS})")
        ).collect();
        format!(
            "{path} {} {FLAKE_PILOT_NFS_EXPORT_MARKER}", clients.join(" ")
        )
    }

    #[test]
    fn test_nfs_export_options() {
        assert_eq!(
            nfs_export_options(&VolumeOwner { uid: 1000, gid: 100 }),
            OPTIONS
        );
        assert!(! defaults::NFS_EXPORT_OPTIONS.contains("no_root_squash"));
        assert!(! defaults::NFS_EXPORT_OPTIONS.contains("insecure"));
    }

    #[test]
    fn test_export_is_created_per_guest() {
        let exports = "/other host(ro)\n";
        let updated = get_updated_nfs_exports(
            exports, "/srv/vol", add("172.16.0.2")
        ).unwrap();
        assert_eq!(
            updated,
            format!("/other host(ro)\n{}\n", entry("/srv/vol", &["172.16.0.2"]))
        );
        // A further VM is added to the same entry
        let updated = get_updated_nfs_exports(
            &updated, "/srv/vol", add("172.16.0.3")
        ).unwrap();
        assert_eq!(
            updated,
            format!(
                "/other host(ro)\n{}\n",
                entry("/srv/vol", &["172.16.0.2", "172.16.0.3"])
            )
        );
        // Exporting again for a known VM changes nothing
        assert_eq!(
            get_updated_nfs_exports(&updated, "/srv/vol", add("172.16.0.3")),
            None
        );
    }

    #[test]
    fn test_export_replaces_network_wide_entry() {
        let exports = format!(
            "/srv/vol 172.16.0.0/24(rw,sync,no_subtree_check,\
            no_root_squash,insecure) {FLAKE_PILOT_NFS_EXPORT_MARKER}\n\
            /srv/vol 10.0.0.1(ro)\n"
        );
        let updated = get_updated_nfs_exports(
            &exports, "/srv/vol", add("172.16.0.2")
        ).unwrap();
        // The export not managed by flake-pilot is kept as it is
        assert_eq!(
            updated,
            format!(
                "/srv/vol 10.0.0.1(ro)\n{}\n",
                entry("/srv/vol", &["172.16.0.2"])
            )
        );
        assert!(! updated.contains("no_root_squash"));
        assert!(! updated.contains("insecure"));
    }

    #[test]
    fn test_export_release() {
        let exports = format!(
            "{}\n", entry("/srv/vol", &["172.16.0.2", "172.16.0.3"])
        );
        let updated = get_updated_nfs_exports(
            &exports, "/srv/vol",
            NfsExportUpdate::RemoveGuest("172.16.0.2".parse().unwrap())
        ).unwrap();
        assert_eq!(
            updated, format!("{}\n", entry("/srv/vol", &["172.16.0.3"]))
        );
        // The entry is deleted with its last VM
        assert_eq!(
            get_updated_nfs_exports(
                &updated, "/srv/vol",
                NfsExportUpdate::RemoveGuest("172.16.0.3".parse().unwrap())
            ),
            Some(String::new())
        );
        assert_eq!(
            get_updated_nfs_exports(
                &exports, "/srv/vol", NfsExportUpdate::RemoveAll
            ),
            Some(String::new())
        );
        // Nothing to release
        assert_eq!(
            get_updated_nfs_exports(
                "/srv/vol 10.0.0.1(ro)\n", "/srv/vol",
                NfsExportUpdate::RemoveAll
            ),
            None
        );
    }

    #[test]
    fn test_get_volume_owner() {
        let caller = Caller::from_uid(get_current_uid()).unwrap();
        // A path owned by root is never exported
        assert!(get_volume_owner(&caller, "/").is_none());

        let tmp_dir = tempfile::tempdir().unwrap();
        let volume = tmp_dir.path().join("volume");
        fs::create_dir(&volume).unwrap();
        let link = tmp_dir.path().join("link");
        symlink(&volume, &link).unwrap();

        // A path leading through a symbolic link is refused
        assert!(get_volume_owner(&caller, link.to_str().unwrap()).is_none());

        // The path is owned by the calling user, which is only
        // accepted if that user is not root
        let resolved = fs::canonicalize(&volume).unwrap();
        assert_eq!(
            get_volume_owner(&caller, resolved.to_str().unwrap()).is_some(),
            get_current_uid() != 0
        );
    }

    #[test]
    fn test_get_export_owner() {
        let exports = format!(
            "/srv/other 10.0.0.1(ro)\n{}\n", entry("/srv/vol", &["172.16.0.2"])
        );
        assert_eq!(Some(1000), get_export_owner(&exports, "/srv/vol"));
        // an export which is not managed by flake-pilot has no owner
        assert_eq!(None, get_export_owner(&exports, "/srv/other"));
        assert_eq!(None, get_export_owner(&exports, "/srv/missing"));
    }

    #[test]
    fn test_export_owner_of_network_wide_entry() {
        // an entry of a former version has no owner, only root can
        // release it
        let exports = format!(
            "/srv/vol 172.16.0.0/24(rw,sync,no_subtree_check,\
            no_root_squash,insecure) {FLAKE_PILOT_NFS_EXPORT_MARKER}\n"
        );
        assert!(is_flake_pilot_nfs_export(exports.trim(), "/srv/vol"));
        assert_eq!(None, get_export_owner(&exports, "/srv/vol"));
    }

    #[test]
    fn test_release_by_another_user_is_refused() {
        let mut caller = Caller::from_uid(get_current_uid()).unwrap();
        caller.uid = 1001;
        assert!(! caller.may_act_for(
            get_export_owner(
                &format!("{}\n", entry("/srv/vol", &["172.16.0.2"])),
                "/srv/vol"
            ).unwrap()
        ));
    }
}
