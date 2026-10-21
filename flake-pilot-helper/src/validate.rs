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
//! Validation of the arguments of the helper commands
//!
//! The arguments come from a caller who is not trusted. Names are
//! checked against a strict pattern before they become part of a
//! path, a device name or a program argument. Paths have to be
//! absolute and free of characters which change their meaning in
//! the files the helper writes them to.
//!
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use flakes::config::get_flakes_dir;
use flakes::defaults::IFNAMSIZ;
use flakes::lookup::Lookup;

use crate::caller::Caller;

// Max. length of a name given to the helper
const NAME_MAX_LEN: usize = 128;

pub fn absolute_path(path: &str) -> Result<(), String> {
    /*!
    Check that the given path is absolute and free of control
    characters. A path with a component of '..' is refused, its
    meaning depends on symbolic links on the way
    !*/
    if ! path.starts_with('/') {
        return Err(format!("{path:?} is not an absolute path"))
    }
    if path.chars().any(char::is_control) {
        return Err(format!("{path:?} contains control characters"))
    }
    if path.split('/').any(|component| component == "..") {
        return Err(format!("{path:?} contains a '..' component"))
    }
    Ok(())
}

pub fn application_name(name: &str) -> Result<(), String> {
    /*!
    Check the name of a flake application, the basename of the
    application on the host. It becomes part of device and file
    names and has to start with a letter or a digit
    !*/
    let mut chars = name.chars();
    let valid = name.len() <= NAME_MAX_LEN
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c));
    if ! valid {
        return Err(format!(
            "Invalid application name {name:?}: expected up to \
            {NAME_MAX_LEN} characters out of [A-Za-z0-9._+-], starting \
            with [A-Za-z0-9]"
        ))
    }
    Ok(())
}

pub fn instance_selector(instance: &str) -> Result<String, String> {
    /*!
    Check the @NAME instance selector of an application and
    provide it in its '@NAME' notation. For convenience the plain
    name is accepted as well
    !*/
    let instance = if instance.starts_with('@') {
        instance.to_string()
    } else {
        format!("@{instance}")
    };
    if ! Lookup::is_safe_instance_name(&instance) {
        return Err(format!(
            "Invalid instance name {instance:?}: expected up to 64 \
            characters out of [A-Za-z0-9@=_-]"
        ))
    }
    Ok(instance)
}

pub fn interface_name(name: &str) -> Result<(), String> {
    /*!
    Check the name of a network interface of the host
    !*/
    let valid = ! name.is_empty() && name.len() < IFNAMSIZ
        && name != "." && name != ".."
        && name.chars().all(
            |c| c.is_ascii_alphanumeric() || "._-".contains(c)
        );
    if ! valid {
        return Err(format!("Invalid interface name {name:?}"))
    }
    Ok(())
}

pub fn terminal_type(term: &str) -> Result<(), String> {
    /*!
    Check the name of a terminal type, the value of TERM
    !*/
    let valid = ! term.is_empty() && term.len() <= 64
        && term.chars().all(
            |c| c.is_ascii_alphanumeric() || "._+-".contains(c)
        );
    if ! valid {
        return Err(format!("Invalid terminal type {term:?}"))
    }
    Ok(())
}

// FlakeConfig is the flake configuration of an application the
// caller may act on
#[derive(Debug, PartialEq)]
pub struct FlakeConfig {
    /// Path of the configuration file
    pub path: String,
    /// Name of the application, the configuration is named after it
    pub name: String,
    /// ID of the user the configuration belongs to
    pub owner: u32
}

impl FlakeConfig {
    pub fn is_system(&self) -> bool {
        /*!
        Check if the flake is registered system wide. Only root can
        register such a flake, its configuration belongs to root
        !*/
        self.owner == 0
    }

    pub fn meta_name(&self, instance: Option<&str>) -> String {
        /*!
        Provide the name of the given instance of the application,
        the name followed by its @NAME selector
        !*/
        format!("{}{}", self.name, instance.unwrap_or_default())
    }
}

pub fn flake_config(
    caller: &Caller, config_file: &str
) -> Result<FlakeConfig, String> {
    flake_config_in(caller, config_file, &get_flakes_dir(false))
}

pub fn flake_config_in(
    caller: &Caller, config_file: &str, system_flakes_dir: &str
) -> Result<FlakeConfig, String> {
    /*!
    Check the given flake configuration file, see flake_config()

    The configuration identifies the flake an address, a TAP
    device or an NFS export belongs to. A caller may only act on a
    configuration of its own, root on every configuration. A
    configuration which belongs to root has to be part of the system
    wide registry, which every user may use. The file itself must
    not be a symbolic link
    !*/
    absolute_path(config_file)?;
    let path = Path::new(config_file);
    let name = match (path.extension(), path.file_stem()) {
        (Some(extension), Some(stem)) if extension == "yaml" => {
            stem.to_str().unwrap_or_default()
        },
        _ => return Err(format!("{config_file} is no flake configuration"))
    };
    application_name(name)?;
    let metadata = fs::symlink_metadata(path).map_err(
        |error| format!("Failed to access {config_file}: {error}")
    )?;
    if ! metadata.is_file() {
        return Err(format!("{config_file} is not a regular file"))
    }
    let owner = metadata.uid();
    if owner == 0 {
        if path.parent() != Some(Path::new(system_flakes_dir)) {
            return Err(format!(
                "{config_file} belongs to root but is not part of the \
                system wide registry {system_flakes_dir}"
            ))
        }
    } else if ! caller.may_act_for(owner) {
        return Err(format!(
            "{config_file} does not belong to the calling user"
        ))
    }
    Ok(FlakeConfig {
        path: config_file.to_string(), name: name.to_string(), owner
    })
}

