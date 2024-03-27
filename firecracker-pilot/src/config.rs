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
use strum::Display;
use std::{env, fs, path::PathBuf};
use flakes::config::get_flakes_dir;

lazy_static! {
    static ref CONFIG: Config<'static> = load_config();
}

/// Returns the config singleton
///
/// Will initialize the config on first call and return the cached version afterwards
pub fn config() -> &'static Config<'static> {
    &CONFIG
}

fn get_base_path() -> PathBuf {
    which::which(env::args().next().expect("Arg 0 must be present")).expect("Symlink should exist")
}

fn load_config() -> Config<'static> {
    /*!
    Read firecracker runtime configuration for given program

    FIRECRACKER_FLAKE_DIR/
       ├── program_name.d
       │   └── other.yaml
       └── program_name.yaml

    Config files below program_name.d are read in alpha sort order
    and attached to the master program_name.yaml file. The result
    is send to the Yaml parser
    !*/
    let base_path = get_base_path();
    let base_path = base_path.file_name().unwrap().to_str().unwrap();
    let base_yaml = fs::read_to_string(config_file(base_path));

    let mut extra_yamls: Vec<_> = fs::read_dir(config_dir(base_path))
        .into_iter()
        .flatten()
        .flatten()
        .map(|x| x.path())
        .collect();

    extra_yamls.sort();

    let full_yaml: String = base_yaml
        .into_iter()
        .chain(extra_yamls.into_iter().flat_map(fs::read_to_string))
        .collect();
    config_from_str(&full_yaml)
}

pub fn config_from_str(input: &str) -> Config<'static> {
    // Parse into a generic YAML to remove duplicate keys

    let yaml = yaml_rust::YamlLoader::load_from_str(input).unwrap();
    let yaml = yaml.first().unwrap();
    let mut buffer = String::new();
    yaml_rust::YamlEmitter::new(&mut buffer).dump(yaml).unwrap();

    // Convert to a String and leak it to make it static
    // Can not use serde_yaml::from_value because of lifetime limitations
    // Safety: This does not cause a reocurring memory leak since `load_config` is only called once
    let content = Box::leak(buffer.into_boxed_str());

    serde_yaml::from_str(content).unwrap()
}

pub fn config_file(program: &str) -> String {
    format!("{}/{}.yaml", get_flakes_dir(), program)
}

fn config_dir(program: &str) -> String {
    format!("{}/{}.d", get_flakes_dir(), program)
}

#[derive(Deserialize)]
pub struct Config<'a> {
    #[serde(borrow)]
    pub vm: VMSection<'a>,
    #[serde(borrow)]
    pub include: IncludeSection<'a>,
}

impl<'a> Config<'a> {
    pub fn runtime(&self) -> RuntimeSection {
        self.vm.runtime.as_ref().cloned().unwrap_or_default()
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
pub struct VMSection<'a> {
    /// Mandatory registration setup
    /// Name of the vm in the local registry
    pub name: &'a str,

    /// Path of the program to call inside of the vm (target)
    pub target_app_path: Option<&'a str>,

    /// Path of the program to register on the host
    pub host_app_path: &'a str,

    /// Optional registration setup
    /// VM runtime parameters
    #[serde(default)]
    pub runtime: Option<RuntimeSection<'a>>,
}

#[derive(Deserialize, Default, Clone)]
pub struct RuntimeSection<'a> {
    /// Run the VM engine as a user other than the
    /// default target user root. The user may be either
    /// a user name or a numeric user-ID (UID) prefixed
    /// with the ‘#’ character (e.g. #0 for UID 0). The call
    /// of the VM engine is performed by sudo.
    /// The behavior of sudo can be controlled via the
    /// file /etc/sudoers
    pub runas: &'a str,

    /// Resume the VM from previous execution.
    /// If the VM is still running, the app will be
    /// executed inside of this VM instance.
    ///
    /// Default: false
    #[serde(default)]
    pub resume: bool,

    /// Force using a vsock to communicate between guest and
    /// host if resume is set to false. In resume mode the
    /// vsock setup is always required.
    ///
    /// Default: false
    #[serde(default)]
    pub force_vsock: bool,

    pub firecracker: EngineSection<'a>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct EngineSection<'a> {
    /// Size of the VM overlay
    /// If specified a new ext2 overlay filesystem image of the
    /// specified size will be created and attached to the VM
    pub overlay_size: Option<&'a str>,

    pub cache_type: Option<CacheType>,
    pub mem_size_mib: Option<i64>,
    pub vcpu_count: Option<i64>,

    /// Path to rootfs image done by app registration
    pub rootfs_image_path: &'a str,

    /// Path to kernel image done by app registration
    pub kernel_image_path: &'a str,

    /// Optional path to initrd image done by app registration
    pub initrd_path: Option<&'a str>,

    pub boot_args: Vec<&'a str>,
}

#[derive(Debug, Deserialize, Clone, Display)]
pub enum CacheType {
    Writeback,
    Unsafe
}

impl Default for CacheType {
    fn default() -> Self {
        Self::Writeback
    }
}
