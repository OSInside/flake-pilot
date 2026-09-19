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
#[macro_use]
extern crate log;

#[cfg(test)]
pub mod tests;

use std::process::{ExitCode, Termination};

use config::config;
use env_logger::Env;
use flakes::error::FlakeError;

pub mod app_path;
pub mod bubblewrap;
pub mod defaults;
pub mod config;
pub mod overlay;

fn main() -> ExitCode {
    setup_logger();
    // load config now so we can terminate early if the config is invalid
    config();
    // past here there should be no more panics

    let result = run();

    match result {
        Ok(code) => code,
        Err(err) => {
            error!("{err}");
            err.report()
        },
    }
}

fn run() -> Result<ExitCode, FlakeError> {
    /*!
    Create the sandbox for the calling program and run it there

    The exit code of the application in the sandbox is passed
    on as the exit code of this pilot
    !*/
    let program_path = app_path::program_abs_path();
    let program_name = app_path::basename(&program_path);

    let sandbox_id_file = bubblewrap::create(&program_name)?;
    bubblewrap::start(&program_name, &sandbox_id_file)
}

fn setup_logger() {
    let env = Env::default()
        .filter_or("FLAKE_LOG_LEVEL", "debug")
        .write_style_or("FLAKE_LOG_STYLE", "always");

    env_logger::init_from_env(env);
}
