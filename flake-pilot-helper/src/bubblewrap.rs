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
//! bubblewrap flakes with mount points below their root filesystem
//!
//! bubblewrap-pilot cannot overlay a root filesystem with mount
//! points below it in a user namespace, e.g the root of the host.
//! The helper creates such a sandbox from the registration alone.
//! The flake is selected by the user, the rootfs and the layers are
//! taken from the registration. The helper mounts them in the mount
//! namespace of the sandbox, see sandbox.rs, drops its privileges
//! for good and starts bubblewrap-pilot in that namespace as the
//! user. The pilot takes the bwrap options and the application from
//! the registration as well. The arguments of the user are passed
//! to the application only.
//!
//! The registration of a flake registered system wide belongs to
//! root, a sudo rule can grant single applications. The
//! registration of a flake of the user belongs to the user and is
//! read with the identity of the user. It provides nothing the user
//! could not choose anyway: the rootfs and the layers are opened
//! with the identity of the user and the pilot runs with the
//! privileges of the user.
//!
use std::ffi::OsString;
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::Command;

use serde::Deserialize;

use flakes::bubblewrap::split_options;
use flakes::config::get_flakes_dir;
use flakes::defaults;
use flakes::registration;

use crate::caller::Caller;
use crate::sandbox;
use crate::validate;

// The parts of the registration of a bubblewrap flake which make
// up the root filesystem of its sandbox
#[derive(Deserialize)]
struct Registration {
    sandbox: SandboxSection
}

#[derive(Deserialize)]
struct SandboxSection {
    /// Directory tree which is the rootfs of the sandbox
    name: String,

    #[serde(default)]
    runtime: Option<RuntimeSection>
}

#[derive(Deserialize, Default)]
struct RuntimeSection {
    /// bwrap options of the flake, the layers are given with
    /// --overlay-src
    #[serde(default)]
    bubblewrap: Option<Vec<String>>
}

// The part of the flakes configuration of a user which locates the
// registrations of the user
#[derive(Deserialize)]
struct FlakesConfig {
    generic: GenericSection
}

#[derive(Deserialize)]
struct GenericSection {
    /// Directory the registrations of the user are stored in
    #[serde(default)]
    flakes_dir: Option<String>
}

pub fn run_pilot(
    caller: &Caller, app: &str, size: Option<&str>, debug: bool,
    term: Option<&str>, args: &[String]
) -> bool {
    /*!
    Mount the root filesystem of the sandbox of the given flake in a
    new mount namespace and replace the helper by bubblewrap-pilot,
    called as the given application with the given arguments and
    the privileges of the caller

    The tmpfs which keeps the writable layer is of the given size,
    see flakes::tmpfs::get_tmpfs_size(). The pilot gets a fresh
    environment, see pilot_environment(). The variables the bwrap
    options of the flake reference as %VAR resolve to the values of
    that environment, in the layers read by the helper as well as
    in the options read by the pilot
    !*/
    let term = term.filter(|term| match validate::terminal_type(term) {
        Ok(()) => true,
        Err(error) => {
            warn!("{error}, not passed on");
            false
        }
    });
    let environment = pilot_environment(caller, term, debug);
    let (rootfs, layers) = match flake_root(caller, app, &environment) {
        Ok(root) => root,
        Err(error) => {
            error!("{error}");
            return false
        }
    };
    if let Err(error) = sandbox::mount_root(caller, &rootfs, &layers, size) {
        error!("{error}");
        return false
    }
    if let Err(error) = sandbox::drop_privileges(caller) {
        error!("{error}");
        return false
    }
    let mut call = Command::new(defaults::BUBBLEWRAP_PILOT);
    call.arg0(app)
        .args(args)
        .env_clear()
        .envs(environment);
    let error = call.exec();
    error!("Failed to execute {}: {error}", defaults::BUBBLEWRAP_PILOT);
    false
}

