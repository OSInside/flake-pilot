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
use std::path::Path;
use flakes::config::{get_firecracker_ids_dir, get_podman_ids_dir};
use flakes::defaults::{FLAKES_CONFIG_USER, FLAKES_DIR_USER};
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
    !*/
    if ! usermode {
        error!("Only the user specific setup can be created");
        error!("The system wide setup is provided with the package");
        error!("Please call 'flake-ctl init --user' as a normal user");
        return false
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
    true
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
    format!(
"generic:
  flakes_dir: {flakes_dir}
  podman_ids_dir: {podman_ids_dir}
  firecracker_ids_dir: {firecracker_ids_dir}
  podman_storage_conf: {storage_conf}
"
    )
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
    let runroot_dir = format!(
        "{}/{}", storage_dir, defaults::PODMAN_STORAGE_RUNROOT_NAME
    );
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

fn user_home() -> Option<String> {
    /*!
    Home directory of the user calling the program
    !*/
    get_user_by_uid(get_current_uid()).map(
        |user| user.home_dir().to_string_lossy().to_string()
    )
}
