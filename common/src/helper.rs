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
//! Calls of the flake-pilot helper
//!
//! Every action of flake-pilot which needs root privileges is
//! performed by the helper, see defaults::HELPER. It provides a
//! fixed set of commands and validates their arguments as well as
//! the authority of its caller. This module creates the calls of
//! these commands for the tools which run with the privileges of
//! the calling user.
//!
use std::process::Command;

use uzers::get_current_uid;

use crate::defaults;
use crate::lookup::Lookup;

pub fn helper_call(command: &str) -> Command {
    /*!
    Create a call of the given helper command

    A caller who is not root runs the helper through sudo. sudo
    resets the environment and tells the helper who called it. A
    caller who is root runs the helper directly. The environment
    of the caller is not passed along in this case either: the
    helper takes its caller from the SUDO_UID variable, a root
    shell started through sudo would otherwise hand over the ID of
    the user who started it

    The log messages of the helper are shown if PILOT_DEBUG is set
    for the caller. The helper does not see the environment of the
    caller, debug mode is requested with its --debug option. It is
    given after the command, a sudo rule can be limited to helper
    commands by their name in the first argument
    !*/
    let mut call = if get_current_uid() == 0 {
        let mut call = Command::new(defaults::HELPER);
        call.env_clear();
        call
    } else {
        let mut call = Command::new(defaults::SUDO_TOOL);
        call.arg(defaults::HELPER);
        call
    };
    call.arg(command);
    if Lookup::is_debug() {
        call.arg("--debug");
    }
    call
}

#[cfg(test)]
mod tests {
    use super::helper_call;
    use crate::defaults;
    use uzers::get_current_uid;

    #[test]
    fn test_helper_call() {
        let call = helper_call("network-cleanup");
        let args: Vec<String> = call.get_args()
            .map(|arg| arg.to_string_lossy().to_string()).collect();
        let mut expected = vec!["network-cleanup"];
        if std::env::var_os("PILOT_DEBUG").is_some() {
            expected.push("--debug");
        }
        if get_current_uid() == 0 {
            assert_eq!(defaults::HELPER, call.get_program());
            assert_eq!(expected, args);
        } else {
            // the helper is the only program called through sudo
            assert_eq!(defaults::SUDO_TOOL, call.get_program());
            expected.insert(0, defaults::HELPER);
            assert_eq!(expected, args);
        }
    }
}
