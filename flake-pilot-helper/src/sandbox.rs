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
//! The root filesystem of the bubblewrap sandboxes
//!
//! A sandbox runs on an overlay of a directory tree of the host,
//! see bubblewrap-pilot. The setup of an instance lives in the
//! directory of its user below OVERLAY_BASE_DIR, e.g
//!
//! /var/tmp/bwrap_1000/
//!    ├── myapp@one_merged   <- overlay mount of the rootfs
//!    └── myapp@one_overlay  <- tmpfs with the upper/work dirs
//!
//! The directories belong to the user, the mounts can only be
//! created by root. A user can rename and replace the entries of
//! a directory of its own at any time. Every directory is therefore
//! opened without following a symbolic link and its owner is
//! checked on the open descriptor. The mounts are created detached
//! with the file descriptor based mount API, prepared, and then
//! attached to these descriptors. No path is resolved twice.
//!
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use flakes::defaults::{
    OVERLAY_BASE_DIR, OVERLAY_USER_DIR_NAME, OVERLAY_MERGED_NAME,
    OVERLAY_TMPFS_NAME, OVERLAY_UPPER_NAME, OVERLAY_WORK_NAME
};

use crate::caller::Caller;
use crate::validate;

// Constants of the file descriptor based mount API, linux/mount.h
const FSOPEN_CLOEXEC: libc::c_uint = 0x1;
const FSCONFIG_SET_STRING: libc::c_uint = 1;
const FSCONFIG_CMD_CREATE: libc::c_uint = 6;
const FSMOUNT_CLOEXEC: libc::c_uint = 0x1;
const MOUNT_ATTR_NOSUID: libc::c_uint = 0x2;
const MOUNT_ATTR_NODEV: libc::c_uint = 0x4;
const MOVE_MOUNT_F_EMPTY_PATH: libc::c_uint = 0x4;
const MOVE_MOUNT_T_EMPTY_PATH: libc::c_uint = 0x40;

// Filesystem types of the setup and their magic numbers as
// reported by statfs(2)
const TMPFS_TYPE: &str = "tmpfs";
const TMPFS_MAGIC: i64 = 0x0102_1994;
const OVERLAY_TYPE: &str = "overlay";
const OVERLAY_MAGIC: i64 = 0x794c_7630;

// Permissions of the upper and the work directory of the overlay
const OVERLAY_DIR_MODE: libc::mode_t = 0o755;

pub fn mount(caller: &Caller, instance: &str, rootfs: &str) -> bool {
    /*!
    Create the overlay of the given rootfs for the given sandbox
    instance of the caller

    The rootfs is the lower, read only layer. The upper and the
    work directory live in a tmpfs which belongs to the caller.
    Both mounts are created with nosuid and nodev. The rootfs is
    only read: access to its content through the overlay is checked
    against its own permissions, the overlay grants no access the
    caller does not have on the rootfs itself
    !*/
    match mount_overlay(caller, instance, rootfs) {
        Ok(()) => true,
        Err(error) => {
            error!("{error}");
            false
        }
    }
}

pub fn umount(caller: &Caller, instance: &str) -> bool {
    /*!
    Delete the mounts of the given sandbox instance of the caller

    The overlay is deleted first, the tmpfs provides its upper and
    work directory. A directory which is no mount of the expected
    type is left alone
    !*/
    let result = validate::sandbox_instance(instance)
        .and_then(|_| open_user_dir(caller))
        .map(|user_dir| release_mounts(&user_dir, instance));
    match result {
        Ok(ok) => ok,
        Err(error) => {
            error!("{error}");
            false
        }
    }
}

