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
use crate::cli::ListFormat;
use crate::{app_config, defaults, firecracker, output, podman};
use glob::glob;
use serde::Serialize;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use flakes::config::get_flakes_dir;
use uzers::{get_current_username};

pub fn register(
    app: Option<&String>, target: Option<&String>,
    engine: &str, usermode: bool,
) -> bool {
    /*!
    Register container application for specified engine.

    Create an app symlink pointing to the engine launcher.
    !*/
    if app.is_none() {
        error!("No application specified");
        return false;
    }
    let host_app_path = app.unwrap();
    let target_app_path = target.unwrap_or(host_app_path);
    for path in &[host_app_path, target_app_path] {
        if !path.starts_with('/') {
            error!(
                "Application {path:?} must be specified with an absolute path"
            );
            return false;
        }
    }
    info!("Registering application: {host_app_path}");

    // host_app_path -> pointing to engine
    let host_app_dir = Path::new(host_app_path)
        .parent().unwrap().to_str().unwrap();
    match fs::create_dir_all(host_app_dir) {
        Ok(dir) => dir,
        Err(error) => {
            error!("Failed creating: {}: {:?}", host_app_dir, error);
            return false;
        }
    };
    match symlink(engine, host_app_path) {
        Ok(link) => link,
        Err(error) => {
            error!(
                "Error while creating symlink \"{} -> {}\": {:?}",
                host_app_path, engine, error
            );
            return false;
        }
    }

    // creating default app configuration
    let app_basename = Path::new(app.unwrap())
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    let app_config_dir = format!(
        "{}/{}.d", get_flakes_dir(usermode), app_basename
    );
    match fs::create_dir_all(&app_config_dir) {
        Ok(dir) => dir,
        Err(error) => {
            error!("Failed creating: {}: {:?}", app_config_dir, error);
            return false;
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
pub fn create_container_config(
    container: &str,
    app: Option<&String>,
    target: Option<&String>,
    base: Option<&String>,
    check_host_dependencies: bool,
    layers: Option<Vec<String>>,
    includes_tar: Option<Vec<String>>,
    includes_path: Option<Vec<String>>,
    resume: bool,
    attach: bool,
    usermode: bool,
    opts: Option<Vec<String>>,
) -> bool {
    /*!
    Create app configuration for the container engine.

    Create an app configuration file as get_flakes_dir()/app.yaml
    containing the required information to launch the
    application inside of the container engine.
    !*/
    let mut current_user = String::new();
    current_user.push_str(
        get_current_username().unwrap().to_str().unwrap()
    );
    if base.is_none() && layers.is_some() {
        error!("Layer(s) specified without a base");
        return false;
    }
    let host_app_path = app.unwrap();

    let target_app_path = target.unwrap_or(host_app_path);

    let app_basename = Path::new(app.unwrap())
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    let app_config_file = format!(
        "{}/{}.yaml", get_flakes_dir(usermode), app_basename
    );
    match app_config::AppConfig::save_container(
        Path::new(&app_config_file),
        container,
        target_app_path,
        host_app_path,
        base,
        check_host_dependencies,
        layers,
        includes_tar,
        includes_path,
        resume,
        attach,
        Some(&current_user),
        opts,
    ) {
        Ok(_) => true,
        Err(error) => {
            error!(
                "Failed to create AppConfig {app_config_file}: {error:?}"
            );
            false
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn create_vm_config(
    vm: &String,
    app: Option<&String>,
    target: Option<&String>,
    run_as: Option<&String>,
    overlay_size: Option<&String>,
    no_net: bool,
    resume: bool,
    force_vsock: bool,
    includes_tar: Option<Vec<String>>,
    includes_path: Option<Vec<String>>,
    usermode: bool,
) -> bool {
    /*!
    Create app configuration for the firecracker engine.

    Create an app configuration file as get_flakes_dir()/app.yaml
    containing the required information to launch the
    application inside of the firecracker engine.
    !*/
    let host_app_path = app.unwrap();
    let target_app_path = target.unwrap_or(host_app_path);
    let app_basename = Path::new(host_app_path)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    let app_config_file = format!(
        "{}/{}.yaml", get_flakes_dir(usermode), app_basename
    );
    match app_config::AppConfig::save_vm(
        Path::new(&app_config_file),
        vm,
        target_app_path,
        host_app_path,
        run_as,
        overlay_size,
        no_net,
        resume,
        force_vsock,
        includes_tar,
        includes_path,
        usermode,
    ) {
        Ok(_) => true,
        Err(error) => {
            error!(
                "Failed to create AppConfig {app_config_file}: {error:?}"
            );
            false
        }
    }
}

pub fn remove(
    app: &str, engine: &str, usermode: bool, silent: bool, force: bool
) -> bool {
    /*!
    Delete application link and config files
    !*/
    if !app.starts_with('/') {
        if !silent {
            error!(
                "Application {app:?} must be specified with an absolute path"
            );
        };
        return false
    }
    if !silent {
        info!("Removing application: {app}");
    }

    // sanity checks
    let app_basename = basename(&app.to_string());
    let config_file = format!(
        "{}/{}.yaml", get_flakes_dir(usermode), app_basename
    );
    let app_config_dir = format!(
        "{}/{}.d", get_flakes_dir(usermode), app_basename
    );
    let config_file_exists = Path::new(&config_file).exists();
    let app_config_dir_exists = Path::new(&app_config_dir).exists();
    let app_exists = Path::new(&app).exists();
    if ! force {
        if ! config_file_exists {
            if !silent {
                error!(
                    "No app config file found: {config_file}, consider --force"
                );
            }
            return false
        }
        if ! app_config_dir_exists {
            if !silent {
                error!(
                    "No app directory found: {app_config_dir}, consider --force"
                );
            }
            return false
        }
    }

    if force {
        if app_exists {
            match fs::remove_file(app) {
                Ok(_) => {}
                Err(error) => {
                    if !silent {
                        error!("Error removing: {app}: {error:?}");
                    };
                    return false
                }
            }
        }
    } else {
        // remove pilot link if valid
        match fs::read_link(app) {
            Ok(link_name) => {
                if link_name.into_os_string() == engine {
                    match fs::remove_file(app) {
                        Ok(_) => {}
                        Err(error) => {
                            if !silent {
                                error!(
                                    "Error removing pilot link: {app}: {error:?}"
                                );
                            };
                            return false
                        }
                    }
                } else {
                    if !silent {
                        error!("Symlink not pointing to {engine}: {app}");
                    };
                    return false
                }
            }
            Err(error) => {
                if !silent {
                    error!("Failed to read as symlink: {app}: {error:?}");
                };
                return false
            }
        }
    }
    // remove config file and config directory
    if config_file_exists {
        match fs::remove_file(&config_file) {
            Ok(_) => {}
            Err(error) => {
                if !silent {
                    error!(
                        "Error removing config file: {config_file}: {error:?}"
                    )
                };
                return false
            }
        }
    }
    if app_config_dir_exists {
        match fs::remove_dir_all(&app_config_dir) {
            Ok(_) => {}
            Err(error) => {
                if !silent {
                    error!(
                        "Error removing config directory: {app_config_dir}: {error:?}"
                    );
                    return false
                }
            }
        }
    }
    true
}

pub fn basename(program_path: &String) -> String {
    /*!
    Get basename from given program path
    !*/
    let mut program_name = String::new();
    program_name.push_str(
        Path::new(program_path)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap(),
    );
    program_name
}

pub fn app_names(usermode: bool) -> Vec<String> {
    /*!
    Read all flake config files
    !*/
    let mut flakes: Vec<String> = Vec::new();
    let glob_pattern = format!("{}/*.yaml", get_flakes_dir(usermode));
    for config_file in glob(&glob_pattern).unwrap() {
        match config_file {
            Ok(filepath) => {
                let base_config_file = basename(
                    &filepath.into_os_string().into_string().unwrap()
                );
                match base_config_file.split('.').next() {
                    Some(value) => {
                        let mut app_name = String::new();
                        app_name.push_str(value);
                        flakes.push(app_name);
                    }
                    None => error!(
                        "Ignoring invalid config_file: {base_config_file}"
                    ),
                }
            }
            Err(error) => error!(
                "Error while traversing flakes folder: {error:?}"
            ),
        }
    }
    flakes
}

pub fn app_details(app: &str, usermode: bool) -> app_config::AppConfig {
    /*!
    Read app config for given app base name
    !*/
    let config_file = format!("{}/{}.yaml", get_flakes_dir(usermode), app);
    match app_config::AppConfig::init_from_file(Path::new(&config_file)) {
        Ok(app_conf) => {
            app_conf
        },
        Err(error) => {
            panic!(
                "Failed reading app config file: {}: {:?}",
                config_file, error
            );
        }
    }
}

// FlakeInfo represents one registered flake application
// as it is presented by the list command
#[derive(Debug, Serialize)]
pub struct FlakeInfo {
    pub name: String,
    pub engine: Option<String>,
    pub target: Option<String>,
    pub host_app_path: Option<String>,
    pub config: String,
}

pub fn app_list(usermode: bool) -> Vec<FlakeInfo> {
    /*!
    Read the details of all registered flakes
    !*/
    let mut flakes: Vec<FlakeInfo> = Vec::new();
    let mut app_names = app_names(usermode);
    app_names.sort();
    for app in app_names {
        let config = format!("{}/{}.yaml", get_flakes_dir(usermode), app);
        let details = app_details(&app, usermode);
        let mut flake = FlakeInfo {
            name: app, engine: None, target: None,
            host_app_path: None, config
        };
        if let Some(ref container_conf) = details.container {
            flake.engine = Some(defaults::PODMAN_ENGINE.to_string());
            flake.target = Some(container_conf.target_app_path.to_string());
            flake.host_app_path = Some(
                container_conf.host_app_path.to_string()
            );
        } else if let Some(ref vm_conf) = details.vm {
            flake.engine = Some(defaults::FIRECRACKER_ENGINE.to_string());
            flake.target = Some(vm_conf.target_app_path.to_string());
            flake.host_app_path = Some(
                vm_conf.host_app_path.to_string()
            );
        }
        flakes.push(flake);
    }
    flakes
}

pub fn list(usermode: bool, format: ListFormat) {
    /*!
    Print all registered flakes in the requested output format
    !*/
    let flakes = app_list(usermode);
    match format {
        ListFormat::Table => list_as_table(&flakes, usermode),
        ListFormat::Json => output::print_json(&flakes),
        ListFormat::Csv => list_as_csv(&flakes),
    }
}

fn list_as_table(flakes: &[FlakeInfo], usermode: bool) {
    /*!
    Print flakes as human readable table with a headline
    !*/
    println!(
        "Flake applications registered in {}", get_flakes_dir(usermode)
    );
    println!();
    if flakes.is_empty() {
        println!("No application(s) registered");
        return;
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    for flake in flakes {
        rows.push(vec![
            flake.name.to_string(),
            output::column_value(flake.engine.as_ref()),
            output::column_value(flake.target.as_ref()),
            output::column_value(flake.host_app_path.as_ref()),
            flake.config.to_string(),
        ]);
    }
    output::print_table(&defaults::FLAKE_LIST_COLUMNS, &rows);
}

fn list_as_csv(flakes: &[FlakeInfo]) {
    /*!
    Print flakes as comma separated values, machine readable.
    Values which could not be read from the flake config are
    printed as empty fields
    !*/
    let mut rows: Vec<Vec<String>> = Vec::new();
    for flake in flakes {
        rows.push(vec![
            flake.name.to_string(),
            flake.engine.as_deref().unwrap_or_default().to_string(),
            flake.target.as_deref().unwrap_or_default().to_string(),
            flake.host_app_path.as_deref().unwrap_or_default().to_string(),
            flake.config.to_string(),
        ]);
    }
    output::print_csv(&rows);
}

pub fn purge(app: &str, engine: &str, usermode: bool) {
    /*!
    Iterate over all yaml config files and delete all app
    registrations and its connected resources for the specified app
    !*/
    if engine == defaults::PODMAN_PILOT {
        podman::purge_container(app, usermode)
    }
    if engine == defaults::FIRECRACKER_PILOT {
        firecracker::purge_vm(app, usermode)
    }
}

pub fn init(app: Option<&String>, usermode: bool) -> bool {
    /*!
    Create required directory structure.

    Symlink references to apps will be stored in get_flakes_dir()
    The init method makes sure to create this directory unless it
    already exists.
    !*/
    let mut status = true;
    if let Some(path) = app {
        if Path::new(&app.unwrap()).exists() {
            error!("App path {path} already exists");
            return false;
        }
    }
    let mut flake_dir = String::new();
    match fs::read_link(get_flakes_dir(usermode)) {
        Ok(target) => {
            flake_dir.push_str(&target.into_os_string().into_string().unwrap());
        }
        Err(_) => {
            flake_dir.push_str(&get_flakes_dir(usermode));
        }
    }
    fs::create_dir_all(flake_dir).unwrap_or_else(|why| {
        error!(
            "Failed creating {}: {:?}", get_flakes_dir(usermode), why.kind()
        );
        status = false
    });
    status
}