fn pilot_environment(
    caller: &Caller, term: Option<&str>, debug: bool
) -> Vec<(String, OsString)> {
    /*!
    Provide the environment of the pilot: the search path of the
    helper, the variables of the caller from the user database, its
    terminal type and the request to only call bwrap, see
    defaults::SANDBOX_MOUNTED_VAR. Nothing of it is picked by the
    caller, the terminal type is validated
    !*/
    let mut environment = vec![
        ("PATH".to_string(), OsString::from(crate::HELPER_PATH)),
        ("HOME".to_string(), caller.home.clone().into_os_string()),
        ("USER".to_string(), OsString::from(&caller.name)),
        ("LOGNAME".to_string(), OsString::from(&caller.name)),
        (defaults::SANDBOX_MOUNTED_VAR.to_string(), OsString::from("1"))
    ];
    if let Some(term) = term {
        environment.push(("TERM".to_string(), OsString::from(term)));
    }
    if debug {
        environment.push(("PILOT_DEBUG".to_string(), OsString::from("1")));
    }
    environment
}

fn flake_root(
    caller: &Caller, app: &str, environment: &[(String, OsString)]
) -> Result<(String, Vec<String>), String> {
    /*!
    Provide the rootfs and the layers of the sandbox of the flake of
    the given application

    Like for the pilot, a system wide registration takes precedence
    over a registration of the caller, see
    flakes::registration::is_usermode(). Anything at the place of
    the system wide registration makes the flake a system wide one,
    which has to pass the checks of validate::system_flake()
    !*/
    let name = validate::flake_name(app)?;
    if fs::symlink_metadata(registration::config_file(&name, false)).is_ok() {
        validate::system_flake(
            app, defaults::BUBBLEWRAP_PILOT, &get_flakes_dir(false)
        )?;
        return registered_root(&name, environment)
    }
    sandbox::as_caller(caller, || user_root(caller, app, environment))
}

fn registered_root(
    name: &str, environment: &[(String, OsString)]
) -> Result<(String, Vec<String>), String> {
    /*!
    Read the rootfs and the layers of the sandbox of the given flake
    from its system wide registration, along with its drop-in files

    The layers are read the way bubblewrap-pilot reads them, with
    the variables of the given environment of the pilot
    !*/
    let yaml = registration::read_config(name, false).ok_or(format!(
        "No system wide flake registration found for: {name}"
    ))?;
    root_from_registration(
        &yaml, &registration::config_file(name, false), environment
    )
}

fn user_root(
    caller: &Caller, app: &str, environment: &[(String, OsString)]
) -> Result<(String, Vec<String>), String> {
    /*!
    Read the rootfs and the layers of the sandbox of the flake of
    the given application from its registration below the flakes
    directory of the caller, along with its drop-in files

    The application, the flakes configuration and the registration
    belong to the caller. They have to be read with the identity of
    the caller, see sandbox::as_caller()
    !*/
    let name = validate::user_flake(app, defaults::BUBBLEWRAP_PILOT)?;
    let flakes_dir = user_flakes_dir(caller)?;
    let config_file = format!("{flakes_dir}/{name}.yaml");
    if let Err(error) = fs::metadata(&config_file) {
        return Err(format!(
            "No flake registration found for: {name}, {config_file}: {error}"
        ))
    }
    let yaml = registration::merge_config(
        &config_file, &[format!("{flakes_dir}/{name}.d")]
    );
    root_from_registration(&yaml, &config_file, environment)
}

fn user_flakes_dir(caller: &Caller) -> Result<String, String> {
    /*!
    Provide the flakes directory of the caller, as configured in the
    flakes configuration in its home directory. Unlike the system
    wide one it has no default, see flakes::config::get_flakes_dir()
    !*/
    let config_file = caller.home.join(defaults::FLAKES_CONFIG_USER);
    let origin = config_file.to_string_lossy().to_string();
    let yaml = fs::read_to_string(&config_file).map_err(
        |error| format!("Failed to read {origin}: {error}")
    )?;
    let config: FlakesConfig = registration::try_config_from_str(&yaml, &origin)?;
    config.generic.flakes_dir.ok_or(format!(
        "No flakes_dir configured in {origin}, run 'flake-ctl init'"
    ))
}

