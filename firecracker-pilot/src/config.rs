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
use std::collections::HashMap;
use std::{env, fs, path::Path, path::PathBuf};
use flakes::config::get_flakes_dir;
use flakes::lookup::Lookup;

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
    // first try to find system wide config
    let mut usermode = false;

    let base_path = get_base_path();
    let base_path = base_path.file_name().unwrap().to_str().unwrap();
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

pub fn config_file(program: &str, usermode: bool) -> String {
    format!("{}/{}.yaml", get_flakes_dir(usermode), program)
}

fn config_dir(program: &str, usermode: bool) -> String {
    format!("{}/{}.d", get_flakes_dir(usermode), program)
}

#[derive(Deserialize)]
pub struct Config<'a> {
    #[serde(borrow)]
    pub vm: VMSection<'a>,
    #[serde(borrow)]
    pub include: IncludeSection<'a>,
}

impl<'a> Config<'a> {
    pub fn runtime(&self) -> RuntimeSection<'_> {
        self.vm.runtime.as_ref().cloned().unwrap_or_default()
    }

    pub fn pilot_options(&self) -> Vec<&'a str> {
        match self.vm.runtime.as_ref() {
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

    /// Optional instance specific settings. The map is keyed by
    /// the @NAME instance selector given at call time. As the '@'
    /// character is reserved in YAML the key has to be quoted.
    /// For convenience the plain NAME without the '@' prefix is
    /// accepted as a key as well
    #[serde(default)]
    pub instance: Option<HashMap<String, InstanceSection<'a>>>,
}

impl<'a> EngineSection<'a> {
    pub fn get_boot_args(&self, instance_name: &str) -> Vec<&'a str> {
        /*!
        Provide the boot_args to use for the given instance

        The boot_args of the engine section are the base. An option
        which is also set in the instance section of instance_name
        takes the place of the global setting of the same option.
        Options which are not set globally are appended. If there
        is no section for instance_name the global boot_args are
        used unchanged
        !*/
        let instance_boot_args = self.get_instance_boot_args(instance_name);
        if instance_boot_args.is_empty() {
            return self.boot_args.clone()
        }
        let instance_keys: Vec<&str> = instance_boot_args.iter()
            .map(|boot_arg| boot_arg_name(boot_arg)).collect();

        let mut boot_args: Vec<&'a str> = Vec::new();
        let mut applied: Vec<&str> = Vec::new();
        for boot_arg in &self.boot_args {
            let name = boot_arg_name(boot_arg);
            if ! instance_keys.contains(&name) {
                boot_args.push(boot_arg);
                continue
            }
            // the instance setting(s) of this option take the
            // place of the global setting
            if ! applied.contains(&name) {
                applied.push(name);
                boot_args.extend(
                    instance_boot_args.iter()
                        .filter(|arg| boot_arg_name(arg) == name)
                );
            }
        }
        // options which are not set globally are appended
        for (boot_arg, name) in instance_boot_args.iter().zip(&instance_keys) {
            if ! applied.contains(name) {
                boot_args.push(boot_arg);
            }
        }
        boot_args
    }

    fn get_instance_boot_args(&self, instance_name: &str) -> Vec<&'a str> {
        /*!
        Provide the boot_args configured for the given instance
        !*/
        let instances = match self.instance.as_ref() {
            Some(instances) => instances,
            None => return Vec::new()
        };
        let instance_section = instances.get(instance_name).or_else(
            || instances.get(instance_name.trim_start_matches('@'))
        );
        match instance_section {
            Some(instance_section) => {
                if Lookup::is_debug() {
                    debug!("Using boot_args of instance {instance_name}");
                }
                instance_section.boot_args.clone().unwrap_or_default()
            },
            None => {
                if Lookup::is_debug() && ! instance_name.is_empty() {
                    debug!("No boot_args configured for {instance_name}");
                }
                Vec::new()
            }
        }
    }
}

fn boot_arg_name(boot_arg: &str) -> &str {
    /*!
    Provide the option name of a kernel boot argument

    The name is the part in front of the first '=' character.
    Boot arguments without a value, e.g 'quiet', are their own name
    !*/
    boot_arg.split('=').next().unwrap_or(boot_arg)
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct InstanceSection<'a> {
    /// Boot arguments effective for this instance only
    #[serde(borrow, default)]
    pub boot_args: Option<Vec<&'a str>>,
}

#[derive(Default, Debug, Deserialize, Clone, Display)]
pub enum CacheType {
    #[default]
    Writeback,
    Unsafe
}
