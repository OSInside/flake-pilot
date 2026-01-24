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
use std::path::Path;
use lazy_static::lazy_static;
use serde::Deserialize;
use std::{env, path::PathBuf, fs};
use flakes::config::get_flakes_dir;

lazy_static! {
    static ref CONFIG: Config<'static> = load_config();
}

/// Returns the config singleton
///
/// Will initialize the config on first call and return
/// the cached version afterwards
pub fn config() -> &'static Config<'static> {
    &CONFIG
}

fn get_base_path() -> PathBuf {
    which::which(env::args().next().expect("Arg 0 must be present")).expect("Symlink should exist")
}

fn load_config() -> Config<'static> {
    /*!
    Read container runtime configuration for given program

    CONTAINER_FLAKE_DIR/
       ├── program_name.d
       │   └── other.yaml
       └── program_name.yaml

    Config files below program_name.d are read in alpha sort order
    and attached to the master program_name.yaml file. The result
    is send to the Yaml parser
    !*/
    // first try to find system wide config
    let mut usermode = false;

    let base_path = get_base_path();
    let base_path  = base_path.file_name().unwrap().to_str().unwrap();
    let mut base_file = config_file(base_path, usermode);

    if ! Path::new(&base_file).exists() {
        // no system wide config found, try user specific
        usermode = true;
        base_file = config_file(base_path, usermode);
        if ! Path::new(&base_file).exists() {
            panic!(
                "No user/system wide flake registration found for: {}",
                base_path
            )
        }
    }

    let base_yaml = fs::read_to_string(&base_file);

    let mut extra_yamls: Vec<_> = fs::read_dir(config_dir(base_path, usermode))
        .into_iter()
        .flatten()
        .flatten()
        .map(|x| x.path()).collect();

    extra_yamls.sort();

    let full_yaml: String = base_yaml.into_iter().chain(
        extra_yamls.into_iter().flat_map(fs::read_to_string)
    ).collect();
    config_from_str(&full_yaml, usermode)
}

pub fn config_from_str(input: &str, usermode: bool) -> Config<'static> {
    // Parse into a generic YAML to remove duplicate keys
    let yaml_documents = match yaml_rust::YamlLoader::load_from_str(input) {
        Ok(yaml_documents) => {
            yaml_documents
        }
        Err(error) => {
            panic!(
                "Failed to parse yaml input at: {:?}: {}",
                config_file(
                    get_base_path().file_name().unwrap().to_str().unwrap(),
                    usermode
                ), error
            )
        }
    };

    let yaml = yaml_documents.first();
    if let Some(yaml) = yaml {
        let mut buffer = String::new();
        yaml_rust::YamlEmitter::new(&mut buffer).dump(yaml).unwrap();

        // Convert to a String and leak it to make it static
        // Can not use serde_yaml::from_value because of lifetime limitations
        // Safety: This does not cause a reocurring memory leak
        // since `load_config` is only called once
        let content = Box::leak(buffer.into_boxed_str());

        serde_yaml::from_str(content).unwrap()
    } else {
        panic!(
            "No configuration data provided for {:?} in {} or {}",
            get_base_path(), get_flakes_dir(false), get_flakes_dir(true)
        )
    }
}

pub fn config_file(program: &str, usermode: bool) -> String {
    format!("{}/{}.yaml", get_flakes_dir(usermode), program)
}

fn config_dir(program: &str, usermode: bool) -> String {
    format!("{}/{}.d", get_flakes_dir(usermode), program)
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

    /// Caller arguments for the podman engine in the format:
    /// - PODMAN_OPTION_NAME_AND_OPTIONAL_VALUE
    ///
    /// For details on podman options please consult the
    /// podman documentation.
    #[serde(default)]
    pub podman: Option<Vec<&'a str>>,
}
