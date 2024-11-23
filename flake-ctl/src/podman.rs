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
use crate::defaults;
use crate::{app, app_config};
use flakes::container::Container;
use flakes::config::get_flakes_dir;
use users::{get_current_username};

pub fn pull(uri: &String) -> i32 {
    /*!
    Call podman pull and prune with the provided uri
    !*/
    info!("Fetching from registry...");
    info!("podman pull {}", uri);

    let mut call = setup_podman_call("any");
    call.arg("pull")
        .arg(uri);
    let status = match call.status() {
        Ok(status) => {
            if status.success() {
                status
            } else {
                let _ = Container::podman_setup_permissions();
                call.status().unwrap()
            }
        }
        Err(_) => {
            let _ = Container::podman_setup_permissions();
            call.status().unwrap()
        }
    };
    let status_code = status.code().unwrap();
    if ! status.success() {
        error!("Failed, error message(s) reported");
    } else {
        info!("podman prune");
        let mut prune = setup_podman_call("any");
        let _ = prune.arg("image")
            .arg("prune")
            .arg("--force")
            .status();
    }
    status_code
}

pub fn load(oci: &String) -> i32 {
    /*!
    Call podman load with the provided oci tar file
    !*/
    info!("Loading OCI image...");
    info!("podman load -i {}", oci);

    let mut call = setup_podman_call("any");
    call.arg("load")
        .arg("-i")
        .arg(oci);
    let status = match call.status() {
        Ok(status) => {
            if status.success() {
                status
            } else {
                let _ = Container::podman_setup_permissions();
                call.status().unwrap()
            }
        }
        Err(_) => {
            let _ = Container::podman_setup_permissions();
            call.status().unwrap()
        }
    };

    let status_code = status.code().unwrap();
    if ! status.success() {
        error!("Failed, error message(s) reported");
    }
    status_code
}

pub fn rm(container: &String) {
    /*!
    Call podman image rm with force option to remove all running containers
    !*/
    info!("Removing image and all running containers...");
    info!("podman rm -f {}", container);

    let mut call = setup_podman_call("any");
    call.arg("image")
        .arg("rm")
        .arg("-f")
        .arg(container);
    let status = match call.status() {
        Ok(status) => {
            if ! status.success() {
                status
            } else {
                let _ = Container::podman_setup_permissions();
                call.status().unwrap()
            }
        }
        Err(_) => {
            let _ = Container::podman_setup_permissions();
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
    let mut call = setup_podman_call("root");
    call.arg("image")
        .arg("mount")
        .arg(container_name);
    let output = match call.output() {
        Ok(output) => {
            output
        }
        Err(_) => {
            let _ = Container::podman_setup_permissions();
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
    let mut call = setup_podman_call("root");
    call.arg("image")
        .arg("umount")
        .arg(container_name);
    let output = match call.output() {
        Ok(output) => {
            output
        }
        Err(_) => {
            let _ = Container::podman_setup_permissions();
            call.output().unwrap()
        }
    };
    output.status.code().unwrap()
}

pub fn purge_container(container: &str) {
    /*!
    Iterate over all yaml config files and find those connected
    to the container. Delete all app registrations for this
    container and also delete the container from the local
    registry
    !*/
    for app_name in app::app_names() {
        let config_file = format!(
            "{}/{}.yaml", get_flakes_dir(), app_name
        );
        match app_config::AppConfig::init_from_file(Path::new(&config_file)) {
            Ok(mut app_conf) => {
                if app_conf.container.is_some() &&
                    container == app_conf.container.as_mut().unwrap().name
                {
                    app::remove(
                        &app_conf.container.as_mut().unwrap().host_app_path,
                        defaults::PODMAN_PILOT, false
                    );
                }
            },
            Err(error) => {
                error!(
                    "Ignoring error on load or parse flake config {}: {:?}",
                    config_file, error
                );
            }
        };
    }
    rm(&container.to_string());
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
        "{}/{}.yaml", image_mount_point, container_basename
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
                error!("Error reading {}: {:?}", info_file, error);
            }
        }
    } else {
        error!("No info file {}.yaml found in container: {}",
            container_basename, container
        );
    }
    umount_container(container);
}

pub fn setup_podman_call(user: &str) -> Command {
    let mut current_user = String::new();
    if user == "any" {
        let username = get_current_username().unwrap();
        current_user.push_str(username.to_str().unwrap())
    } else {
        current_user.push_str(user)
    }
    let container_runroot = format!(
        "{}/{}", defaults::FLAKES_REGISTRY_RUNROOT, current_user
    );
    env::set_var("CONTAINERS_STORAGE_CONF", defaults::FLAKES_STORAGE);
    env::set_var("XDG_RUNTIME_DIR", &container_runroot);
    let mut call = Command::new("sudo");
    call.arg("--preserve-env")
        .arg(defaults::PODMAN_PATH);
    call
}