fn root_from_registration(
    yaml: &str, origin: &str, environment: &[(String, OsString)]
) -> Result<(String, Vec<String>), String> {
    /*!
    Provide the rootfs and the layers from the given registration,
    see registered_root()
    !*/
    let registration: Registration = registration::try_config_from_str(
        yaml, origin
    )?;
    let options = registration.sandbox.runtime.unwrap_or_default()
        .bubblewrap.unwrap_or_default();
    let options: Vec<&str> = options.iter().map(String::as_str).collect();
    let (layers, _) = split_options(&options, |var_name| {
        environment.iter()
            .find(|(name, _)| name == var_name)
            .and_then(|(_, value)| value.to_str())
            .map(str::to_string)
    })?;
    Ok((registration.sandbox.name, layers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uzers::get_current_uid;

    #[test]
    fn test_pilot_environment() {
        let caller = Caller::from_uid(get_current_uid()).unwrap();
        let environment = pilot_environment(&caller, Some("vt220"), false);
        let names: Vec<&str> = environment.iter()
            .map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            vec![
                "PATH", "HOME", "USER", "LOGNAME",
                defaults::SANDBOX_MOUNTED_VAR, "TERM"
            ],
            names
        );
    }

    #[test]
    fn test_user_root() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let home = tmp_dir.path();
        let base = home.to_str().unwrap();
        let mut caller = Caller::from_uid(get_current_uid()).unwrap();
        caller.home = home.to_path_buf();
        let environment = vec![
            ("HOME".to_string(), OsString::from(base))
        ];
        let app = format!("{base}/myapp");
        std::os::unix::fs::symlink(defaults::BUBBLEWRAP_PILOT, &app).unwrap();

        // without a flakes configuration of the user there are no
        // registrations of the user
        assert!(user_root(&caller, &app, &environment).is_err());
        fs::create_dir_all(format!("{base}/.config")).unwrap();
        fs::write(
            format!("{base}/{}", defaults::FLAKES_CONFIG_USER),
            "generic:\n  podman_ids_dir: ~\n"
        ).unwrap();
        assert!(user_flakes_dir(&caller).is_err());
        fs::write(
            format!("{base}/{}", defaults::FLAKES_CONFIG_USER),
            format!("generic:\n  flakes_dir: {base}/flakes\n")
        ).unwrap();
        assert_eq!(Ok(format!("{base}/flakes")), user_flakes_dir(&caller));

        // a flake which is not registered
        fs::create_dir_all(format!("{base}/flakes/myapp.d")).unwrap();
        assert!(user_root(&caller, &app, &environment).is_err());

        // the rootfs and the layers, with the drop-in files of the
        // registration
        fs::write(
            format!("{base}/flakes/myapp.yaml"),
            "sandbox:\n  name: /\n  host_app_path: /usr/bin/myapp\n  \
            runtime:\n    bubblewrap:\n      - --overlay-src %HOME/a\n"
        ).unwrap();
        assert_eq!(
            Ok(("/".to_string(), vec![format!("{base}/a")])),
            user_root(&caller, &app, &environment)
        );
        fs::write(
            format!("{base}/flakes/myapp.d/other.yaml"),
            "sandbox:\n  name: /srv/root\n  host_app_path: /usr/bin/myapp\n  \
            runtime:\n    bubblewrap:\n      - --overlay-src %HOME/b\n"
        ).unwrap();
        assert_eq!(
            Ok(("/srv/root".to_string(), vec![format!("{base}/b")])),
            user_root(&caller, &app, &environment)
        );

        // an application which is no link to the pilot
        let other = format!("{base}/other");
        std::os::unix::fs::symlink(defaults::PODMAN_PILOT, &other).unwrap();
        assert!(user_root(&caller, &other, &environment).is_err());
    }

    #[test]
    fn test_root_from_registration() {
        let environment = vec![
            ("HOME".to_string(), OsString::from("/home/tux"))
        ];
        // the layers are taken from the --overlay-src options, with
        // the variables of the environment of the pilot only
        let (rootfs, layers) = root_from_registration(
            "sandbox:\n  name: /\n  host_app_path: /usr/bin/aws\n  \
            runtime:\n    bubblewrap:\n      - --overlay-src /usr/share/a\n      \
            - --bind /home /home\n      - --overlay-src %HOME/b\n      \
            - --overlay-src %OTHER/c\n",
            "aws.yaml", &environment
        ).unwrap();
        assert_eq!("/", rootfs);
        assert_eq!(vec!["/usr/share/a", "/home/tux/b", "$OTHER/c"], layers);
        // without bwrap options there are no layers
        let (_, layers) = root_from_registration(
            "sandbox:\n  name: /srv/root\n  host_app_path: /usr/bin/x\n",
            "x.yaml", &environment
        ).unwrap();
        assert!(layers.is_empty());
        assert!(root_from_registration("sandbox: {}\n", "x.yaml", &environment)
            .is_err());
    }
}
