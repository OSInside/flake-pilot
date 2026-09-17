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
use std::path::Path;
use lazy_static::lazy_static;
use serde::Deserialize;
use std::{env, path::PathBuf, fs};
use flakes::config::get_flakes_dir;

lazy_static! {
    static ref CONFIG: Config<'static> = load_config();
}

lazy_static! {
    static ref USERMODE: bool = is_usermode();
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

fn get_base_path() -> PathBuf {
    which::which(env::args().next().expect("Arg 0 must be present")).expect("Symlink should exist")
}

fn is_usermode() -> bool {
    /*!
    Check if the flake is registered for the calling user only
    !*/
    let base_path = get_base_path();
    let base_path = base_path.file_name().unwrap().to_str().unwrap();
    ! Path::new(&config_file(base_path, false)).exists()
}

fn load_config() -> Config<'static> {
    /*!
    Read sandbox runtime configuration for given program

    FLAKE_DIR/
       ├── program_name.d
       │   └── other.yaml
       └── program_name.yaml

    Config files below program_name.d are read in alpha sort order
    and attached to the master program_name.yaml file. The result
    is send to the Yaml parser
    !*/
    // the system wide registration takes precedence over the
    // registration of the calling user
    let usermode = is_usermode();

    let base_path = get_base_path();
    let base_path = base_path.file_name().unwrap().to_str().unwrap();
    let base_file = config_file(base_path, usermode);

    if ! Path::new(&base_file).exists() {
        panic!(
            "No user/system wide flake registration found for: {}",
            base_path
        )
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
