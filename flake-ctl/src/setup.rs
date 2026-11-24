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
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use flakes::config::{
    get_bubblewrap_ids_dir, get_firecracker_ids_dir, get_podman_ids_dir
};
use flakes::defaults::{FLAKES_CONFIG_USER, FLAKES_DIR_USER, META_DIR_MODE};
use flakes::helper::helper_call;
use flakes::io::IO;
use flakes::user::{User, mkdir};
use uzers::{get_current_uid, get_user_by_uid};
use uzers::os::unix::UserExt;

pub fn init(usermode: bool, force: bool) -> bool {
    /*!
    Create the user specific setup to run flake applications

    The setup consists of the flakes directory of the calling
    user, the user specific flakes configuration file and the
    podman storage configuration the flakes of that user are
    stored in. Files which already exist are kept unless the
    creation is forced

    In addition the meta data directories shared by the pilots of
    all users are checked and created if needed, see
    init_meta_dirs(). For the root user this is the only action,
    the system wide setup is provided with the package
    !*/
    if ! usermode {
        if ! init_meta_dirs() {
            return false
        }
        info!("The system wide setup is provided with the package");
        info!("Call 'flake-ctl init' as a normal user to create the");
        info!("setup of that user");
        return true
    }
    let home = match user_home() {
        Some(home) => home,
        None => {
            error!("Failed to lookup the home directory of the caller");
            return false
        }
    };
    let flakes_dir = format!("{home}/{FLAKES_DIR_USER}");
    let podman_dir = format!(
        "{}/{}", flakes_dir, defaults::PODMAN_REGISTRY_NAME
    );
    let firecracker_dir = format!(
        "{}/{}", flakes_dir, defaults::FIRECRACKER_REGISTRY_NAME
    );
    for flake_dir in [&flakes_dir, &podman_dir, &firecracker_dir] {
        if let Err(error) = fs::create_dir_all(flake_dir) {
            error!("Failed to create {flake_dir}: {error:?}");
            return false
        }
    }
    let config_file = format!("{home}/{FLAKES_CONFIG_USER}");
    let storage_conf = format!(
        "{}/{}", podman_dir, defaults::PODMAN_STORAGE_CONF_NAME
    );
    if ! write_file(
        &config_file, &flakes_config(&flakes_dir, &storage_conf), force
    ) {
        return false
    }
    if ! write_file(
        &storage_conf, &podman_storage_config(&podman_dir), force
    ) {
        return false
    }
    info!("Flake registrations of this user are stored in {flakes_dir}");
    info!("To let podman commands operate on this flake storage run:");
    info!("export CONTAINERS_STORAGE_CONF={storage_conf}");
    init_meta_dirs()
}

fn init_meta_dirs() -> bool {
    /*!
    Make sure the meta data directories of the pilots are in place

    The directories are shared by the flakes of all users, every
    user keeps a private directory below them. The flake-pilot
    package lets systemd-tmpfiles create them at boot. A directory
    which does not exist, e.g on a host without systemd, is created
    here by root, or by the helper for a caller who is not root,
    see its meta-dirs-init command. The pilots do not create it and
    refuse to run without it. An existing directory has to be set
    up the way IO::meta_dir_problem() expects
    !*/
    let mut ids_dirs = vec![
        get_podman_ids_dir(false),
        get_firecracker_ids_dir(false),
        get_bubblewrap_ids_dir(false)
    ];
    ids_dirs.sort();
    ids_dirs.dedup();
    if get_current_uid() != 0 && ids_dirs.iter().any(
        |ids_dir| fs::symlink_metadata(ids_dir).is_err()
    ) {
        info!("Creating meta data directories through the helper");
        return match helper_call("meta-dirs-init").status() {
            Ok(status) => status.success(),
            Err(error) => {
                error!("Failed to run the helper: {error}");
                false
            }
        }
    }
    let mut result = true;
    for ids_dir in ids_dirs {
        if fs::symlink_metadata(&ids_dir).is_err() {
            info!("Creating meta data directory {ids_dir}");
            if let Err(error) = mkdir(
                &ids_dir, META_DIR_MODE, User::ROOT
            ) {
                error!("Failed to create {ids_dir}: {error}");
                result = false;
                continue
            }
        }
        if let Some(problem) = IO::meta_dir_problem(&ids_dir) {
            error!("{problem}");
            result = false
        }
        // Private directories which were created by another user than
        // the one they are named after keep that user from running
        // flakes. They are reported but not deleted, root decides.
        // Root gets all of them reported, a user its own one
        let problems = IO::squatted_private_dirs(&ids_dir).into_iter().filter(
            |problem| get_current_uid() == 0 || problem.starts_with(
                &format!("{ids_dir}/{} ", get_current_uid())
            )
        );
        for problem in problems {
            warn!("{problem}");
        }
    }
    result
}

fn flakes_config(flakes_dir: &str, storage_conf: &str) -> String {
    /*!
    Provide the contents of the user specific flakes config file

    The setup of the calling user uses its own flakes directory
    and its own podman storage. The meta data directories of the
    pilots are shared with the system wide setup, they store the
    instance information of all users in private directories
    !*/
    let podman_ids_dir = get_podman_ids_dir(false);
    let firecracker_ids_dir = get_firecracker_ids_dir(false);
    let bubblewrap_ids_dir = get_bubblewrap_ids_dir(false);
    format!(
"generic:
  flakes_dir: {flakes_dir}
  podman_ids_dir: {podman_ids_dir}
  firecracker_ids_dir: {firecracker_ids_dir}
  bubblewrap_ids_dir: {bubblewrap_ids_dir}
  podman_storage_conf: {storage_conf}
"
    )
}

