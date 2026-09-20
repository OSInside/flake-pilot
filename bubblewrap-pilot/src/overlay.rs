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
use crate::defaults;

use flakes::command::CommandExtTrait;
use flakes::error::FlakeError;
use flakes::lookup::Lookup;
use flakes::user::{User, mkdir};

use uzers::get_current_uid;

use std::process::Command;

pub fn mount(
    instance: &str, rootfs: &str, user: User
) -> Result<String, FlakeError> {
    /*!
    Provide the root filesystem of the sandbox as overlay mount

    The rootfs of the flake is the lower, read only layer of an
    overlay filesystem. Its upper and work directory live in a
    tmpfs which is created for the instance. Thus everything
    written to the root of the sandbox is kept in memory and the
    rootfs on the host stays untouched.

    The setup of an instance named myapp@one, created by the
    user with the ID 1000, looks as follows:

    /var/tmp/bwrap_1000/
       ├── myapp@one_merged   <- overlay mount of the rootfs
       ├── myapp@one_overlay  <- tmpfs with the upper/work dirs
       ├── myapp@one_rw       <- upper dir of the sandbox root
       └── myapp@one_work     <- work dir of the sandbox root

    The merged directory is returned. It becomes the root
    filesystem bwrap creates the sandbox from, whereas the rw and
    the work directory are used by bwrap to make that root
    writable inside of the sandbox
    !*/
    // 1. Create the directories of the setup. The rw and the work
    //    directory are the writable layer of the sandbox root and
    //    are therefore created for the user of the sandbox. The
    //    user directory which keeps them is created first, to not
    //    leave its permissions to the umask of the caller
    mkdir(&get_user_dir(), defaults::OVERLAY_DIR_MODE, user)?;
    for name in [
        defaults::OVERLAY_RW_NAME,
        defaults::OVERLAY_WORK_NAME,
        defaults::OVERLAY_MERGED_NAME,
        defaults::OVERLAY_TMPFS_NAME
    ] {
        mkdir(&get_dir(instance, name), defaults::OVERLAY_DIR_MODE, user)?;
    }

    // 2. Mount the tmpfs which keeps the data written to the root
    let tmpfs_dir = get_dir(instance, defaults::OVERLAY_TMPFS_NAME);
    let mut mount_tmpfs = privileged_call(defaults::MOUNT_TOOL);
    mount_tmpfs.arg("-t").arg(defaults::TMPFS_TYPE)
        .arg(defaults::TMPFS_TYPE)
        .arg(&tmpfs_dir);
    if Lookup::is_debug() {
        debug!("{:?} {:?}", mount_tmpfs.get_program(), mount_tmpfs.get_args());
    }
    mount_tmpfs.perform()?;

    // 3. Create the upper and the work directory of the overlay.
    //    They belong to the tmpfs and can only be created after
    //    it is mounted
    for name in [defaults::OVERLAY_UPPER_NAME, defaults::OVERLAY_WORK_NAME] {
        if let Err(error) = mkdir(
            &format!("{tmpfs_dir}/{name}"),
            defaults::OVERLAY_DIR_MODE, User::ROOT
        ) {
            // The setup is incomplete, its tmpfs must not stay behind
            umount_dir(&tmpfs_dir);
            return Err(error)
        }
    }

    // 4. Mount the rootfs of the flake as overlay
    let merged_dir = get_dir(instance, defaults::OVERLAY_MERGED_NAME);
    let mut mount_overlay = privileged_call(defaults::MOUNT_TOOL);
    mount_overlay.arg("-t").arg(defaults::OVERLAY_TYPE)
        .arg(defaults::OVERLAY_TYPE)
        .arg("-o").arg(get_mount_options(instance, rootfs))
        .arg(&merged_dir);
    if Lookup::is_debug() {
        debug!(
            "{:?} {:?}",
            mount_overlay.get_program(), mount_overlay.get_args()
        );
    }
    if let Err(error) = mount_overlay.perform() {
        umount_dir(&tmpfs_dir);
        return Err(error.into())
    }
    Ok(merged_dir)
}

pub fn umount(instance: &str) {
    /*!
    Delete the overlay mount of the rootfs and the tmpfs below it

    The mounts exist as long as the sandbox. They are deleted in
    the reverse order of their creation, the tmpfs provides the
    upper and the work directory of the overlay and can only be
    released after it
    !*/
    umount_dir(&get_dir(instance, defaults::OVERLAY_MERGED_NAME));
    umount_dir(&get_dir(instance, defaults::OVERLAY_TMPFS_NAME));
}

pub fn get_user_dir() -> String {
    /*!
    Provide the directory which keeps the overlay setups of the
    calling user

    The base directory of the setups is shared between all users
    of the system. Each of them therefore gets a directory of its
    own below it, named after its user ID, e.g /var/tmp/bwrap_1000.
    Without it two users running the same flake would use the
    same paths for their setups
    !*/
    format!(
        "{}/{}_{}",
        defaults::OVERLAY_BASE_DIR,
        defaults::OVERLAY_USER_DIR_NAME,
        get_current_uid()
    )
}

pub fn get_dir(instance: &str, name: &str) -> String {
    /*!
    Provide the path of the given directory of the overlay setup

    All directories of the setup belong to one instance and are
    therefore named after it, e.g /var/tmp/bwrap_1000/myapp@one_merged
    !*/
    format!("{}/{}_{}", get_user_dir(), instance, name)
}

pub fn get_mount_options(instance: &str, rootfs: &str) -> String {
    /*!
    Provide the options of the overlay mount of the rootfs

    The rootfs is the lower layer of the overlay. The upper and
    the work directory are provided by the tmpfs of the instance
    !*/
    let tmpfs_dir = get_dir(instance, defaults::OVERLAY_TMPFS_NAME);
    format!(
        "lowerdir={},upperdir={}/{},workdir={}/{}",
        rootfs,
        tmpfs_dir, defaults::OVERLAY_UPPER_NAME,
        tmpfs_dir, defaults::OVERLAY_WORK_NAME
    )
}

fn umount_dir(mount_point: &str) {
    /*!
    Delete the mount at the given directory

    A mount which cannot be deleted is reported. It does not
    stop the pilot, at the time the mounts are released the
    application in the sandbox has terminated already
    !*/
    let mut call = privileged_call(defaults::UMOUNT_TOOL);
    call.arg(mount_point);
    if Lookup::is_debug() {
        debug!("{:?} {:?}", call.get_program(), call.get_args());
    }
    if let Err(error) = call.perform() {
        error!("Failed to umount {mount_point}: {error}");
    }
}

fn privileged_call(program: &str) -> Command {
    /*!
    Create the call of a program which needs root privileges

    Creating and deleting a mount on the host is not allowed for
    a standard user. Only a caller which is root already can do
    it on its own, every other caller has to pass the call to
    sudo
    !*/
    if User::ROOT.is_calling_user() {
        Command::new(program)
    } else {
        User::ROOT.run(program)
    }
}
