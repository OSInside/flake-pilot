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
    tmpfs which is created for the instance. Thus the rootfs on
    the host stays untouched. The data the application writes
    inside of the sandbox does not end up in that tmpfs though:
    bwrap makes the root writable through an overlay of its own,
    whose upper directory is the rw directory on disk. The rw and
    the work directory are kept after the sandbox has ended and
    are reused by the next sandbox of the same instance.

    The setup of an instance named myapp@one, created by the
    user with the ID 1000, looks as follows:

    /var/tmp/bwrap_1000/
       ├── myapp@one_merged   <- overlay mount of the rootfs
       ├── myapp@one_overlay  <- tmpfs with the upper/work dirs
       ├── myapp@one_rw       <- upper dir of the sandbox root (disk)
       └── myapp@one_work     <- work dir of the sandbox root (disk)

    The directories are created here for the user of the sandbox.
    The tmpfs and the overlay are mounted by the helper, see its
    sandbox-mount command, which derives all paths from the user
    and the instance and checks the directories before it mounts.

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

    // 2. Mount the tmpfs and the overlay of the rootfs on them
    let mut call = helper_call("sandbox-mount");
    call.arg("--instance").arg(instance).arg("--rootfs").arg(rootfs);
    run_helper(&mut call)?;
    Ok(get_dir(instance, defaults::OVERLAY_MERGED_NAME))
}

pub fn umount(instance: &str) {
    /*!
    Delete the overlay mount of the rootfs and the tmpfs below it

    The mounts exist as long as the sandbox and are deleted by the
    helper, see its sandbox-umount command. A mount which cannot be
    deleted is reported. It does not stop the pilot, at the time the
    mounts are released the application in the sandbox has
    terminated already
    !*/
    let mut call = helper_call("sandbox-umount");
    call.arg("--instance").arg(instance);
    if let Err(error) = run_helper(&mut call) {
        error!("Failed to release the root filesystem of {instance}: {error}");
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
