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
//! Access to the podman engine
//!
//! The pilot which runs a container application and flake-ctl
//! which manages the registrations and instances of those
//! applications talk to the same podman setup. This module
//! provides the calls both of them do to this engine.
//!
//! A podman setup exists twice, the system wide one which is
//! used by the flakes running as root and the rootless one of
//! the calling user. All calls therefore take the storage they
//! operate on as their usermode argument.
//!
use std::env;
use std::process::Command;

use uzers::{get_current_uid, get_current_username};

use crate::command::CommandExtTrait;
use crate::config::{
    get_podman_ids_dir, get_podman_storage_conf, read_storage_conf
};
use crate::defaults;
use crate::error::FlakeError;
use crate::flakelog::FlakeLog;
use crate::io::IO;
use crate::user::User;

pub fn setup_podman_call(usermode: bool) -> Command {
    /*!
    Create the podman call for the given storage setup

    All podman calls are done through sudo, either as the
    calling user if the rootless storage is addressed or as
    root for the system wide one. The storage to operate on
    is handed over in the environment of the call
    !*/
    let storage = read_storage_conf(usermode).unwrap();
    let calling_user_name = get_current_username().unwrap();
    let container_runroot = format!(
        "{}/{}",
        storage.get("runroot").unwrap(),
        calling_user_name.to_str().unwrap()
    );
    env::set_var("CONTAINERS_STORAGE_CONF", get_podman_storage_conf(usermode));
    env::set_var("XDG_RUNTIME_DIR", &container_runroot);
    let mut call = Command::new("sudo");
    // Only the variables set above are handed over to the podman
    // call. Passing the complete environment of the caller to a
    // process running as root allows to influence that process in
    // ways the sudo rule for it never intended
    call.arg(format!(
        "--preserve-env={}",
        ["CONTAINERS_STORAGE_CONF", "XDG_RUNTIME_DIR"].join(",")
    ));
    if usermode {
        call.arg("--user").arg(calling_user_name);
    } else {
        call.arg("--user").arg("root");
    }
    call.arg(defaults::PODMAN_PATH);
    call
}

pub fn init_cid_dir(user: User) -> Result<String, FlakeError> {
    /*!
    Create meta data directory structure and return the private
    directory of the calling user to store the CID files in
    !*/
    let usermode = user.get_name() != "root";
    IO::private_dir(&get_podman_ids_dir(usermode), user)
}

pub fn cid_dir(usermode: bool) -> String {
    /*!
    Provide the directory the CID files of the calling user are
    stored in

    In contrast to init_cid_dir the directory is only named, it
    is not created. This is the lookup of the meta data of the
    instances which were created before
    !*/
    format!("{}/{}", get_podman_ids_dir(usermode), get_current_uid())
}

pub fn cid_file(cid_dir: &str, name: &str) -> String {
    /*!
    Provide the path of the CID file of the given instance

    The instance is named after the application it belongs to
    plus the '@NAME' selectors the application was called with
    !*/
    format!("{}/{}.{}", cid_dir, name, defaults::PODMAN_ID_EXTENSION)
}

pub fn container_ids(
    usermode: bool, all: bool
) -> Result<Vec<String>, FlakeError> {
    /*!
    Provide the IDs of the containers podman knows about

    Without all only the containers which are running are
    reported. The IDs are provided the way podman reports them,
    which is abbreviated
    !*/
    let mut call = setup_podman_call(usermode);
    call.arg("ps");
    if all {
        call.arg("--all");
    }
    call.arg("--format").arg("{{.ID}}");
    log_call(&call);
    let output = match call.output() {
        Ok(output) => output,
        Err(error) => {
            return Err(FlakeError::IOError {
                kind: "call failed".to_string(),
                message: format!("{error:?}")
            })
        }
    };
    if ! output.status.success() {
        return Err(FlakeError::IOError {
            kind: "Reading the list of containers failed".to_string(),
            message: String::from_utf8_lossy(&output.stderr).to_string()
        })
    }
    Ok(
        String::from_utf8_lossy(&output.stdout).lines()
            .filter(|cid| ! cid.is_empty())
            .map(|cid| cid.to_string())
            .collect()
    )
}

