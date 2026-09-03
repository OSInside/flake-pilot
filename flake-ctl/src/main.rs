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
#[macro_use]
extern crate log;

use env_logger::Env;
use std::process::{exit, ExitCode};

pub mod cli;
pub mod podman;
pub mod firecracker;
pub mod app;
pub mod app_config;
pub mod defaults;
pub mod fetch;
pub mod instance;
pub mod network;
pub mod output;
pub mod setup;
pub mod volume;

use flakes::config::get_flakes_dir;
use flakes::user::{User, mkdir};
use uzers::get_current_uid;
use std::fs;

#[tokio::main]
async fn main() -> Result<ExitCode, Box<dyn std::error::Error>> {
    setup_logger();

    let args = cli::parse_args();

    // In user mode only resources of the calling user are touched
    let user = usermode();

    match &args.command {
        // init
        cli::Commands::Init { force } => {
            // The setup this command creates is a precondition
            // for the other commands. It is therefore created
            // before the flake registry directory is expected
            // to exist
            if ! setup::init(user, *force) {
                return Ok(ExitCode::FAILURE)
            }
        },
        // list
        cli::Commands::List { format } => {
            init_flakes_dir(user)?;
            app::list(user, *format);
        },
        // firecracker engine
        cli::Commands::Firecracker { command } => {
            init_flakes_dir(user)?;
            match &command {
                // pull
                cli::Firecracker::Pull {
                    name, kis_image, rootfs, kernel, initrd, force
                } => {
                    if ! kis_image.is_none() {
                        exit(
                            firecracker::pull_kis_image(
                                name, kis_image.as_ref(), *force, user
                            ).await
                        );
                    } else {
                        exit(
                            firecracker::pull_component_image(
                                name, rootfs.as_ref(), kernel.as_ref(),
                                initrd.as_ref(), *force, user
                            ).await
                        );
                    }
                },
                // register
                cli::Firecracker::Register {
                    vm, app, target, run_as, overlay_size, no_net, resume,
                    force_vsock, include_tar, include_path, pilot_option,
                    force
                } => {
                    if *force {
                        app::remove(
                            app,
                            defaults::FIRECRACKER_PILOT,
                            user,
                            true,
                            *force
                        );
                    }
                    if app::init(Some(app), user) {
                        let mut ok = app::register(
                            Some(app), target.as_ref(),
                            defaults::FIRECRACKER_PILOT,
                            user
                        );
                        if ok {
                            ok = app::create_vm_config(
                                vm,
                                Some(app),
                                target.as_ref(),
                                run_as.as_ref(),
                                overlay_size.as_ref(),
                                *no_net,
                                *resume,
                                *force_vsock,
                                include_tar.as_ref().cloned(),
                                include_path.as_ref().cloned(),
                                pilot_option.as_ref().cloned(),
                                user,
                            );
                        }
                        if ! ok {
                            app::remove(
                                app, defaults::FIRECRACKER_PILOT,
                                user,
                                true,
                                *force
                            );
                            return Ok(ExitCode::FAILURE)
                        }
                    } else {
                        return Ok(ExitCode::FAILURE)
                    }
                },
                // show
                cli::Firecracker::Show { format } => {
                    instance::show(
                        defaults::FIRECRACKER_ENGINE, user, *format
                    );
                },
                // network
                cli::Firecracker::Network { command } => {
                    match &command {
                        // init
                        cli::Network::Init { outgoing_interface } => {
                            if ! network::init(outgoing_interface, user) {
                                return Ok(ExitCode::FAILURE)
                            }
                        },
                        // add
                        cli::Network::Add { app, instance } => {
                            if ! network::add(app, instance.as_ref(), user) {
                                return Ok(ExitCode::FAILURE)
                            }
                        },
                        // remove
                        cli::Network::Remove { app, instance } => {
                            if ! network::remove(app, instance.as_ref(), user) {
                                return Ok(ExitCode::FAILURE)
                            }
                        }
                    }
                },
                // volume
                cli::Firecracker::Volume { command } => {
                    match &command {
                        cli::Volume::Export { path } => {
                            if ! firecracker::export_volume(path) {
                                return Ok(ExitCode::FAILURE)
                            }
                        },
                        cli::Volume::Release { path } => {
                            if ! firecracker::release_volume(path) {
                                return Ok(ExitCode::FAILURE)
                            }
                        },
                        cli::Volume::Add { app, volume, instance } => {
                            if ! volume::add(
                                app, volume, instance.as_ref(), user
                            ) {
                                return Ok(ExitCode::FAILURE)
                            }
                        },
                        cli::Volume::Remove { app, volume, instance } => {
                            if ! volume::remove(
                                app, volume, instance.as_ref(), user
                            ) {
                                return Ok(ExitCode::FAILURE)
                            }
                        }
                    }
                },
                // remove
                cli::Firecracker::Remove { vm, app, force } => {
                    if ! app.is_none() && ! app::remove(
                        app.as_ref().map(String::as_str).unwrap(),
                        defaults::FIRECRACKER_PILOT,
                        user,
                        false,
                        *force
                    ) {
                        return Ok(ExitCode::FAILURE)
                    }
                    if ! vm.is_none() {
                        app::purge(
                            vm.as_ref().map(String::as_str).unwrap(),
                            defaults::FIRECRACKER_PILOT,
                            user
                        );
                    }
                }
            }
        },
        // podman engine
        cli::Commands::Podman { command } => {
            init_flakes_dir(user)?;
            match &command {
                // pull
                cli::Podman::Pull { uri } => {
                    exit(podman::pull(uri, user));
                },
                // load
                cli::Podman::Load { oci } => {
                    exit(podman::load(oci, user));
                },
                // register
                cli::Podman::Register {
                    container, app, target, base, check_host_dependencies,
                    layer, include_tar, include_path, resume, attach,
                    opt, pilot_option, info, force
                } => {
                    if *info {
                        podman::print_container_info(container);
                    } else {
                        if *force {
                            app::remove(
                                app.as_ref().map(String::as_str).unwrap(),
                                defaults::PODMAN_PILOT,
                                user,
                                true,
                                *force
                            );
                        }
                        if app::init(app.as_ref(), user) {
                            let mut ok = app::register(
                                app.as_ref(), target.as_ref(),
                                defaults::PODMAN_PILOT,
                                user
                            );
                            if ok {
                                ok = app::create_container_config(
                                    container,
                                    app.as_ref(),
                                    target.as_ref(),
                                    base.as_ref(),
                                    *check_host_dependencies,
                                    layer.as_ref().cloned(),
                                    include_tar.as_ref().cloned(),
                                    include_path.as_ref().cloned(),
                                    *resume,
                                    *attach,
                                    user,
                                    opt.as_ref().cloned(),
                                    pilot_option.as_ref().cloned(),
                                );
                            }
                            if ! ok {
                                app::remove(
                                    app.as_ref().map(String::as_str).unwrap(),
                                    defaults::PODMAN_PILOT,
                                    user,
                                    true,
                                    *force
                                );
                                return Ok(ExitCode::FAILURE)
                            }
                        } else {
                            return Ok(ExitCode::FAILURE)
                        }
                    }
                },
                // show
                cli::Podman::Show { format } => {
                    instance::show(defaults::PODMAN_ENGINE, user, *format);
                },
                // remove
                cli::Podman::Remove { container, app, force } => {
                    if ! app.is_none() && ! app::remove(
                        app.as_ref().map(String::as_str).unwrap(),
                        defaults::PODMAN_PILOT,
                        user,
                        false,
                        *force
                    ) {
                        return Ok(ExitCode::FAILURE)
                    }
                    if ! container.is_none() {
                        app::purge(
                            container.as_ref().map(String::as_str).unwrap(),
                            defaults::PODMAN_PILOT,
                            user
                        );
                    }
                }
            }
        },
    }
    Ok(ExitCode::SUCCESS)
}

fn usermode() -> bool {
    /*!
    Check if the command should operate on the resources of the
    calling user only. The mode is detected from the caller. Only
    the root user manages the system wide setup, every other user
    operates on its own registry in rootless mode
    !*/
    get_current_uid() != 0
}

fn init_flakes_dir(usermode: bool) -> Result<(), Box<dyn std::error::Error>> {
    /*!
    Create the directory the flake registrations are stored in
    !*/
    if usermode {
        // The user flake registry belongs to the calling user and
        // is therefore created with the privileges of that user
        fs::create_dir_all(get_flakes_dir(true))?;
    } else {
        // The flake registry is read by the pilots to learn how to
        // run an application. It is therefore only writable by root
        mkdir(&get_flakes_dir(false), "755", User::ROOT)?;
    }
    Ok(())
}

fn setup_logger() {
    let env = Env::default()
        .filter_or("FLAKE_LOG_LEVEL", "info")
        .write_style_or("FLAKE_LOG_STYLE", "always");

    env_logger::init_from_env(env);
}
