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

#[cfg(test)]
pub mod tests;

use std::env;
use std::os::unix::process::CommandExt;
use std::process::{ExitCode, Termination};

use config::{config, is_usermode};
use env_logger::Env;
use flakes::error::FlakeError;
use flakes::helper::helper_call;
use flakes::registration;
use flakes::user::get_caller_uid;
use uzers::get_current_uid;

pub mod podman;
pub mod defaults;
pub mod config;

fn main() -> ExitCode {
    setup_logger();
    // A system wide flake runs in the podman setup of root. A user
    // who calls it hands the call over to the helper, which starts
    // this pilot as root for that user
    if ! is_usermode() && get_current_uid() != 0 {
        return run_through_helper()
    }
    // A pilot started as root for a user only runs system wide
    // flakes. The registration of a flake of a user, and the setup
    // it references, belongs to that user and is never used by root
    if is_usermode() && get_current_uid() == 0 && get_caller_uid() != 0 {
        error!("A flake registered by a user is not run as root");
        return ExitCode::FAILURE
    }
    // load config now so we can terminate early if the config is invalid
    config();
    // past here there should be no more panics

    let result = run();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            error!("{err}");
            err.report()
        },
    }
}

fn run() -> Result<(), FlakeError> {
    let program_name = registration::program_name();

    let container = podman::create(&program_name)?;
    let cid = &container.0;
    podman::start(
        &program_name, cid
    )
}

fn run_through_helper() -> ExitCode {
    /*!
    Replace this pilot by the helper, which runs the system wide
    flake for the calling user, see its podman-pilot command

    The helper starts the pilot with a fresh environment. The
    terminal type and the debug mode, see helper_call(), are handed
    over as arguments, the arguments of the application are passed
    along unchanged
    !*/
    let mut call = helper_call("podman-pilot");
    call.arg("--app").arg(registration::program_abs_path());
    if let Ok(term) = env::var("TERM") {
        call.arg("--term").arg(term);
    }
    call.arg("--").args(env::args().skip(1));
    let error = call.exec();
    error!("Failed to run the flake through the helper: {error}");
    ExitCode::FAILURE
}

fn setup_logger() {
    let env = Env::default()
        .filter_or("FLAKE_LOG_LEVEL", "debug")
        .write_style_or("FLAKE_LOG_STYLE", "always");

    env_logger::init_from_env(env);
}