pub fn is_known_container(cid: &str, container_ids: &[String]) -> bool {
    /*!
    Look up the given container ID in a list of IDs as it is
    provided by container_ids
    !*/
    container_ids.iter()
        .filter(|known_cid| ! known_cid.is_empty())
        // podman reports the container IDs abbreviated, the CID
        // file of an instance holds the complete one
        .any(|known_cid| cid.starts_with(known_cid.as_str()))
}

pub fn container_exists(
    cid: &str, usermode: bool
) -> Result<bool, FlakeError> {
    /*!
    Check if the container with the given ID is known to podman

    A container which was created but is not running is known
    too. If the lookup itself failed no statement about the
    container can be made and none is provided
    !*/
    Ok(is_known_container(cid, &container_ids(usermode, true)?))
}

pub fn container_running(
    cid: &str, usermode: bool
) -> Result<bool, FlakeError> {
    /*!
    Check if the container with the given ID is running
    !*/
    Ok(is_known_container(cid, &container_ids(usermode, false)?))
}

pub fn mount_container(
    container: &str, as_image: bool, usermode: bool
) -> Result<String, FlakeError> {
    /*!
    Mount container and return mount point
    !*/
    let mut call = setup_podman_call(usermode);
    if as_image {
        call.arg("image").arg("mount").arg(container);
    } else {
        call.arg("mount").arg(container);
    }
    log_call(&call);
    let output = call.perform()?;
    Ok(
        String::from_utf8_lossy(&output.stdout)
            .trim_end_matches('\n').to_owned()
    )
}

pub fn umount_container(
    container: &str, as_image: bool, usermode: bool
) -> Result<(), FlakeError> {
    /*!
    Umount container or container image
    !*/
    let mut call = setup_podman_call(usermode);
    if as_image {
        call.arg("image").arg("umount").arg(container);
    } else {
        call.arg("umount").arg(container);
    }
    log_call(&call);
    call.perform()?;
    Ok(())
}

pub fn remove_container(
    container: &str, force: bool, usermode: bool
) -> Result<(), FlakeError> {
    /*!
    Delete the given container instance

    An instance which is still running is only deleted if the
    deletion is forced
    !*/
    let mut call = setup_podman_call(usermode);
    call.arg("rm");
    if force {
        call.arg("--force");
    }
    call.arg(container);
    log_call(&call);
    call.perform()?;
    Ok(())
}

pub fn prune(usermode: bool) {
    /*!
    Delete the container images which are not used anymore

    Errors of the call are only logged but will not cause the
    caller to fail. In the worse case old images doesn't get
    wiped but this should not prevent the caller from doing
    its job
    !*/
    let mut call = setup_podman_call(usermode);
    call.arg("image").arg("prune").arg("--force");
    log_call(&call);
    match call.status() {
        Ok(status) => FlakeLog::debug(&format!("{status:?}")),
        Err(error) => FlakeLog::debug(&format!("{error:?}"))
    }
}

fn log_call(call: &Command) {
    /*!
    Log the given podman call in debug mode
    !*/
    FlakeLog::debug(
        &format!("{:?} {:?}", call.get_program(), call.get_args())
    );
}

#[cfg(test)]
mod tests {
    use super::{cid_file, is_known_container};

    #[test]
    fn test_cid_file() {
        assert_eq!(
            "/tmp/flakes/0/myapp@one.cid",
            cid_file("/tmp/flakes/0", "myapp@one")
        );
    }

    #[test]
    fn test_is_known_container() {
        let container_ids = vec![
            "8c9a3b1d4e5f".to_string(), "1a2b3c4d5e6f".to_string()
        ];
        // podman reports the container IDs abbreviated, the CID
        // file of an instance holds the complete one
        assert!(is_known_container("1a2b3c4d5e6f78901234", &container_ids));
        assert!(! is_known_container("deadbeefcafe12345678", &container_ids));
        // an empty list must not match any container
        assert!(! is_known_container("1a2b3c4d5e6f78901234", &[]));
        assert!(
            ! is_known_container("1a2b3c4d5e6f78901234", &["".to_string()])
        );
    }
}
