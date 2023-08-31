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
use std::backtrace::Backtrace;
use std::collections::HashMap;
use std::env;
use std::fs;

use crate::flakelog::FlakeLog;

#[derive(Debug, Default, Clone, Copy)]
pub struct Lookup {
}

impl Lookup {
    pub fn do_trace() {
        if Self::is_debug() {
            debug!("{}", Backtrace::force_capture());
        }
    }

    pub fn is_debug() -> bool {
        env::var("PILOT_DEBUG").is_ok()
    }

    pub fn get_run_cmdline(
        init: Vec<String>, quote_for_kernel_cmdline: bool
    ) -> Vec<String> {
        /*!
        setup run commandline for the command call
        !*/
        let args: Vec<String> = env::args().collect();
        let mut run: Vec<String> = init;
        for arg in &args[1..] {
            FlakeLog::debug(&format!("Got Argument: {}", arg));
            if ! arg.starts_with('@') && ! arg.starts_with('%') {
                if quote_for_kernel_cmdline {
                    run.push(arg.replace('-', "\\-").to_string());
                } else {
                    run.push(arg.to_string());
                }
            }
        }
        run
    }

    pub fn get_pilot_run_options() -> HashMap<String, String> {
        /*!
        read runtime options which are only meant to be used for the
        pilot and should not interfere with the standard arguments
        passed along to the command call. For this purpose we deviate
        from the standard Unix/Linux commandline format and treat
        options passed as %name:value to be a pilot option
        !*/
        let args: Vec<String> = env::args().collect();
        let mut pilot_options = HashMap::new();
        for arg in &args[1..] {
            if arg.starts_with('%') {
                let (name, value) = arg.rsplit_once(':').unwrap_or_default();
                if name.is_empty() {
                    pilot_options.insert(arg.to_string(), "".to_string());
                } else {
                    pilot_options.insert(name.to_string(), value.to_string());
                }
            }
        }
        pilot_options
    }

    pub fn which(command: &str) -> bool {
        if let Ok(path) = env::var("PATH") {
            for path_entry in path.split(':') {
                let abs_command = format!("{}/{}", path_entry, command);
                if fs::metadata(abs_command).is_ok() {
                    return true;
                }
            }
        }
        false
    }
}
