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
//! flake-pilot-helper performs every action of flake-pilot which
//! needs root privileges.
//!
//! The helper belongs to root and is called through sudo, or by
//! root directly. It provides a fixed set of commands, see cli.rs.
//! None of them passes an argument of the caller on to a program
//! unchecked: every argument is validated, and the authority of
//! the caller over the resource a command operates on is checked
//! against data which belongs to root. A sudo rule for the helper
//! therefore does not make a user equivalent to root, unlike a
//! rule for the general purpose programs the helper calls.
//!
#[macro_use]
extern crate log;

pub mod caller;
pub mod cli;
pub mod firewall;
pub mod meta;
pub mod network;
pub mod nfs;
pub mod podman;
pub mod run;
pub mod sandbox;
pub mod validate;

use env_logger::{Builder, WriteStyle};
use log::LevelFilter;
use std::env;
use std::process::ExitCode;

use crate::caller::Caller;

// Search path of the programs the helper calls. Every program is
// called by its absolute path, the search path only applies to the
// programs those call in turn
pub const HELPER_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";

fn main() -> ExitCode {
    // The caller is read from the environment sudo provides, before
    // the environment is cleared
    let caller = match Caller::from_environment() {
        Ok(caller) => caller,
        Err(error) => {
            eprintln!("flake-pilot-helper: {error}");
            return ExitCode::FAILURE
        }
    };
    // Debug mode is requested with the --debug option. PILOT_DEBUG
    // is honored as well, in case it got passed on to the helper
    let debug_env = env::var_os("PILOT_DEBUG").is_some();
    harden_process();

    let args = cli::parse_args();
    let debug = args.debug || debug_env;
    setup_logger(debug);
    let ok = match &args.command {
        cli::Commands::NetworkInit {
            network, outgoing_interface, host_resolver, host_dhcp
        } => network::init(
            &caller, network, outgoing_interface, *host_resolver, *host_dhcp
        ),
        cli::Commands::NetworkCleanup => network::cleanup(&caller),
        cli::Commands::NetworkConnect {
            config_file, instance, uid, prefer, exclude
        } => network::connect(
            &caller, config_file, instance.as_deref(), *uid, *prefer, exclude
        ),
        cli::Commands::NetworkDisconnect { config_file, instance, uid } => {
            network::disconnect(&caller, config_file, instance.as_deref(), *uid)
        },
        cli::Commands::VolumeExport { path, config_file, instance } => {
            nfs::export(&caller, path, config_file, instance.as_deref())
        },
        cli::Commands::VolumeRelease { path, config_file, instance } => {
            nfs::release(
                &caller, path, config_file.as_deref(), instance.as_deref()
            )
        },
        cli::Commands::SandboxMount { instance, rootfs, layers, size } => {
            sandbox::mount(&caller, instance, rootfs, layers, size.as_deref())
        },
        cli::Commands::SandboxUmount { instance, discard } => {
            sandbox::umount(&caller, instance, *discard)
        },
        cli::Commands::PodmanPilot { app, term, args } => {
            podman::run_pilot(&caller, app, debug, term.as_deref(), args)
        },
        cli::Commands::MetaDirsInit => meta::init_dirs(&caller)
    };
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn harden_process() {
    /*!
    Clear the environment and the umask inherited from the caller

    Nothing of the environment of the caller is used. sudo resets
    it, but which variables it keeps is a matter of its
    configuration, and root may call the helper with any
    environment. The programs the helper calls get a fixed search
    path. Files the helper creates get their permissions from the
    helper, not from the umask of the caller
    !*/
    for (name, _) in env::vars_os() {
        env::remove_var(name);
    }
    env::set_var("PATH", HELPER_PATH);
    unsafe { libc::umask(0o022) };
}

fn setup_logger(debug: bool) {
    /*!
    Log to stderr, the caller passes it through to the user. The
    standard output is reserved for the results of a command

    The log messages are only shown in debug mode, see PILOT_DEBUG.
    Errors are always shown, they tell the caller why a command
    was refused or failed. The messages are written without color
    !*/
    Builder::new()
        .filter_level(if debug { LevelFilter::Debug } else { LevelFilter::Error })
        .write_style(WriteStyle::Never)
        .init();
}
