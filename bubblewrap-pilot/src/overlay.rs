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

use flakes::error::FlakeError;
use flakes::helper::helper_call;
use flakes::lookup::Lookup;
use flakes::user::{User, chmod, mkdir};

use uzers::get_current_uid;

use std::fs;
use std::process::Command;

pub fn mount(
    instance: &str, rootfs: &str, layers: &[String], user: User
) -> Result<String, FlakeError> {
    /*!
    Provide the root filesystem of the sandbox as overlay mount

    The rootfs of the flake is the lowest, read only layer of an
    overlay filesystem, the given layers are stacked on top of it.
    Its upper and work directory live in a tmpfs which is created
    for the instance. Thus the rootfs on the host stays untouched
    and nothing written in the sandbox ends up on disk. The overlay
    exists as long as the sandbox. The tmpfs is kept for the next
    sandbox of the same instance, which sees what was written
    before, until it is discarded, see umount(), or the host is
    rebooted.

    The setup of an instance named myapp@one, created by the
    user with the ID 1000, looks as follows:

    /var/tmp/bwrap_1000/
       ├── myapp@one_merged   <- overlay mount of the rootfs
       └── myapp@one_overlay  <- tmpfs with the upper/work dirs

    The directories are created here for the user of the sandbox,
    with access for that user only. The tmpfs and the overlay are
    mounted by the helper, see its sandbox-mount command, which
    derives all paths from the user and the instance and checks the
    directories before it mounts.

    The merged directory is returned. It becomes the writable root
    filesystem bwrap creates the sandbox from
    !*/
    // 1. Create the directories of the setup. The user directory is
    //    created first, to not leave its permissions to the umask of
    //    the caller. It closes the directories of all instances to
    //    other users, also those created by former versions with
    //    more permissions
    let user_dir = get_user_dir();
    mkdir(&user_dir, defaults::OVERLAY_DIR_MODE, user)?;
    chmod(&user_dir, defaults::OVERLAY_DIR_MODE, user)?;
    for name in [defaults::OVERLAY_MERGED_NAME, defaults::OVERLAY_TMPFS_NAME] {
        mkdir(&get_dir(instance, name), defaults::OVERLAY_DIR_MODE, user)?;
    }

    // 2. Mount the tmpfs and the overlay of the layers on them
    let mut call = helper_call("sandbox-mount");
    call.arg("--instance").arg(instance).arg("--rootfs").arg(rootfs);
    for layer in layers {
        call.arg("--layer").arg(layer);
    }
    run_helper(&mut call)?;
    Ok(get_dir(instance, defaults::OVERLAY_MERGED_NAME))
}

pub fn umount(instance: &str, discard: bool) {
    /*!
    Delete the overlay mount of the rootfs

    The overlay exists as long as the sandbox and is deleted by
    the helper, see its sandbox-umount command. The tmpfs below it
    is kept for the next sandbox of the instance. With discard it
    is deleted as well, along with everything written in the
    sandboxes of the instance. The writable layer former versions
    kept on disk is deleted in this case too.

    A mount which cannot be deleted is reported. It does not stop
    the pilot, at the time the mounts are released the application
    in the sandbox has terminated already
    !*/
    let mut call = helper_call("sandbox-umount");
    call.arg("--instance").arg(instance);
    if discard {
        call.arg("--discard");
    }
    if let Err(error) = run_helper(&mut call) {
        error!("Failed to release the root filesystem of {instance}: {error}");
    }
    if discard {
        for name in [defaults::OVERLAY_RW_NAME, defaults::OVERLAY_WORK_NAME] {
            let legacy_dir = get_dir(instance, name);
            if let Err(error) = fs::remove_dir_all(&legacy_dir) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    error!("Failed to remove {legacy_dir}: {error}");
                }
            }
        }
    }
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

fn run_helper(call: &mut Command) -> Result<(), FlakeError> {
    /*!
    Run the given helper call, its messages are passed through
    !*/
    if Lookup::is_debug() {
        debug!("{:?} {:?}", call.get_program(), call.get_args());
    }
    let status = call.status()?;
    if ! status.success() {
        return Err(FlakeError::IOError {
            kind: "Helper call failed".to_string(),
            message: format!("{:?} {:?}: {status}", call.get_program(), call.get_args())
        })
    }
    Ok(())
}
