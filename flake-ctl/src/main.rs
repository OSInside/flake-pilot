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

use flakes::config::get_flakes_dir;
use flakes::user::{User, mkdir};
use uzers::get_current_username;

#[tokio::main]
async fn main() -> Result<ExitCode, Box<dyn std::error::Error>> {
    setup_logger();

    let args = cli::parse_args();

    mkdir(&get_flakes_dir(false), "777", User::ROOT)?;

    match &args.command {
        // list
        cli::Commands::List { mut user } => {
            info!("Registered applications:");
            let calling_user_name = get_current_username().unwrap();
            if calling_user_name == "root" {
                // if --user is used for the root user, we ignore it
                user = false
            }
            let app_names = app::app_names(user);
            if app_names.is_empty() {
                println!("No application(s) registered");
            } else {
                for app in app_names {
                    println!("- {app}");
                }
            }
        },
        // firecracker engine
        cli::Commands::Firecracker { command } => {
            match &command {
                // pull
                cli::Firecracker::Pull {
                    name, kis_image, rootfs, kernel, initrd, force
                } => {
                    if ! kis_image.is_none() {
                        exit(
                            firecracker::pull_kis_image(
                                name, kis_image.as_ref(), *force
                            ).await
                        );
                    } else {
                        exit(
                            firecracker::pull_component_image(
                                name, rootfs.as_ref(), kernel.as_ref(),
                                initrd.as_ref(), *force
                            ).await
                        );
                    }
                },
                // register
                cli::Firecracker::Register {
                    vm, app, target, run_as, overlay_size, no_net, resume,
                    force_vsock, include_tar, include_path
                } => {
                    if app::init(Some(app), false) {
                        let mut ok = app::register(
                            Some(app), target.as_ref(),
                            defaults::FIRECRACKER_PILOT,
                            false
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
                            );
                        }
                        if ! ok {
                            app::remove(
                                app, defaults::FIRECRACKER_PILOT,
                                false,
                                true
                            );
                            return Ok(ExitCode::FAILURE)
                        }
                    } else {
                        return Ok(ExitCode::FAILURE)
                    }
                },
                // remove
                cli::Firecracker::Remove { vm, app } => {
                    if ! app.is_none() && ! app::remove(
                        app.as_ref().map(String::as_str).unwrap(),
                        defaults::FIRECRACKER_PILOT,
                        false,
                        false
                    ) {
                        return Ok(ExitCode::FAILURE)
                    }
                    if ! vm.is_none() {
                        app::purge(
                            vm.as_ref().map(String::as_str).unwrap(),
                            defaults::FIRECRACKER_PILOT,
                            false
                        );
                    }
                }
            }
        },
        // podman engine
        cli::Commands::Podman { command, mut user } => {
            let calling_user_name = get_current_username().unwrap();
            if calling_user_name == "root" {
                // if --user is used for the root user, we ignore it
                user = false
            }
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
                    opt, info
                } => {
                    if *info {
                        podman::print_container_info(container);
                    } else if app::init(app.as_ref(), user) {
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
                                opt.as_ref().cloned()
                            );
                        }
                        if ! ok {
                            app::remove(
                                app.as_ref().map(String::as_str).unwrap(),
                                defaults::PODMAN_PILOT,
                                user,
                                true
                            );
                            return Ok(ExitCode::FAILURE)
                        }
                    } else {
                        return Ok(ExitCode::FAILURE)
                    }
                },
                // remove
                cli::Podman::Remove { container, app } => {
                    if ! app.is_none() && ! app::remove(
                        app.as_ref().map(String::as_str).unwrap(),
                        defaults::PODMAN_PILOT,
                        user,
                        false
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

fn setup_logger() {
    let env = Env::default()
        .filter_or("FLAKE_LOG_LEVEL", "info")
        .write_style_or("FLAKE_LOG_STYLE", "always");

    env_logger::init_from_env(env);
}
