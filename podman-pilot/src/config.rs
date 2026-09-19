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

/// Reads the given container runtime configuration
///
/// The yaml document is expected to be the registration of
/// the calling program, see `flakes::registration`
pub fn config_from_str(input: &str) -> Config<'static> {
    registration::config_from_str(input, &PROGRAM)
}

#[derive(Deserialize)]
pub struct Config<'a> {
    #[serde(borrow)]
    pub container: ContainerSection<'a>,
    #[serde(borrow)]
    pub include: IncludeSection<'a>
}

impl<'a> Config<'a> {
    pub fn is_delta_container(&self) -> bool {
        self.container.base_container.is_some()
    }

    pub fn runtime(&self) -> RuntimeSection<'_> {
        self.container.runtime.as_ref().cloned().unwrap_or_default()
    }

    pub fn layers(&self) -> Vec<&'a str> {
        self.container.layers.as_ref().cloned().unwrap_or_default()
    }

    pub fn pilot_options(&self) -> Vec<&'a str> {
        match self.container.runtime.as_ref() {
            Some(runtime) => runtime.pilot_options
                .as_ref().cloned().unwrap_or_default(),
            None => Vec::new()
        }
    }

    pub fn tars(&self) -> Vec<&'a str> {
        self.include.tar.as_ref().cloned().unwrap_or_default()
    }

    pub fn paths(&self) -> Vec<&'a str> {
        self.include.path.as_ref().cloned().unwrap_or_default()
    }
}

#[derive(Deserialize)]
pub struct IncludeSection<'a> {
    #[serde(borrow)]
    tar: Option<Vec<&'a str>>,
    path: Option<Vec<&'a str>>,
}

#[derive(Deserialize)]
pub struct ContainerSection<'a> {
    /// Mandatory registration setup
    /// Name of the container in the local registry
    pub name: &'a str,

    /// Path of the program to call inside of the container (target)
    pub target_app_path: Option<&'a str>,

    /// Path of the program to register on the host
    pub host_app_path: &'a str,

    /// Optional base container to use with a delta 'container: name'
    ///
    /// If specified the given 'container: name' is expected to be
    /// an overlay for the specified base_container. podman-pilot
    /// combines the 'container: name' with the base_container into
    /// one overlay and starts the result as a container instance
    ///
    /// Default: not_specified
    pub base_container: Option<&'a str>,

    /// Optional check if the container has dependencies to the host
    pub check_host_dependencies: bool,

    /// Optional additional container layers on top of the
    /// specified base container
    #[serde(default)]
    layers: Option<Vec<&'a str>>,

    /// Optional registration setup
    /// Container runtime parameters
    #[serde(default)]
    pub runtime: Option<RuntimeSection<'a>>,
}

#[derive(Deserialize, Default, Clone)]
pub struct RuntimeSection<'a> {
    /// Shows which user has created this registration
    /// file and serves as indicator for using the system wide
    /// or user specific flake setup
    pub runas: &'a str,

    /// Resume the container from previous execution.
    ///
    /// If the container is still running, the app will be
    /// executed inside of this container instance.
    ///
    /// Default: false
    #[serde(default)]
    pub resume: bool,

    /// Attach to the container if still running, rather than
    /// executing the app again. Only makes sense for interactive
    /// sessions like a shell running as app in the container.
    ///
    /// Default: false
    #[serde(default)]
    pub attach: bool,

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

    /// Caller arguments for the podman engine in the format:
    /// - PODMAN_OPTION_NAME_AND_OPTIONAL_VALUE
    ///
    /// For details on podman options please consult the
    /// podman documentation.
    #[serde(default)]
    pub podman: Option<Vec<&'a str>>,
}
