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
use std::env;
use serde::Deserialize;
use std::path::Path;
use lazy_static::lazy_static;
use uzers::{get_current_username};

use crate::defaults;

lazy_static! {
    static ref FLAKES_CONFIG: FlakesConfig = read_flakes_config();
}

pub fn get_flakes_dir() -> String {
    let GenericData { flakes_dir, .. } = &flakes_config().generic;
    flakes_dir.clone().unwrap_or(defaults::FLAKES_DIR.to_string())
}

pub fn get_podman_ids_dir() -> String {
    let GenericData { podman_ids_dir, .. } = &flakes_config().generic;
    podman_ids_dir.clone().unwrap_or(defaults::PODMAN_IDS_DIR.to_string())
}

pub fn get_firecracker_ids_dir() -> String {
    let GenericData { firecracker_ids_dir, .. } = &flakes_config().generic;
    firecracker_ids_dir.clone().unwrap_or(
        defaults::FIRECRACKER_IDS_DIR.to_string()
    )
}

fn flakes_config() -> &'static FlakesConfig {
    &FLAKES_CONFIG
}

fn read_flakes_config() -> FlakesConfig {
    /*!
    Read systemwide flakes configuration file

    generic:
        flakes_dir: ~
        podman_ids_dir: ~
        firecracker_ids_dir: ~
    !*/
    let current_user = get_current_username().unwrap();
    let flake_config_path;
    if current_user != "root" {
        flake_config_path = format!(
            "{}/.config/flakes.yml", env::var("HOME").unwrap()
        );
    } else {
        flake_config_path = format!("{}", defaults::FLAKES_CONFIG);
    }
    if Path::new(&flake_config_path).exists() {
        info!("Using flakes config file: {}", flake_config_path);
        let flakes_file = std::fs::File::open(&flake_config_path)
            .unwrap_or_else(|_| panic!("Failed to open {}", flake_config_path));
        serde_yaml::from_reader(flakes_file)
            .unwrap_or_else(
                |error| panic!(
                    "Failed to import {}: {}", flake_config_path, error
                    )
                )
    } else {
        info!("Using compiled in flakes config");
        FlakesConfig {
            generic: GenericData {
                flakes_dir: None::<String>,
                podman_ids_dir: None::<String>,
                firecracker_ids_dir: None::<String>
            }
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
    firecracker_ids_dir: Option<String>
}