fn mount_overlay(
    caller: &Caller, instance: &str, rootfs: &str
) -> Result<(), String> {
    validate::sandbox_instance(instance)?;
    validate::absolute_path(rootfs)?;
    let rootfs_dir = open_dir(libc::AT_FDCWD, rootfs, true).map_err(
        |error| format!("Failed to open rootfs {rootfs}: {error}")
    )?;
    let user_dir = open_user_dir(caller)?;

    // A setup left behind by a sandbox which did not end properly,
    // e.g because its pilot got killed, is released first. There is
    // only one sandbox per instance
    release_mounts(&user_dir, instance);
    let tmpfs_dir = open_setup_dir(
        &user_dir, caller, instance, OVERLAY_TMPFS_NAME
    )?;
    let merged_dir = open_setup_dir(
        &user_dir, caller, instance, OVERLAY_MERGED_NAME
    )?;

    // 1. The tmpfs, with the upper and the work directory created
    //    before it becomes visible
    let tmpfs = create_mount(TMPFS_TYPE, &[
        ("mode", "0700"),
        ("uid", &caller.uid.to_string()),
        ("gid", &caller.gid.to_string())
    ]).map_err(|error| format!("Failed to create the tmpfs: {error}"))?;
    for name in [OVERLAY_UPPER_NAME, OVERLAY_WORK_NAME] {
        make_dir_for(&tmpfs, name, caller).map_err(
            |error| format!("Failed to create {name} in the tmpfs: {error}")
        )?;
    }
    attach_mount(&tmpfs, &tmpfs_dir).map_err(
        |error| format!("Failed to mount the tmpfs of {instance}: {error}")
    )?;

    // 2. The overlay of the rootfs. All of its directories are
    //    given through the descriptors opened above
    let overlay = create_mount(OVERLAY_TYPE, &[
        ("lowerdir", &fd_path(&rootfs_dir)),
        ("upperdir", &format!("{}/{OVERLAY_UPPER_NAME}", fd_path(&tmpfs))),
        ("workdir", &format!("{}/{OVERLAY_WORK_NAME}", fd_path(&tmpfs)))
    ]).and_then(|overlay| attach_mount(&overlay, &merged_dir));
    if let Err(error) = overlay {
        detach_mount(&tmpfs);
        return Err(format!(
            "Failed to mount the overlay of {rootfs} for {instance}: {error}"
        ))
    }
    Ok(())
}

fn release_mounts(user_dir: &OwnedFd, instance: &str) -> bool {
    /*!
    Detach the overlay and the tmpfs of the given instance if they
    are mounted. A setup directory which is no mount root of the
    expected type is left alone
    !*/
    let mut ok = true;
    for (name, magic) in [
        (OVERLAY_MERGED_NAME, OVERLAY_MAGIC), (OVERLAY_TMPFS_NAME, TMPFS_MAGIC)
    ] {
        let entry = format!("{instance}_{name}");
        let directory = match open_dir(user_dir.as_raw_fd(), &entry, false) {
            Ok(directory) => directory,
            // Nothing the helper mounts, which is left to the checks
            // of the setup directories
            Err(error) if matches!(
                error.raw_os_error(),
                Some(libc::ENOENT | libc::ENOTDIR | libc::ELOOP)
            ) => continue,
            Err(error) => {
                error!("Failed to open {entry}: {error}");
                ok = false;
                continue
            }
        };
        match is_mount_root_of(user_dir, &directory, magic) {
            Ok(true) => {
                info!("Releasing {name} mount of {instance}");
                ok &= detach_mount(&directory)
            },
            Ok(false) => { },
            Err(error) => {
                error!("Failed to inspect {entry}: {error}");
                ok = false
            }
        }
    }
    ok
}

fn open_user_dir(caller: &Caller) -> Result<OwnedFd, String> {
    /*!
    Open the directory which keeps the sandbox setups of the caller

    The base directory and all of its parents have to belong to
    root. The directory of the caller has to belong to the caller
    and must not be writable for others, nobody else may replace
    the setup directories in it
    !*/
    let base_dir = open_root_owned_dir(OVERLAY_BASE_DIR)?;
    let name = format!("{OVERLAY_USER_DIR_NAME}_{}", caller.uid);
    let user_dir = open_dir(base_dir.as_raw_fd(), &name, false).map_err(
        |error| format!("Failed to open {OVERLAY_BASE_DIR}/{name}: {error}")
    )?;
    let attributes = fstat(&user_dir).map_err(|error| error.to_string())?;
    if attributes.st_uid != caller.uid || attributes.st_mode & 0o022 != 0 {
        return Err(format!(
            "{OVERLAY_BASE_DIR}/{name} has to belong to the caller and \
            must not be writable for others"
        ))
    }
    Ok(user_dir)
}

fn open_root_owned_dir(path: &str) -> Result<OwnedFd, String> {
    /*!
    Open the given absolute directory, every component of which has
    to be a directory owned by root, not a symbolic link
    !*/
    let mut directory = open_dir(libc::AT_FDCWD, "/", false).map_err(
        |error| format!("Failed to open /: {error}")
    )?;
    for component in path.split('/').filter(|component| ! component.is_empty()) {
        directory = open_dir(directory.as_raw_fd(), component, false).map_err(
            |error| format!("Failed to open {component} of {path}: {error}")
        )?;
        let attributes = fstat(&directory).map_err(|error| error.to_string())?;
        if attributes.st_uid != 0 {
            return Err(format!("{component} of {path} does not belong to root"))
        }
    }
    Ok(directory)
}

