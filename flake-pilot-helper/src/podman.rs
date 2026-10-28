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
//! System wide podman flakes run for a user
//!
//! A podman flake registered system wide runs in the podman setup
//! of root. A user who calls such a flake gets its pilot started as
//! root by the helper. The flake is selected by the user, its
//! container and all of its runtime options are taken from the
//! registration, which belongs to root. The arguments of the user
//! are passed to the application in the container.
//!
use std::os::unix::process::CommandExt;
use std::process::Command;

use flakes::config::get_flakes_dir;
use flakes::defaults;

use crate::caller::Caller;
use crate::validate;

pub fn run_pilot(
    caller: &Caller, app: &str, debug: bool, term: Option<&str>,
    args: &[String]
) -> bool {
    /*!
    Replace the helper by podman-pilot running the given system
    wide flake for the caller

    The pilot gets a fresh environment. The variables a flake
    configuration references as %VAR in its runtime options, e.g
    %HOME, therefore resolve to the values of the user database for
    the caller and not to values the caller picked. The pilot names
    the meta data and the containers of the instance after the
    caller given in defaults::CALLER_UID_VAR
    !*/
    if let Err(error) = validate::system_flake(
        app, defaults::PODMAN_PILOT, &get_flakes_dir(false)
    ) {
        error!("{error}");
        return false
    }
    let term = term.filter(|term| match validate::terminal_type(term) {
        Ok(()) => true,
        Err(error) => {
            warn!("{error}, not passed on");
            false
        }
    });
    let mut call = Command::new(defaults::PODMAN_PILOT);
    call.arg0(app)
        .args(args)
        .env_clear()
        .env("PATH", crate::HELPER_PATH)
        .env("HOME", &caller.home)
        .env("USER", &caller.name)
        .env("LOGNAME", &caller.name)
        .env(defaults::CALLER_UID_VAR, caller.uid.to_string());
    if let Some(term) = term {
        call.env("TERM", term);
    }
    if debug {
        call.env("PILOT_DEBUG", "1");
    }
    let error = call.exec();
    error!("Failed to execute {}: {error}", defaults::PODMAN_PILOT);
    false
}