pub fn vm_user(
    caller: &Caller, flake: &FlakeConfig, uid: Option<u32>
) -> Result<u32, String> {
    /*!
    Provide the ID of the user who runs the VM of the given flake

    This is the caller unless another user is given, which only
    root may do. A flake registered by a user is only run by that
    user, a system wide flake by any user
    !*/
    let uid = uid.unwrap_or(caller.uid);
    if ! caller.may_act_for(uid) {
        return Err(
            "Only root can set up the VM of another user".to_string()
        )
    }
    if ! flake.is_system() && uid != flake.owner {
        return Err(format!(
            "{} is registered by the user ID {} and only run by that user",
            flake.path, flake.owner
        ))
    }
    Ok(uid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use uzers::get_current_uid;

    fn caller(uid: u32) -> Caller {
        let mut caller = Caller::from_uid(get_current_uid()).unwrap();
        caller.uid = uid;
        caller
    }

    #[test]
    fn test_absolute_path() {
        assert!(absolute_path("/home/tux/work dir").is_ok());
        assert!(absolute_path("relative/path").is_err());
        assert!(absolute_path("/home/tux/../root").is_err());
        assert!(absolute_path("/home/tux\nother").is_err());
    }

    #[test]
    fn test_application_name() {
        for name in ["myapp", "g++", "python3.11", "aws-cli", "x_y"] {
            assert!(application_name(name).is_ok(), "{}", name);
        }
        for name in ["", ".hidden", "-opt", "a/b", "a b", "my@app", "näme"] {
            assert!(application_name(name).is_err(), "{:?}", name);
        }
    }

    #[test]
    fn test_instance_selector() {
        assert_eq!(Ok("@one".to_string()), instance_selector("one"));
        assert_eq!(Ok("@one".to_string()), instance_selector("@one"));
        assert!(instance_selector("@one/../x").is_err());
        assert!(instance_selector("@one two").is_err());
    }

    #[test]
    fn test_interface_name() {
        assert!(interface_name("eth0").is_ok());
        assert!(interface_name("enp0s31f6").is_ok());
        assert!(interface_name("").is_err());
        assert!(interface_name("..").is_err());
        assert!(interface_name("eth0/../lo").is_err());
        assert!(interface_name("averyverylongname").is_err());
    }

    #[test]
    fn test_terminal_type() {
        assert!(terminal_type("xterm-256color").is_ok());
        assert!(terminal_type("").is_err());
        assert!(terminal_type("xterm;rm").is_err());
    }

    #[test]
    fn test_flake_config() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let base = tmp_dir.path().to_str().unwrap();
        let config_file = format!("{base}/myapp.yaml");
        fs::write(&config_file, "").unwrap();
        let uid = get_current_uid();
        // a configuration of root is only accepted in the system
        // wide registry, which is the temporary directory for root
        let registry = if uid == 0 { base } else { "/none" };

        // a configuration of the caller
        let flake = flake_config_in(&caller(uid), &config_file, registry);
        assert_eq!(
            Ok(FlakeConfig {
                path: config_file.clone(), name: "myapp".to_string(),
                owner: uid
            }),
            flake
        );
        assert_eq!("myapp@one", flake.unwrap().meta_name(Some("@one")));

        if uid != 0 {
            // a configuration of another user is refused, root may
            // act on it
            assert!(
                flake_config_in(&caller(uid + 1), &config_file, "/none")
                    .is_err()
            );
            assert!(flake_config_in(&caller(0), &config_file, "/none").is_ok());
        }

        // a symbolic link, a missing file and a file which is no
        // flake configuration are refused
        let link = format!("{base}/link.yaml");
        symlink(&config_file, &link).unwrap();
        assert!(flake_config_in(&caller(uid), &link, registry).is_err());
        assert!(
            flake_config_in(&caller(uid), &format!("{base}/x.yaml"), registry)
                .is_err()
        );
        fs::write(format!("{base}/myapp.conf"), "").unwrap();
        assert!(
            flake_config_in(
                &caller(uid), &format!("{base}/myapp.conf"), registry
            ).is_err()
        );
        // a configuration of root outside of the system wide
        // registry is refused
        if uid == 0 {
            assert!(
                flake_config_in(&caller(0), &config_file, "/none").is_err()
            );
        }
    }

    #[test]
    fn test_vm_user() {
        let user_flake = FlakeConfig {
            path: "/home/tux/.config/flakes/myapp.yaml".to_string(),
            name: "myapp".to_string(), owner: 1000
        };
        let system_flake = FlakeConfig {
            path: "/usr/share/flakes/myapp.yaml".to_string(),
            name: "myapp".to_string(), owner: 0
        };
        assert_eq!(Ok(1000), vm_user(&caller(1000), &user_flake, None));
        assert_eq!(Ok(1000), vm_user(&caller(1000), &system_flake, None));
        // only root names another user
        assert!(vm_user(&caller(1000), &system_flake, Some(1001)).is_err());
        assert_eq!(
            Ok(1001), vm_user(&caller(0), &system_flake, Some(1001))
        );
        // a flake of a user is only run by that user
        assert!(vm_user(&caller(0), &user_flake, Some(1001)).is_err());
        assert!(vm_user(&caller(0), &user_flake, None).is_err());
    }
}