fn podman_runroot(storage_dir: &str) -> String {
    /*!
    Provide the runroot of the podman storage of the calling user

    The runroot holds the runtime data of podman, the pilots use
    a sub directory of it as runtime directory of the podman calls.
    This includes the network namespaces created by podman for
    rootless networking. On a host with SELinux a directory below
    the home directory has a context that is not allowed to open
    namespaces due to selinux default rules. This includes for
    example the network namespace in. Therefore the runroot is
    placed in the runtime directory of the user, /run/user/UID,
    which is the location rootless podman uses as well.
    The runtime directory is provided by systemd-logind for
    the time the user is logged in. If there is none, e.g on a
    host without systemd-logind, the runroot is placed in the
    given storage directory as fallback
    !*/
    let uid = get_current_uid();
    let runtime_dir = format!("{}/{uid}", defaults::USER_RUNTIME_DIR_PREFIX);
    user_runroot(storage_dir, &runtime_dir, uid)
}

fn user_runroot(storage_dir: &str, runtime_dir: &str, uid: u32) -> String {
    /*!
    Provide the runroot below the given runtime directory if it
    is a directory owned by the given user, the runroot below the
    given storage directory otherwise
    !*/
    match fs::symlink_metadata(runtime_dir) {
        Ok(metadata) if metadata.is_dir() && metadata.uid() == uid => {
            format!("{runtime_dir}/{}", defaults::PODMAN_USER_RUNROOT_NAME)
        },
        _ => {
            warn!("No runtime directory {runtime_dir} of the user found");
            warn!("Using podman runroot below {storage_dir} instead");
            warn!("Rootless networking might fail on hosts using SELinux");
            format!("{storage_dir}/{}", defaults::PODMAN_STORAGE_RUNROOT_NAME)
        }
    }
}

fn podman_storage_config(podman_dir: &str) -> String {
    /*!
    Provide the contents of the podman storage config file
    for the flake containers of the calling user
    !*/
    let driver = defaults::PODMAN_STORAGE_DRIVER;
    let storage_dir = format!(
        "{}/{}", podman_dir, defaults::PODMAN_STORAGE_NAME
    );
    let runroot_dir = podman_runroot(&storage_dir);
    // rootless_storage_path is deprecated in favor of graphroot, but
    // containers-storage before its config rework (podman 4.x) ignores
    // graphroot for rootless users when CONTAINERS_STORAGE_CONF is set.
    // Newer versions only warn about it as both point to the same path
    format!(
r#"[storage]
driver = "{driver}"
graphroot = "{storage_dir}"
runroot = "{runroot_dir}"
rootless_storage_path = "{storage_dir}"
"#
    )
}

fn write_file(config_file: &str, data: &str, force: bool) -> bool {
    /*!
    Write the given configuration file

    An existing configuration file is expected to be adapted
    by the user and is therefore not touched unless the
    creation is forced
    !*/
    if Path::new(config_file).exists() && ! force {
        info!("Keeping existing {config_file}");
        info!("Use --force to create it from scratch");
        return true
    }
    match fs::write(config_file, data) {
        Ok(_) => {
            info!("Created {config_file}");
            true
        },
        Err(error) => {
            error!("Failed to write {config_file}: {error:?}");
            false
        }
    }
}

pub fn user_home() -> Option<String> {
    /*!
    Home directory of the user calling the program
    !*/
    get_user_by_uid(get_current_uid()).map(
        |user| user.home_dir().to_string_lossy().to_string()
    )
}

#[cfg(test)]
mod tests {
    use super::user_runroot;
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;
    use uzers::get_current_uid;

    #[test]
    fn test_user_runroot_in_runtime_dir() {
        let runtime_dir = tempdir().unwrap();
        let runtime_dir = runtime_dir.path().to_str().unwrap();
        assert_eq!(
            format!("{runtime_dir}/flakes"),
            user_runroot("/storage", runtime_dir, get_current_uid())
        );
    }

    #[test]
    fn test_user_runroot_fallback_no_runtime_dir() {
        let runtime_dir = tempdir().unwrap();
        let runtime_dir = format!(
            "{}/missing", runtime_dir.path().to_str().unwrap()
        );
        assert_eq!(
            "/storage/runroot",
            user_runroot("/storage", &runtime_dir, get_current_uid())
        );
    }

    #[test]
    fn test_user_runroot_fallback_runtime_dir_of_other_user() {
        let runtime_dir = tempdir().unwrap();
        let runtime_dir = runtime_dir.path().to_str().unwrap();
        assert_eq!(
            "/storage/runroot",
            user_runroot("/storage", runtime_dir, get_current_uid() + 1)
        );
    }

    #[test]
    fn test_user_runroot_fallback_runtime_dir_symlink() {
        let tmp = tempdir().unwrap();
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let runtime_dir = tmp.path().join("link");
        symlink(&target, &runtime_dir).unwrap();
        assert_eq!(
            "/storage/runroot",
            user_runroot(
                "/storage", runtime_dir.to_str().unwrap(), get_current_uid()
            )
        );
    }
}
