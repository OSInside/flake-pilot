//
// Copyright (c) 2022 Elektrobit Automotive GmbH
// Copyright (c) 2023 Marcus Schäfer
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
use std::collections::HashMap;
use std::io::{Error, ErrorKind};
use std::path::Path;
use serde::{Serialize, Deserialize};
use serde_yaml::{self};
use crate::defaults;
use crate::firecracker;

type GenericError = Box<dyn std::error::Error + Send + Sync + 'static>;

// AppConfig represents application yaml configuration
#[derive(Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub include: AppInclude,
    pub container: Option<AppContainer>,
    pub vm: Option<AppFireCracker>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppContainer {
    pub name: String,
    pub target_app_path: String,
    pub host_app_path: String,
    pub base_container: Option<String>,
    pub check_host_dependencies: bool,
    pub layers: Option<Vec<String>>,
    pub runtime: Option<AppContainerRuntime>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct AppContainerRuntime {
    pub runas: Option<String>,
    pub resume: Option<bool>,
    pub attach: Option<bool>,
    pub pilot_options: Option<Vec<String>>,
    pub podman: Option<Vec<String>>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct AppInclude {
    pub tar: Option<Vec<String>>,
    pub path: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppFireCracker {
    pub name: String,
    pub target_app_path: String,
    pub host_app_path: String,
    pub base_vm: Option<String>,
    pub layers: Option<Vec<String>>,
    pub runtime: Option<AppFireCrackerRuntime>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct AppFireCrackerRuntime {
    pub runas: Option<String>,
    pub resume: Option<bool>,
    pub force_vsock: Option<bool>,
    pub pilot_options: Option<Vec<String>>,
    pub firecracker: Option<AppFireCrackerEngine>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct AppFireCrackerEngine {
    pub boot_args: Option<Vec<String>>,
    pub overlay_size: Option<String>,
    pub rootfs_image_path: Option<String>,
    pub kernel_image_path: Option<String>,
    pub initrd_path: Option<String>,
    pub mem_size_mib: Option<i32>,
    pub vcpu_count: Option<i32>,
    pub cache_type: Option<String>,
    pub instance: Option<HashMap<String, AppFireCrackerInstance>>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct AppFireCrackerInstance {
    pub boot_args: Option<Vec<String>>,
}

fn normalize_pilot_options(pilot_options: &[String]) -> Vec<String> {
    /*!
    Provide the pilot options in the %name or %name:value format

    A pilot option is marked by a leading '%' character. For
    convenience the option can be specified without that marker
    which is added here
    !*/
    pilot_options.iter().map(
        |pilot_option| if pilot_option.starts_with('%') {
            pilot_option.to_string()
        } else {
            format!("%{pilot_option}")
        }
    ).collect()
}

impl AppConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn save_container(
        config_file: &Path,
        container: &str,
        target_app_path: &str,
        host_app_path: &str,
        base: Option<&String>,
        check_host_dependencies: bool,
        layers: Option<Vec<String>>,
        includes_tar: Option<Vec<String>>,
        includes_path: Option<Vec<String>>,
        resume: bool,
        attach: bool,
        run_as: Option<&String>,
        opts: Option<Vec<String>>,
        pilot_options: Option<Vec<String>>,
    ) -> Result<(), GenericError> {
        /*!
        save stores an AppConfig to the given file
        !*/
        let template = std::fs::File::open(defaults::FLAKE_TEMPLATE_CONTAINER)
            .unwrap_or_else(|_| panic!("Failed to open {}", defaults::FLAKE_TEMPLATE_CONTAINER));
        let mut yaml_config: AppConfig =
            serde_yaml::from_reader(template).expect("Failed to import config template");
        let container_config = yaml_config.container.as_mut().unwrap();

        container_config.name = container.to_string();
        container_config.target_app_path = target_app_path.to_string();
        container_config.host_app_path = host_app_path.to_string();
        if let Some(base) = base {
            container_config.base_container = Some(
                base.to_string()
            );
        }
        if check_host_dependencies {
            container_config.check_host_dependencies = check_host_dependencies
        }
        if let Some(layers) = &layers {
            container_config.layers = Some(layers.to_vec());
        }
        if resume {
            container_config.runtime.as_mut().unwrap()
                .resume = Some(resume);
        } else if attach {
            container_config.runtime.as_mut().unwrap()
                .attach = Some(attach);
        }
        if let Some(run_as) = run_as {
            container_config.runtime.as_mut().unwrap()
                .runas = Some(run_as.to_string());
        }
        if let Some(includes_tar) = &includes_tar {
            yaml_config.include.tar = Some(includes_tar.to_vec());
        }
        if let Some(includes_path) = &includes_path {
            yaml_config.include.path = Some(includes_path.to_vec());
        }
        if let Some(opts) = &opts {
            let mut final_opts: Vec<String> = Vec::new();
            for opt in opts {
                if let Some(stripped_opt) = opt.strip_prefix('\\') {
                    final_opts.push(stripped_opt.to_string())
                } else {
                    final_opts.push(opt.to_string())
                }
            }
            container_config.runtime.as_mut().unwrap().podman = Some(
                final_opts
            );
        }
        if let Some(pilot_options) = &pilot_options {
            container_config.runtime.as_mut().unwrap().pilot_options = Some(
                normalize_pilot_options(pilot_options)
            );
        }

        let config = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(config_file)
            .unwrap_or_else(|_| panic!("Failed to open {:?}", config_file));
        serde_yaml::to_writer(config, &yaml_config).unwrap();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_vm(
        config_file: &Path,
        vm: &String,
        target_app_path: &str,
        host_app_path: &String,
        run_as: Option<&String>,
        overlay_size: Option<&String>,
        no_net: bool,
        resume: bool,
        force_vsock: bool,
        includes_tar: Option<Vec<String>>,
        includes_path: Option<Vec<String>>,
        pilot_options: Option<Vec<String>>,
        usermode: bool,
    ) -> Result<(), GenericError> {
        /*!
        save stores an AppConfig to the given file
        !*/
        let image_dir = firecracker::get_image_dir(vm, usermode);
        let template = std::fs::File::open(defaults::FLAKE_TEMPLATE_FIRECRACKER)
            .unwrap_or_else(|_| panic!(
                "Failed to open {}", defaults::FLAKE_TEMPLATE_FIRECRACKER)
            );
        let mut yaml_config: AppConfig =
            serde_yaml::from_reader(template).expect(
                "Failed to import config template"
            );
        let vm_config = yaml_config.vm.as_mut().unwrap();

        vm_config.name = vm.to_string();
        vm_config.target_app_path = target_app_path.to_string();
        vm_config.host_app_path = host_app_path.to_string();

        if resume {
            vm_config.runtime.as_mut().unwrap()
                .resume = Some(resume);
        }
        if force_vsock {
            vm_config.runtime.as_mut().unwrap()
                .force_vsock = Some(force_vsock);
        }
        if let Some(run_as) = run_as {
            vm_config.runtime.as_mut().unwrap()
                .runas = Some(run_as.to_string());
        }
        if let Some(includes_tar) = &includes_tar {
            yaml_config.include.tar = Some(includes_tar.to_vec());
        }
        if let Some(includes_path) = &includes_path {
            yaml_config.include.path = Some(includes_path.to_vec());
        }
        if let Some(pilot_options) = &pilot_options {
            vm_config.runtime.as_mut().unwrap().pilot_options = Some(
                normalize_pilot_options(pilot_options)
            );
        }
        if let Some(overlay_size) = overlay_size {
            vm_config.runtime.as_mut().unwrap()
                .firecracker.as_mut().unwrap()
                .overlay_size = Some(overlay_size.to_string());
        }
        let rootfs_image_path = format!(
            "{}/{}", image_dir, defaults::FIRECRACKER_ROOTFS_NAME
        );
        if Path::new(&rootfs_image_path).exists() {
            vm_config.runtime.as_mut().unwrap()
                .firecracker.as_mut().unwrap()
                .rootfs_image_path = Some(rootfs_image_path);
        } else {
            return Err(
                Box::new(Error::new(
                    ErrorKind::NotFound,
                    format!("No rootfs image found: {rootfs_image_path}")
                ))
            )
        }

        let kernel_image_path = format!(
            "{}/{}", image_dir, defaults::FIRECRACKER_KERNEL_NAME
        );
        if Path::new(&kernel_image_path).exists() {
            vm_config.runtime.as_mut().unwrap()
                .firecracker.as_mut().unwrap()
                .kernel_image_path = Some(kernel_image_path);
        } else {
            return Err(
                Box::new(Error::new(
                    ErrorKind::NotFound,
                    format!("No kernel image found: {kernel_image_path}")
                ))
            )
        }

        let initrd_path = format!(
            "{}/{}", image_dir, defaults::FIRECRACKER_INITRD_NAME
        );
        if Path::new(&initrd_path).exists() {
            vm_config.runtime.as_mut().unwrap()
                .firecracker.as_mut().unwrap()
                .initrd_path = Some(initrd_path);
        }

        if no_net {
            let mut boot_args: Vec<String> = Vec::new();
            let firecracker_section = vm_config.runtime.as_mut().unwrap()
                .firecracker.as_mut().unwrap();
            for boot_arg in
                firecracker_section.boot_args.as_mut().unwrap().iter().cloned()
            {
                if ! boot_arg.starts_with("ip=") {
                    boot_args.push(boot_arg);
                }
            }
            firecracker_section.boot_args = Some(boot_args);
        }

        if resume || force_vsock {
            let firecracker_section = vm_config.runtime.as_mut().unwrap()
                .firecracker.as_mut().unwrap();
            if resume {
                firecracker_section.boot_args.as_mut().unwrap()
                    .push("sci_resume=1".to_string());
            }
            if force_vsock {
                firecracker_section.boot_args.as_mut().unwrap()
                    .push("sci_force_vsock=1".to_string());
            }
        }

        let config = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(config_file)
            .unwrap_or_else(|_| panic!("Failed to open {:?}", config_file));
        serde_yaml::to_writer(config, &yaml_config).unwrap();
        Ok(())
    }

    pub fn init_from_file(
        config_file: &Path
    ) -> Result<AppConfig, GenericError> {
        /*!
        Returns an instance of AppConfig by reading and
        deserializing the data from a given yaml configuration
        !*/
        let base_config = std::fs::read_to_string(config_file);
        let mut config_dir = config_file.display().to_string();
        config_dir = config_dir.replace(".yaml", ".d");
        let mut extra_yamls: Vec<_> = std::fs::read_dir(config_dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|x| x.path()).collect();
        extra_yamls.sort();
        let full_yaml: String = base_config.into_iter().chain(
            extra_yamls.into_iter().flat_map(std::fs::read_to_string)
        ).collect();
        let yaml_config: AppConfig =
            serde_yaml::from_str(&full_yaml).expect(
                "Failed to import config file"
            );
        Ok(yaml_config)
    }
}
