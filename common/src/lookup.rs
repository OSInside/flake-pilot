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
            FlakeLog::debug(&format!("Got Argument: {arg}"));
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

    pub fn get_instance_name() -> String {
        /*!
        Provide the instance name given via the @NAME pilot argument

        The special @NAME argument is not passed to the actual call
        and can be used to run different instances of the same
        application. If more than one @NAME argument is given they
        are concatenated in the order of appearance. If no @NAME
        argument is given an empty string is returned
        !*/
        env::args().skip(1).filter(|arg| arg.starts_with('@')).collect()
    }

    pub fn is_safe_instance_name(name: &str) -> bool {
        /*!
        Check the given @NAME pilot argument

        The instance name becomes part of file names, socket names
        and network device names. Therefore only characters which
        are safe to be used in these names are allowed
        !*/
        if name.is_empty() || name.len() > 64 || ! name.starts_with('@') {
            return false
        }
        name.chars().all(
            |c| c.is_ascii_alphanumeric() || matches!(c, '@' | '=' | '_' | '-')
        )
    }

    pub fn get_pilot_run_options(
        default_options: Vec<&str>
    ) -> HashMap<String, String> {
        /*!
        read runtime options which are only meant to be used for the
        pilot and should not interfere with the standard arguments
        passed along to the command call. For this purpose we deviate
        from the standard Unix/Linux commandline format and treat
        options passed as %name:value to be a pilot option

        The given default_options are provided by the flake
        configuration of the application. They are read first which
        allows to overwrite an option of the same name at call time
        !*/
        let mut pilot_options = HashMap::new();
        for option in default_options {
            let option = if option.starts_with('%') {
                option.to_string()
            } else {
                // for convenience the option can be configured
                // without the '%' pilot option marker
                format!("%{option}")
            };
            FlakeLog::debug(&format!("Got Default Pilot Option: {option}"));
            Self::set_pilot_option(&mut pilot_options, &option);
        }
        let args: Vec<String> = env::args().collect();
        for arg in &args[1..] {
            if arg.starts_with('%') {
                Self::set_pilot_option(&mut pilot_options, arg);
            }
        }
        pilot_options
    }

    fn set_pilot_option(
        pilot_options: &mut HashMap<String, String>, option: &str
    ) {
        /*!
        store the given %name:value pilot option. An option given
        without a value is stored with an empty value
        !*/
        let (name, value) = option.rsplit_once(':').unwrap_or_default();
        if name.is_empty() {
            pilot_options.insert(option.to_string(), "".to_string());
        } else {
            pilot_options.insert(name.to_string(), value.to_string());
        }
    }

    pub fn which(command: &str) -> bool {
        if let Ok(path) = env::var("PATH") {
            for path_entry in path.split(':') {
                let abs_command = format!("{path_entry}/{command}");
                if fs::metadata(abs_command).is_ok() {
                    return true;
                }
            }
        }
        false
    }
}
