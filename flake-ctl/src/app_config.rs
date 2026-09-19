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
use flakes::registration;
use crate::defaults;
use crate::firecracker;

type GenericError = Box<dyn std::error::Error + Send + Sync + 'static>;

// AppConfig represents application yaml configuration
#[derive(Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub include: AppInclude,
    pub container: Option<AppContainer>,
    pub vm: Option<AppFireCracker>,
    pub sandbox: Option<AppSandbox>,
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
pub struct AppSandbox {
    pub name: String,
    pub target_app_path: String,
    pub host_app_path: String,
    pub runtime: Option<AppSandboxRuntime>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct AppSandboxRuntime {
    pub runas: Option<String>,
    pub pilot_options: Option<Vec<String>>,
    pub bubblewrap: Option<Vec<String>>,
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

fn normalize_engine_options(opts: &[String]) -> Vec<String> {
    /*!
    Provide the engine options as they are passed to the engine

    An option is allowed to be escaped with a leading backslash.
    This is needed to pass options which would otherwise be
    eaten by the argument parser of flake-ctl
    !*/
    opts.iter().map(
        |opt| opt.strip_prefix('\\').unwrap_or(opt).to_string()
    ).collect()
}

impl AppInclude {
    fn set(&mut self, tar: Option<Vec<String>>, path: Option<Vec<String>>) {
        /*!
        Set the data to sync into the instance of the flake
        !*/
        if tar.is_some() {
            self.tar = tar;
        }
        if path.is_some() {
            self.path = path;
        }
    }
}

impl AppConfig {
    fn from_template(template_file: &str) -> AppConfig {
        /*!
        Read the registration template of an engine

        Every registration is created from a template which
        provides the defaults of the engine it belongs to
        !*/
        let template = std::fs::File::open(template_file)
            .unwrap_or_else(|_| panic!("Failed to open {}", template_file));
        serde_yaml::from_reader(template).expect(
            "Failed to import config template"
        )
    }

    pub fn from_file(config_file: &Path) -> Result<AppConfig, GenericError> {
        /*!
        Returns an instance of AppConfig by reading and
        deserializing the given yaml configuration

        Only the configuration file itself is read. The optional
        drop-in files from the '.d' directory next to it are not
        merged in, which makes the result safe to write back
        !*/
        Ok(serde_yaml::from_str(&std::fs::read_to_string(config_file)?)?)
    }

    pub fn to_file(&self, config_file: &Path) -> Result<(), GenericError> {
        /*!
        Store the configuration to the given yaml file
        !*/
        Ok(serde_yaml::to_writer(std::fs::File::create(config_file)?, self)?)
    }

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
        let mut yaml_config = AppConfig::from_template(
            defaults::FLAKE_TEMPLATE_CONTAINER
        );
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
        if let Some(opts) = &opts {
            container_config.runtime.as_mut().unwrap().podman = Some(
                normalize_engine_options(opts)
            );
        }
        if let Some(pilot_options) = &pilot_options {
            container_config.runtime.as_mut().unwrap().pilot_options = Some(
                normalize_pilot_options(pilot_options)
            );
        }
        yaml_config.include.set(includes_tar, includes_path);

        yaml_config.to_file(config_file)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_sandbox(
        config_file: &Path,
        rootfs: &str,
        target_app_path: &str,
        host_app_path: &str,
        run_as: Option<&String>,
        opts: Option<Vec<String>>,
        pilot_options: Option<Vec<String>>,
    ) -> Result<(), GenericError> {
        /*!
        save stores an AppConfig to the given file
        !*/
        let mut yaml_config = AppConfig::from_template(
            defaults::FLAKE_TEMPLATE_BUBBLEWRAP
        );
        let sandbox_config = yaml_config.sandbox.as_mut().unwrap();

        sandbox_config.name = rootfs.to_string();
        sandbox_config.target_app_path = target_app_path.to_string();
        sandbox_config.host_app_path = host_app_path.to_string();

        if let Some(run_as) = run_as {
            sandbox_config.runtime.as_mut().unwrap()
                .runas = Some(run_as.to_string());
        }
        if let Some(pilot_options) = &pilot_options {
            sandbox_config.runtime.as_mut().unwrap().pilot_options = Some(
                normalize_pilot_options(pilot_options)
            );
        }
        // Custom sandbox options are added to the options provided
        // by the template. Unlike other engines the options of the
        // sandbox are mostly mount specifications which adds up to
        // the standard setup of the sandbox
        if let Some(opts) = &opts {
            let runtime = sandbox_config.runtime.as_mut().unwrap();
            let mut final_opts: Vec<String> = runtime.bubblewrap
                .as_ref().cloned().unwrap_or_default();
            final_opts.extend(normalize_engine_options(opts));
            runtime.bubblewrap = Some(final_opts);
        }

        yaml_config.to_file(config_file)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_vm(
        config_file: &Path,
        vm: &String,
        target_app_path: &str,
        host_app_path: &String,
        run_as: Option<&String>,
        overlay_size: Option<&String>,
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
        let mut yaml_config = AppConfig::from_template(
            defaults::FLAKE_TEMPLATE_FIRECRACKER
        );
        let vm_config = yaml_config.vm.as_mut().unwrap();

        vm_config.name = vm.to_string();
        vm_config.target_app_path = target_app_path.to_string();
        vm_config.host_app_path = host_app_path.to_string();

        let runtime = vm_config.runtime.as_mut().unwrap();
        if resume {
            runtime.resume = Some(resume);
        }
        if force_vsock {
            runtime.force_vsock = Some(force_vsock);
        }
        if let Some(run_as) = run_as {
            runtime.runas = Some(run_as.to_string());
        }
        if let Some(pilot_options) = &pilot_options {
            runtime.pilot_options = Some(
                normalize_pilot_options(pilot_options)
            );
        }

        let firecracker_section = runtime.firecracker.as_mut().unwrap();
        if let Some(overlay_size) = overlay_size {
            firecracker_section.overlay_size = Some(overlay_size.to_string());
        }
        let rootfs_image_path = format!(
            "{}/{}", image_dir, defaults::FIRECRACKER_ROOTFS_NAME
        );
        if ! Path::new(&rootfs_image_path).exists() {
            return Err(
                Box::new(Error::new(
                    ErrorKind::NotFound,
                    format!("No rootfs image found: {rootfs_image_path}")
                ))
            )
        }
        firecracker_section.rootfs_image_path = Some(rootfs_image_path);

        let kernel_image_path = format!(
            "{}/{}", image_dir, defaults::FIRECRACKER_KERNEL_NAME
        );
        if ! Path::new(&kernel_image_path).exists() {
            return Err(
                Box::new(Error::new(
                    ErrorKind::NotFound,
                    format!("No kernel image found: {kernel_image_path}")
                ))
            )
        }
        firecracker_section.kernel_image_path = Some(kernel_image_path);

        let initrd_path = format!(
            "{}/{}", image_dir, defaults::FIRECRACKER_INITRD_NAME
        );
        if Path::new(&initrd_path).exists() {
            firecracker_section.initrd_path = Some(initrd_path);
        }

        // The registration creates no network setup. The 'ip=' option
        // is deleted from the kernel commandline of the VM. The setup
        // can be created later on with 'flake-ctl firecracker network add'
        let boot_args = firecracker_section.boot_args.as_mut().unwrap();
        boot_args.retain(|boot_arg| ! boot_arg.starts_with("ip="));
        if resume {
            boot_args.push("sci_resume=1".to_string());
        }
        if force_vsock {
            boot_args.push("sci_force_vsock=1".to_string());
        }
        yaml_config.include.set(includes_tar, includes_path);

        yaml_config.to_file(config_file)
    }

    pub fn init_from_file(
        config_file: &Path
    ) -> Result<AppConfig, GenericError> {
        /*!
        Returns an instance of AppConfig by reading and
        deserializing the data from a given yaml configuration

        The optional drop-in files from the '.d' directory next
        to the configuration are merged in
        !*/
        let base_file = config_file.display().to_string();
        let full_yaml = registration::merge_config(
            &base_file, &base_file.replace(".yaml", ".d")
        );
        let yaml_config: AppConfig =
            serde_yaml::from_str(&full_yaml).expect(
                "Failed to import config file"
            );
        Ok(yaml_config)
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    fn read_template(name: &str) -> AppConfig {
        AppConfig::from_template(&format!(
            "{}/template/{}", env!("CARGO_MANIFEST_DIR"), name
        ))
    }

    #[test]
    fn test_flake_templates() {
        // Every registration is created from a template. They
        // have to match with the app config model, a mismatch
        // would let the register command panic
        assert!(read_template("container-flake.yaml").container.is_some());
        assert!(read_template("firecracker-flake.yaml").vm.is_some());
        let sandbox = read_template("bubblewrap-flake.yaml").sandbox.unwrap();
        let runtime = sandbox.runtime.unwrap();
        assert_eq!(Some("any".to_string()), runtime.runas);
        assert_eq!(5, runtime.bubblewrap.unwrap().len());
    }
}
