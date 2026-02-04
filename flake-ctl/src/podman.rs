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
use std::process::Command;
use glob::glob;
use crate::defaults;
use crate::{app, app_config};
use flakes::config::get_flakes_dir;
use flakes::config::get_podman_storage_conf;
use flakes::config::read_storage_conf;
use uzers::{get_current_username};

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
    for app_name in app::app_names(usermode) {
        let config_file = format!(
            "{}/{}.yaml", get_flakes_dir(usermode), app_name
        );
        match app_config::AppConfig::init_from_file(Path::new(&config_file)) {
            Ok(app_conf) => {
                if let Some(ref container_conf) = app_conf.container {
                    if container == container_conf.name {
                        app::remove(
                            &container_conf.host_app_path,
                            defaults::PODMAN_PILOT,
                            usermode,
                            false,
                            false
                        );
                    }
                }
            },
            Err(error) => {
                error!(
                    "Ignoring error on load or parse flake config {config_file}: {error:?}"
                );
            }
        };
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
                println!("{}", &String::from_utf8_lossy(
                    data.as_bytes()
                ).to_string());
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
    call.arg("--preserve-env");
    if usermode {
        call.arg("--user").arg(calling_user_name);
    }
    call.arg(defaults::PODMAN_PATH);
    call
}