fn open_setup_dir(
    user_dir: &OwnedFd, caller: &Caller, instance: &str, name: &str
) -> Result<OwnedFd, String> {
    /*!
    Open the given setup directory of the instance to mount on. It
    has to belong to the caller and must not be a mount already
    !*/
    let entry = format!("{instance}_{name}");
    let directory = open_dir(user_dir.as_raw_fd(), &entry, false).map_err(
        |error| match error.raw_os_error() {
            Some(libc::ENOTDIR | libc::ELOOP) => format!(
                "{entry} is not a directory, a symbolic link is not followed"
            ),
            _ => format!("Failed to open {entry}: {error}")
        }
    )?;
    let attributes = fstat(&directory).map_err(|error| error.to_string())?;
    let user_dir_attributes = fstat(user_dir).map_err(|error| error.to_string())?;
    if attributes.st_uid != caller.uid {
        return Err(format!("{entry} does not belong to the caller"))
    }
    if attributes.st_dev != user_dir_attributes.st_dev {
        return Err(format!("{entry} is mounted already"))
    }
    Ok(directory)
}

fn is_mount_root_of(
    user_dir: &OwnedFd, directory: &OwnedFd, magic: i64
) -> io::Result<bool> {
    /*!
    Check if the given directory below the user directory is the
    root of a mount of the filesystem type of the given magic
    number. The lookup of a mount point provides the root of the
    filesystem mounted on it, which lives on another device
    !*/
    if fstat(directory)?.st_dev == fstat(user_dir)?.st_dev {
        return Ok(false)
    }
    let mut attributes: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatfs(directory.as_raw_fd(), &mut attributes) } != 0 {
        return Err(io::Error::last_os_error())
    }
    // The type of f_type differs between the architectures
    #[allow(clippy::unnecessary_cast)]
    let filesystem_type = attributes.f_type as i64;
    Ok(filesystem_type == magic)
}

