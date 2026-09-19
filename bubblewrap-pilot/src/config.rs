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
use lazy_static::lazy_static;
use serde::Deserialize;
use flakes::registration;

lazy_static! {
    static ref PROGRAM: String = registration::program_name();
}

lazy_static! {
    static ref USERMODE: bool = registration::is_usermode(&PROGRAM);
}

lazy_static! {
    static ref CONFIG: Config<'static> = registration::load_config(
        &PROGRAM, *USERMODE
    );
}

/// Returns the config singleton
///
/// Will initialize the config on first call and return
/// the cached version afterwards
pub fn config() -> &'static Config<'static> {
    &CONFIG
}

/// Returns true if the flake is registered for the calling user
///
/// The registration of the user is only read if there is no
/// system wide registration of the same application. Which one
/// is in use also decides which flakes setup, and with it which
/// meta data directory, belongs to the flake
pub fn usermode() -> bool {
    *USERMODE
}

/// Reads the given sandbox runtime configuration
///
/// The yaml document is expected to be the registration of
/// the calling program, see `flakes::registration`
pub fn config_from_str(input: &str) -> Config<'static> {
    registration::config_from_str(input, &PROGRAM)
}

#[derive(Deserialize)]
pub struct Config<'a> {
    #[serde(borrow)]
    pub sandbox: SandboxSection<'a>
}

impl<'a> Config<'a> {
    pub fn runtime(&self) -> RuntimeSection<'_> {
        self.sandbox.runtime.as_ref().cloned().unwrap_or_default()
    }

    pub fn pilot_options(&self) -> Vec<&'a str> {
        match self.sandbox.runtime.as_ref() {
            Some(runtime) => runtime.pilot_options
                .as_ref().cloned().unwrap_or_default(),
            None => Vec::new()
        }
    }
}

#[derive(Deserialize)]
pub struct SandboxSection<'a> {
    /// Mandatory registration setup
    /// Path of the directory on the host which provides the
    /// root filesystem of the sandbox. It is mounted as the
    /// read only root of the new root system
    pub name: &'a str,

    /// Path of the program to call inside of the sandbox (target)
    pub target_app_path: Option<&'a str>,

    /// Path of the program to register on the host
    pub host_app_path: &'a str,

    /// Optional registration setup
    /// Sandbox runtime parameters
    #[serde(default)]
    pub runtime: Option<RuntimeSection<'a>>
}

#[derive(Deserialize, Default, Clone)]
pub struct RuntimeSection<'a> {
    /// Name of the user the sandbox is created for
    ///
    /// bubblewrap uses user namespaces and needs no privileges.
    /// The sandbox therefore belongs to the calling user unless
    /// another user is configured here. A sandbox of another
    /// user is created through sudo
    ///
    /// Default: any, which is the calling user
    #[serde(default)]
    pub runas: &'a str,

    /// Optional pilot options in the format:
    /// - %name or %name:value
    ///
    /// Pilot options are not passed to the application call but
    /// control the behavior of the pilot. An option configured
    /// here is always effective and does not have to be given
    /// at call time. An option of the same name provided at call
    /// time takes precedence over the configured one
    #[serde(default)]
    pub pilot_options: Option<Vec<&'a str>>,

    /// Caller arguments for the bubblewrap engine in the format:
    /// - BWRAP_OPTION_NAME_AND_OPTIONAL_VALUE
    ///
    /// An option and its value(s) can be given in one entry, e.g
    /// "--ro-bind /etc /etc". For details on the options please
    /// consult the bwrap documentation
    #[serde(default)]
    pub bubblewrap: Option<Vec<&'a str>>
}
