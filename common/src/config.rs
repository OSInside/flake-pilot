//
// Copyright (c) 2023 SUSE Software Solutions Germany GmbH
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
use serde::Deserialize;

use std::path::Path;
use std::collections::HashMap;
use std::process::exit;

use lazy_static::lazy_static;

use uzers::{get_current_username};

use ini::Ini;

use crate::defaults;
use crate::error::FlakeError;
use crate::lookup::{Lookup};

lazy_static! {
    static ref FLAKES_CONFIG_SYSTEM: FlakesConfig = read_flakes_config_system();
}

lazy_static! {
    static ref FLAKES_CONFIG_USER: FlakesConfig = read_flakes_config_user();
}

pub fn get_flakes_dir(user: bool) -> String {
    let GenericData { flakes_dir, .. } = &flakes_config(user).generic;
    if flakes_dir.is_none() {
        if ! user {
            defaults::FLAKES_DIR.to_string()
        } else {
            error!("No flakes_dir configured");
            error!("Please check {}", get_user_flakes_config());
            error!("More details on rootless mode in 'man flake-pilot'");
            exit(1);
        }
    } else {
        flakes_dir.clone().unwrap()
    }
}

pub fn get_podman_storage_conf(user: bool) -> String {
    let GenericData { podman_storage_conf, .. } = &flakes_config(user).generic;
    if podman_storage_conf.is_none() {
        if ! user {
            defaults::PODMAN_STORAGE_CONF.to_string()
        } else {
            error!("No podman_storage_conf configured");
            error!("Please check {}", get_user_flakes_config());
            exit(1);
        }
    } else {
        podman_storage_conf.clone().unwrap()
    }
}

pub fn get_podman_ids_dir(user: bool) -> String {
    let GenericData { podman_ids_dir, .. } = &flakes_config(user).generic;
    if podman_ids_dir.is_none() {
        if ! user {
            defaults::PODMAN_IDS_DIR.to_string()
        } else {
            error!("No podman_ids_dir configured");
            error!("Please check {}", get_user_flakes_config());
            exit(1);
        }
    } else {
        podman_ids_dir.clone().unwrap()
    }
}

pub fn get_firecracker_ids_dir(user: bool) -> String {
    let GenericData { firecracker_ids_dir, .. } = &flakes_config(user).generic;
    if firecracker_ids_dir.is_none() {
        if ! user {
            defaults::FIRECRACKER_IDS_DIR.to_string()
        } else {
            error!("No firecracker_ids_dir configured");
            error!("Please check {}", get_user_flakes_config());
            exit(1);
        }
    } else {
        firecracker_ids_dir.clone().unwrap()
    }
}

fn flakes_config(user: bool) -> &'static FlakesConfig {
    if user {
        &FLAKES_CONFIG_USER
    } else {
        &FLAKES_CONFIG_SYSTEM
    }
}

fn get_user_flakes_config() -> String {
    let current_user = get_current_username().unwrap();
    let flake_config_path = format!(
        "/home/{}/.config/flakes.yml", current_user.to_str().unwrap()
    );
    flake_config_path
}

fn read_flakes_config_user() -> FlakesConfig {
    /*!
    Read user specific flakes configuration file
    !*/
    read_flakes_config(&get_user_flakes_config())
}

fn read_flakes_config_system() -> FlakesConfig {
    /*!
    Read systemwide flakes configuration file
    !*/
    read_flakes_config(defaults::FLAKES_CONFIG)
}

fn read_flakes_config(flake_config_path: &str) -> FlakesConfig {
    /*!
    Read flakes configuration file

    generic:
        flakes_dir: ~
        podman_ids_dir: ~
        firecracker_ids_dir: ~
        podman_storage_conf: ~
    !*/
    if Path::new(&flake_config_path).exists() {
        if Lookup::is_debug() {
            debug!("Reading flakes config file: {flake_config_path}");
        }
        let flakes_file = std::fs::File::open(flake_config_path)
            .unwrap_or_else(|_| panic!("Failed to open {flake_config_path}"));
        serde_yaml::from_reader(flakes_file)
            .unwrap_or_else(
                |error| panic!(
                    "Failed to import {flake_config_path}: {error}"
                    )
                )
    } else {
        FlakesConfig {
            generic: GenericData {
                flakes_dir: None::<String>,
                podman_ids_dir: None::<String>,
                firecracker_ids_dir: None::<String>,
                podman_storage_conf: None::<String>
            }
        }
    }
}

pub fn read_storage_conf(
    usermode: bool
) -> Result<HashMap<&'static str, String>, FlakeError> {
    /*!
    Read configured podman storage conf
    !*/
    let podman_storage_conf = get_podman_storage_conf(usermode);
    let mut result = HashMap::new();
    match Ini::load_from_file(&podman_storage_conf) {
        Ok(storage) => {
            let section = storage.section(Some("storage")).unwrap();
            result.insert(
                "graphroot", section.get("graphroot").unwrap_or("").to_string()
            );
            result.insert(
                "runroot", section.get("runroot").unwrap_or("").to_string()
            );
            Ok(result)
        },
        Err(error) => {
            Err(FlakeError::IOError {
                kind: "Reading INI file failed".to_string(),
                message: format!(
                    "Ini::load_from_file failed for {podman_storage_conf}: {error}"
                )
            })
        }
    }
}

#[derive(Deserialize)]
struct FlakesConfig {
    generic: GenericData,
}

#[derive(Deserialize)]
struct GenericData {
    /// Flakes directory to store registrations
    flakes_dir: Option<String>,

    /// ID files directory for podman registrations
    podman_ids_dir: Option<String>,

    /// ID files directory for firecracker registrations
    firecracker_ids_dir: Option<String>,

    /// Container storage conf for podman pilot
    podman_storage_conf: Option<String>
}
