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
use crate::{
    app_config, defaults, firecracker, instance, network, output, podman
};
use serde::Serialize;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use flakes::config::get_flakes_dir;
use flakes::registration;
use flakes::registration::basename;
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
    let app_basename = basename(host_app_path);
    let app_config_dir = registration::config_dir(&app_basename, usermode);
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
    pilot_options: Option<Vec<String>>,
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

    let app_config_file = registration::config_file(
        &basename(host_app_path), usermode
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
        pilot_options,
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
    resume: bool,
    force_vsock: bool,
    includes_tar: Option<Vec<String>>,
    includes_path: Option<Vec<String>>,
    pilot_options: Option<Vec<String>>,
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
    let app_config_file = registration::config_file(
        &basename(host_app_path), usermode
    );
    match app_config::AppConfig::save_vm(
        Path::new(&app_config_file),
        vm,
        target_app_path,
        host_app_path,
        run_as,
        overlay_size,
        resume,
        force_vsock,
        includes_tar,
        includes_path,
        pilot_options,
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

pub fn create_sandbox_config(
    rootfs: &str,
    app: Option<&String>,
    target: Option<&String>,
    run_as: Option<&String>,
    opts: Option<Vec<String>>,
    pilot_options: Option<Vec<String>>,
    usermode: bool,
) -> bool {
    /*!
    Create app configuration for the bubblewrap engine.

    Create an app configuration file as get_flakes_dir()/app.yaml
    containing the required information to launch the
    application inside of a bubblewrap sandbox.
    !*/
    if ! rootfs.starts_with('/') {
        error!("Rootfs {rootfs:?} must be specified with an absolute path");
        return false;
    }
    if ! Path::new(rootfs).is_dir() {
        // The rootfs is expected to exist at call time of the
        // application. Registering it ahead of its creation is
        // allowed but most probably a typo
        warn!("Rootfs {rootfs} does not exist or is not a directory");
    }
    let host_app_path = app.unwrap();
    let target_app_path = target.unwrap_or(host_app_path);
    let app_config_file = registration::config_file(
        &basename(host_app_path), usermode
    );
    match app_config::AppConfig::save_sandbox(
        Path::new(&app_config_file),
        rootfs,
        target_app_path,
        host_app_path,
        run_as,
        opts,
        pilot_options,
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
    let app_basename = basename(app);
    let config_file = registration::config_file(&app_basename, usermode);
    let app_config_dir = registration::config_dir(&app_basename, usermode);
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

// FlakeRegistration is a registered flake application as it
// is addressed by the remove command
pub struct FlakeRegistration {
    /// Name of the flake, the basename of the application on
    /// the host and the name the instances of the flake are
    /// named after
    pub name: String,
    /// Path of the application on the host
    pub host_app_path: String
}

pub fn image_flakes(
    image: &str, engine: &str, usermode: bool
) -> Vec<FlakeRegistration> {
    /*!
    Provide the flake applications which are registered with the
    given container or VM image
    !*/
    let mut registrations: Vec<FlakeRegistration> = Vec::new();
    for app_name in app_names(usermode) {
        let config_file = registration::config_file(&app_name, usermode);
        let app_conf = match app_config::AppConfig::init_from_file(
            Path::new(&config_file)
        ) {
            Ok(app_conf) => app_conf,
            Err(error) => {
                error!(
                    "Ignoring error on load or parse flake config {config_file}: {error:?}"
                );
                continue
            }
        };
        let host_app_path = if engine == defaults::PODMAN_ENGINE {
            app_conf.container
                .filter(|container_conf| container_conf.name == image)
                .map(|container_conf| container_conf.host_app_path)
        } else {
            app_conf.vm
                .filter(|vm_conf| vm_conf.name == image)
                .map(|vm_conf| vm_conf.host_app_path)
        };
        if let Some(host_app_path) = host_app_path {
            registrations.push(
                FlakeRegistration {
                    name: basename(&host_app_path), host_app_path
                }
            )
        }
    }
    registrations
}

pub fn remove_allowed(
    app: Option<&String>, image: Option<&String>, engine: &str,
    usermode: bool
) -> bool {
    /*!
    Check if the registration(s) addressed by a remove call
    may be deleted

    A registration which is still in use has to be kept. This is
    the case if one of its instances is still running, the
    instance would stay behind without the configuration it was
    created from. For a VM registration the same applies to the
    TAP devices of the host network setup. They belong to the
    network configuration of the flake and have to be deleted
    with 'flake-ctl firecracker network remove' first
    !*/
    let registrations = match (app, image) {
        (Some(app), _) => vec![
            FlakeRegistration {
                name: basename(app), host_app_path: app.to_string()
            }
        ],
        (None, Some(image)) => image_flakes(image, engine, usermode),
        // Nothing is addressed, there is nothing to protect
        (None, None) => return true
    };
    let mut allowed = true;

    // instances which are still running
    let flakes: Vec<String> = registrations.iter()
        .map(|registration| registration.name.to_string()).collect();
    let running = instance::running_instances(engine, &flakes, usermode);
    if ! running.is_empty() {
        error!("The following instance(s) are still running:");
        for running_instance in &running {
            error!(
                "  {} of user {}", running_instance.name, running_instance.user
            );
        }
        error!("Please stop them before removing the registration");
        allowed = false
    }

    // TAP devices of the host network setup
    if engine == defaults::FIRECRACKER_ENGINE {
        for registration in &registrations {
            let active_taps = network::get_active_taps(
                &registration.host_app_path, usermode
            );
            if active_taps.is_empty() {
                continue
            }
            error!(
                "TAP device(s) of {} are still active:",
                registration.host_app_path
            );
            for active_tap in &active_taps {
                error!(
                    "  {}, delete it with '{}'",
                    active_tap.name,
                    network::get_remove_command(
                        &registration.host_app_path,
                        active_tap.instance.as_deref()
                    )
                );
            }
            allowed = false
        }
    }
    allowed
}

pub fn app_names(usermode: bool) -> Vec<String> {
    /*!
    Read all flake config files
    !*/
    let mut flakes: Vec<String> = Vec::new();
    for config_file in registration::config_files(usermode) {
        let base_config_file = basename(&config_file.to_string_lossy());
        match base_config_file.split('.').next() {
            Some(value) => flakes.push(value.to_string()),
            None => error!(
                "Ignoring invalid config_file: {base_config_file}"
            ),
        }
    }
    flakes
}

pub fn app_details(app: &str, usermode: bool) -> app_config::AppConfig {
    /*!
    Read app config for given app base name
    !*/
    let config_file = registration::config_file(app, usermode);
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
        let config = registration::config_file(&app, usermode);
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
        } else if let Some(ref sandbox_conf) = details.sandbox {
            flake.engine = Some(defaults::BUBBLEWRAP_ENGINE.to_string());
            flake.target = Some(sandbox_conf.target_app_path.to_string());
            flake.host_app_path = Some(
                sandbox_conf.host_app_path.to_string()
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