fn open_dir(dirfd: RawFd, name: &str, follow: bool) -> io::Result<OwnedFd> {
    /*!
    Open the given directory relative to dirfd as O_PATH descriptor.
    Without follow a symbolic link is refused
    !*/
    let name = to_c_string(name)?;
    let mut flags = libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC;
    if ! follow {
        flags |= libc::O_NOFOLLOW;
    }
    let fd = unsafe { libc::openat(dirfd, name.as_ptr(), flags) };
    if fd < 0 {
        return Err(io::Error::last_os_error())
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn fstat(descriptor: &OwnedFd) -> io::Result<libc::stat> {
    let mut attributes: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(descriptor.as_raw_fd(), &mut attributes) } != 0 {
        return Err(io::Error::last_os_error())
    }
    Ok(attributes)
}

fn make_dir_for(
    directory: &OwnedFd, name: &str, caller: &Caller
) -> io::Result<()> {
    /*!
    Create the given directory below the given directory and hand
    it over to the caller
    !*/
    let name = to_c_string(name)?;
    let fd = directory.as_raw_fd();
    if unsafe { libc::mkdirat(fd, name.as_ptr(), OVERLAY_DIR_MODE) } != 0 {
        return Err(io::Error::last_os_error())
    }
    if unsafe {
        libc::fchownat(
            fd, name.as_ptr(), caller.uid, caller.gid,
            libc::AT_SYMLINK_NOFOLLOW
        )
    } != 0 {
        return Err(io::Error::last_os_error())
    }
    Ok(())
}

fn create_mount(fstype: &str, options: &[(&str, &str)]) -> io::Result<OwnedFd> {
    /*!
    Create a detached mount of the given filesystem type with the
    given options, mounted nosuid and nodev. The descriptor of its
    root is returned
    !*/
    let fstype = to_c_string(fstype)?;
    let context = syscall_fd(unsafe {
        libc::syscall(libc::SYS_fsopen, fstype.as_ptr(), FSOPEN_CLOEXEC)
    })?;
    for (key, value) in options {
        let key = to_c_string(key)?;
        let value = to_c_string(value)?;
        syscall_ok(unsafe {
            libc::syscall(
                libc::SYS_fsconfig, context.as_raw_fd(), FSCONFIG_SET_STRING,
                key.as_ptr(), value.as_ptr(), 0
            )
        })?;
    }
    syscall_ok(unsafe {
        libc::syscall(
            libc::SYS_fsconfig, context.as_raw_fd(), FSCONFIG_CMD_CREATE,
            std::ptr::null::<libc::c_char>(),
            std::ptr::null::<libc::c_char>(), 0
        )
    })?;
    syscall_fd(unsafe {
        libc::syscall(
            libc::SYS_fsmount, context.as_raw_fd(), FSMOUNT_CLOEXEC,
            MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV
        )
    })
}

fn attach_mount(mount: &OwnedFd, target: &OwnedFd) -> io::Result<()> {
    /*!
    Attach the given detached mount to the given directory
    !*/
    let empty = to_c_string("")?;
    syscall_ok(unsafe {
        libc::syscall(
            libc::SYS_move_mount, mount.as_raw_fd(), empty.as_ptr(),
            target.as_raw_fd(), empty.as_ptr(),
            MOVE_MOUNT_F_EMPTY_PATH | MOVE_MOUNT_T_EMPTY_PATH
        )
    })
}

fn detach_mount(mount_root: &OwnedFd) -> bool {
    /*!
    Detach the mount of the given root. The mount is released once
    it is not used anymore, the descriptor itself keeps it in use
    !*/
    let path = match to_c_string(&fd_path(mount_root)) {
        Ok(path) => path,
        Err(_) => return false
    };
    if unsafe { libc::umount2(path.as_ptr(), libc::MNT_DETACH) } != 0 {
        error!("Failed to detach mount: {}", io::Error::last_os_error());
        return false
    }
    true
}

fn fd_path(descriptor: &OwnedFd) -> String {
    /*!
    Provide the path the kernel resolves to the given descriptor
    !*/
    format!("/proc/self/fd/{}", descriptor.as_raw_fd())
}

fn to_c_string(value: &str) -> io::Result<CString> {
    CString::new(value).map_err(
        |_| io::Error::new(io::ErrorKind::InvalidInput, "embedded NUL byte")
    )
}

fn syscall_fd(result: libc::c_long) -> io::Result<OwnedFd> {
    if result < 0 {
        return Err(io::Error::last_os_error())
    }
    Ok(unsafe { OwnedFd::from_raw_fd(result as RawFd) })
}

fn syscall_ok(result: libc::c_long) -> io::Result<()> {
    if result < 0 {
        return Err(io::Error::last_os_error())
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use uzers::get_current_uid;

    fn caller() -> Caller {
        Caller::from_uid(get_current_uid()).unwrap()
    }

    #[test]
    fn test_open_setup_dir() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let base = tmp_dir.path();
        fs::create_dir(base.join("myapp_overlay")).unwrap();
        symlink("/etc", base.join("myapp_merged")).unwrap();
        let user_dir = open_dir(
            libc::AT_FDCWD, base.to_str().unwrap(), false
        ).unwrap();

        assert!(
            open_setup_dir(&user_dir, &caller(), "myapp", "overlay").is_ok()
        );
        // a symbolic link is never followed
        assert!(
            open_setup_dir(&user_dir, &caller(), "myapp", "merged").is_err()
        );
        // a directory of another user is refused
        let mut other = caller();
        other.uid += 1;
        assert!(
            open_setup_dir(&user_dir, &other, "myapp", "overlay").is_err()
        );
        // nothing is mounted on it
        let directory = open_dir(
            user_dir.as_raw_fd(), "myapp_overlay", false
        ).unwrap();
        assert!(! is_mount_root_of(&user_dir, &directory, TMPFS_MAGIC).unwrap());
    }

    #[test]
    fn test_open_root_owned_dir() {
        assert!(open_root_owned_dir("/").is_ok());
        let tmp_dir = tempfile::tempdir().unwrap();
        fs::set_permissions(
            tmp_dir.path(), fs::Permissions::from_mode(0o755)
        ).unwrap();
        let path = tmp_dir.path().to_str().unwrap();
        assert_eq!(get_current_uid() == 0, open_root_owned_dir(path).is_ok());
    }

    #[test]
    fn test_release_mounts_without_mounts() {
        let tmp_dir = tempfile::tempdir().unwrap();
        fs::create_dir(tmp_dir.path().join("myapp_merged")).unwrap();
        let user_dir = open_dir(
            libc::AT_FDCWD, tmp_dir.path().to_str().unwrap(), false
        ).unwrap();
        // directories which are no mounts and missing ones are fine
        assert!(release_mounts(&user_dir, "myapp"));
    }
}
