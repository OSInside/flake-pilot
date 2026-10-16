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
//! The overlay is the root filesystem of the sandbox and the tmpfs
//! keeps everything written to it. The overlay exists as long as
//! the sandbox. The tmpfs is kept for the next sandbox of the same
//! instance until it is discarded or the host is rebooted, nothing
//! written in the sandbox ends up on disk.
//!
//! Layers stacked on top of the rootfs are mostly part of it.
//! An overlay refuses lower directories of which one contains
//! another. The rootfs is therefore provided to the overlay
//! through a read only overlay of its own if layers are stacked
//! on top of it, which is mounted on a directory in the tmpfs
//! while the overlay is created.
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

use flakes::config::get_sandbox_tmpfs_size;
use flakes::defaults::{
    FLAKES_CONFIG, OVERLAY_BASE_DIR, OVERLAY_USER_DIR_NAME,
    OVERLAY_MERGED_NAME, OVERLAY_TMPFS_NAME, OVERLAY_UPPER_NAME,
    OVERLAY_WORK_NAME
};

use crate::caller::Caller;
use crate::validate;

// Constants of the file descriptor based mount API, linux/mount.h
const FSOPEN_CLOEXEC: libc::c_uint = 0x1;
const FSCONFIG_SET_STRING: libc::c_uint = 1;
const FSCONFIG_CMD_CREATE: libc::c_uint = 6;
const FSCONFIG_CMD_RECONFIGURE: libc::c_uint = 7;
const FSPICK_CLOEXEC: libc::c_uint = 0x1;
const FSPICK_EMPTY_PATH: libc::c_uint = 0x8;
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
const OVERLAY_DIR_MODE: libc::mode_t = 0o700;

// Max. number of layers a flake can stack on top of its rootfs
const MAX_LAYERS: usize = 16;

// Memory information of the kernel, the base of a tmpfs size given
// as percentage
const PROC_MEMINFO: &str = "/proc/meminfo";

// Directories in the tmpfs of an instance for the read only overlay
// of the rootfs: its mount point, and the empty directory stacked on
// top of the rootfs, a read only overlay needs two lower directories
const ROOTFS_OVERLAY_NAME: &str = "lower";
const ROOTFS_OVERLAY_EMPTY_NAME: &str = "empty";

// Directories created in the tmpfs of an instance
const TMPFS_DIR_NAMES: [&str; 4] = [
    OVERLAY_UPPER_NAME, OVERLAY_WORK_NAME,
    ROOTFS_OVERLAY_NAME, ROOTFS_OVERLAY_EMPTY_NAME
];

pub fn mount(
    caller: &Caller, instance: &str, rootfs: &str, layers: &[String],
    size: Option<&str>
) -> bool {
    /*!
    Create the overlay of the given rootfs for the given sandbox
    instance of the caller

    The rootfs is the lowest, read only layer. The given layers are
    stacked on top of it in the given order, read only as well. The
    upper and the work directory live in a tmpfs which belongs to
    the caller. A tmpfs which is left from a former sandbox of the
    instance is used again, it keeps what was written in that
    sandbox. Both mounts are created with nosuid and nodev. The
    layers are only read: access to their content through the
    overlay is checked against their own permissions, the overlay
    grants no access the caller does not have on the layers.

    The size of the tmpfs is the given size or the limit configured
    system wide, see get_tmpfs_size()
    !*/
    match mount_overlay(caller, instance, rootfs, layers, size) {
        Ok(()) => true,
        Err(error) => {
            error!("{error}");
            false
        }
    }
}

pub fn umount(caller: &Caller, instance: &str, discard: bool) -> bool {
    /*!
    Delete the overlay of the given sandbox instance of the caller

    The tmpfs is kept for the next sandbox of the instance. With
    discard it is deleted as well, along with everything that was
    written in the sandboxes of the instance. A directory which is
    no mount of the expected type is left alone
    !*/
    let result = validate::sandbox_instance(instance)
        .and_then(|_| open_user_dir(caller))
        .map(|user_dir| release_mounts(&user_dir, instance, discard));
    match result {
        Ok(ok) => ok,
        Err(error) => {
            error!("{error}");
            false
        }
    }
}

fn mount_overlay(
    caller: &Caller, instance: &str, rootfs: &str, layers: &[String],
    size: Option<&str>
) -> Result<(), String> {
    validate::sandbox_instance(instance)?;
    let size = get_tmpfs_size(
        size, get_sandbox_tmpfs_size().as_deref(), host_memory()?
    )?;
    if layers.len() > MAX_LAYERS {
        return Err(format!(
            "Up to {MAX_LAYERS} layers can be stacked on top of the rootfs"
        ))
    }
    validate::absolute_path(rootfs)?;
    let rootfs_dir = open_dir(libc::AT_FDCWD, rootfs, true).map_err(
        |error| format!("Failed to open rootfs {rootfs}: {error}")
    )?;
    // The topmost layer is the first one of the lower directories
    let mut layer_dirs: Vec<OwnedFd> = Vec::new();
    for layer in layers.iter().rev() {
        validate::absolute_path(layer)?;
        layer_dirs.push(open_dir(libc::AT_FDCWD, layer, true).map_err(
            |error| format!("Failed to open layer {layer}: {error}")
        )?);
    }
    let user_dir = open_user_dir(caller)?;

    // An overlay left behind by a sandbox which did not end
    // properly, e.g because its pilot got killed, is released
    // first. There is only one sandbox per instance
    release_mounts(&user_dir, instance, false);
    let merged_dir = open_setup_dir(
        &user_dir, caller, instance, OVERLAY_MERGED_NAME
    )?;

    // 1. The tmpfs of the instance, kept from a former sandbox or
    //    created along with its directories before it becomes visible
    let (tmpfs, created) = match open_kept_tmpfs(&user_dir, caller, instance)? {
        Some(tmpfs) => {
            info!("Using the writable layer kept for {instance}");
            if let Some(size) = &size {
                resize_tmpfs(&tmpfs, size).map_err(|error| format!(
                    "Failed to resize the writable layer of {instance} to \
                    {size}: {error}. It may hold more data already, discard \
                    it with the %discard pilot option"
                ))?;
            }
            (tmpfs, false)
        },
        None => {
            let tmpfs = create_tmpfs(
                &user_dir, caller, instance, size.as_deref()
            )?;
            (tmpfs, true)
        }
    };

    // 2. The overlay of the layers
    let overlay = mount_layers(&tmpfs, &merged_dir, &layer_dirs, &rootfs_dir);
    if let Err(error) = overlay {
        if created {
            detach_mount(&tmpfs);
        }
        return Err(format!(
            "Failed to mount the overlay of {rootfs} for {instance}: {error}"
        ))
    }
    Ok(())
}

fn mount_layers(
    tmpfs: &OwnedFd, merged_dir: &OwnedFd,
    layer_dirs: &[OwnedFd], rootfs_dir: &OwnedFd
) -> io::Result<()> {
    /*!
    Mount the overlay of the given layers on top of the given rootfs
    on the merged directory, with its upper and work directory in
    the given tmpfs. All of its directories are given through
    descriptors

    With layers the rootfs is provided through a read only overlay
    of its own, the layers are mostly part of the rootfs. That
    overlay is only attached while the overlay of the layers is
    created, which keeps it in use from then on
    !*/
    let rootfs_overlay = if layer_dirs.is_empty() {
        None
    } else {
        Some(mount_rootfs_overlay(tmpfs, rootfs_dir)?)
    };
    let lowerdir: Vec<String> = layer_dirs.iter()
        .chain(std::iter::once(rootfs_overlay.as_ref().unwrap_or(rootfs_dir)))
        .map(fd_path)
        .collect();
    let overlay = create_mount(OVERLAY_TYPE, &[
        ("lowerdir", &lowerdir.join(":")),
        ("upperdir", &format!("{}/{OVERLAY_UPPER_NAME}", fd_path(tmpfs))),
        ("workdir", &format!("{}/{OVERLAY_WORK_NAME}", fd_path(tmpfs)))
    ]).and_then(|overlay| attach_mount(&overlay, merged_dir));
    if let Some(rootfs_overlay) = rootfs_overlay {
        detach_mount(&rootfs_overlay);
    }
    overlay
}

fn mount_rootfs_overlay(
    tmpfs: &OwnedFd, rootfs_dir: &OwnedFd
) -> io::Result<OwnedFd> {
    /*!
    Mount a read only overlay of the given rootfs on its directory
    in the given tmpfs and provide its root. An overlay left on the
    directory, e.g by a helper which got killed, is released first
    !*/
    let tmpfs_fd = tmpfs.as_raw_fd();
    let mut mount_point = open_dir(tmpfs_fd, ROOTFS_OVERLAY_NAME, false)?;
    if is_mount_root_of(tmpfs, &mount_point, OVERLAY_MAGIC)? {
        detach_mount(&mount_point);
        mount_point = open_dir(tmpfs_fd, ROOTFS_OVERLAY_NAME, false)?;
    }
    let empty_dir = open_dir(tmpfs_fd, ROOTFS_OVERLAY_EMPTY_NAME, false)?;
    let overlay = create_mount(OVERLAY_TYPE, &[(
        "lowerdir",
        &format!("{}:{}", fd_path(&empty_dir), fd_path(rootfs_dir))
    )])?;
    attach_mount(&overlay, &mount_point)?;
    Ok(overlay)
}

fn open_kept_tmpfs(
    user_dir: &OwnedFd, caller: &Caller, instance: &str
) -> Result<Option<OwnedFd>, String> {
    /*!
    Provide the root of the tmpfs a former sandbox of the instance
    left, None if there is none. It has to belong to the caller.
    The directories in it are created if they are gone
    !*/
    let entry = format!("{instance}_{OVERLAY_TMPFS_NAME}");
    let directory = match open_dir(user_dir.as_raw_fd(), &entry, false) {
        Ok(directory) => directory,
        Err(_) => return Ok(None)
    };
    if ! is_mount_root_of(user_dir, &directory, TMPFS_MAGIC)
        .map_err(|error| format!("Failed to inspect {entry}: {error}"))?
    {
        return Ok(None)
    }
    let attributes = fstat(&directory).map_err(|error| error.to_string())?;
    if attributes.st_uid != caller.uid || attributes.st_mode & 0o077 != 0 {
        return Err(format!(
            "The tmpfs on {entry} does not belong to the caller or is \
            accessible for others"
        ))
    }
    for name in TMPFS_DIR_NAMES {
        match make_dir_for(&directory, name, caller) {
            Ok(()) => { },
            Err(error) if error.raw_os_error() == Some(libc::EEXIST) => { },
            Err(error) => return Err(
                format!("Failed to create {name} in the tmpfs: {error}")
            )
        }
    }
    Ok(Some(directory))
}

fn create_tmpfs(
    user_dir: &OwnedFd, caller: &Caller, instance: &str, size: Option<&str>
) -> Result<OwnedFd, String> {
    /*!
    Create the tmpfs of the instance, of the given size if any, with
    its directories created before it becomes visible, and provide
    its root
    !*/
    let tmpfs_dir = open_setup_dir(
        user_dir, caller, instance, OVERLAY_TMPFS_NAME
    )?;
    let uid = caller.uid.to_string();
    let gid = caller.gid.to_string();
    let mut options = vec![
        ("mode", "0700"), ("uid", uid.as_str()), ("gid", gid.as_str())
    ];
    if let Some(size) = size {
        options.push(("size", size));
    }
    let tmpfs = create_mount(TMPFS_TYPE, &options)
        .map_err(|error| format!("Failed to create the tmpfs: {error}"))?;
    for name in TMPFS_DIR_NAMES {
        make_dir_for(&tmpfs, name, caller).map_err(
            |error| format!("Failed to create {name} in the tmpfs: {error}")
        )?;
    }
    attach_mount(&tmpfs, &tmpfs_dir).map_err(
        |error| format!("Failed to mount the tmpfs of {instance}: {error}")
    )?;
    Ok(tmpfs)
}

fn release_mounts(user_dir: &OwnedFd, instance: &str, with_tmpfs: bool) -> bool {
    /*!
    Detach the overlay of the given instance if it is mounted, and
    with with_tmpfs the tmpfs of the instance as well. A setup
    directory which is no mount root of the expected type is left
    alone
    !*/
    let mut ok = true;
    let mut mounts = vec![(OVERLAY_MERGED_NAME, OVERLAY_MAGIC)];
    if with_tmpfs {
        mounts.push((OVERLAY_TMPFS_NAME, TMPFS_MAGIC));
    }
    for (name, magic) in mounts {
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
    parent: &OwnedFd, directory: &OwnedFd, magic: i64
) -> io::Result<bool> {
    /*!
    Check if the given directory below the given parent directory,
    e.g the user directory, is the root of a mount of the filesystem
    type of the given magic number. The lookup of a mount point
    provides the root of the filesystem mounted on it, which lives
    on another device
    !*/
    if fstat(directory)?.st_dev == fstat(parent)?.st_dev {
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

fn resize_tmpfs(tmpfs: &OwnedFd, size: &str) -> io::Result<()> {
    /*!
    Set the size of the given mounted tmpfs, through the descriptor
    of its root
    !*/
    let empty = to_c_string("")?;
    let context = syscall_fd(unsafe {
        libc::syscall(
            libc::SYS_fspick, tmpfs.as_raw_fd(), empty.as_ptr(),
            FSPICK_CLOEXEC | FSPICK_EMPTY_PATH
        )
    })?;
    let key = to_c_string("size")?;
    let value = to_c_string(size)?;
    syscall_ok(unsafe {
        libc::syscall(
            libc::SYS_fsconfig, context.as_raw_fd(), FSCONFIG_SET_STRING,
            key.as_ptr(), value.as_ptr(), 0
        )
    })?;
    syscall_ok(unsafe {
        libc::syscall(
            libc::SYS_fsconfig, context.as_raw_fd(), FSCONFIG_CMD_RECONFIGURE,
            std::ptr::null::<libc::c_char>(),
            std::ptr::null::<libc::c_char>(), 0
        )
    })
}

fn get_tmpfs_size(
    requested: Option<&str>, limit: Option<&str>, memory: u64
) -> Result<Option<String>, String> {
    /*!
    Provide the size of the tmpfs of a sandbox

    The size limit is configured by root in the system wide flakes
    configuration. It is the default size, and a flake may request
    a size up to it but not more. Without a limit the requested
    size is used, without either the tmpfs gets the default size of
    the kernel
    !*/
    let limit_bytes = match limit {
        Some(limit) => Some(validate::tmpfs_size(limit, memory).map_err(
            |error| format!("{error}, see sandbox_tmpfs_size in {FLAKES_CONFIG}")
        )?),
        None => None
    };
    let requested = match requested {
        Some(requested) => requested,
        None => return Ok(limit.map(ToOwned::to_owned))
    };
    let requested_bytes = validate::tmpfs_size(requested, memory)?;
    if let (Some(limit), Some(limit_bytes)) = (limit, limit_bytes) {
        if requested_bytes > limit_bytes {
            return Err(format!(
                "The requested tmpfs size {requested} exceeds the limit \
                of {limit} set in {FLAKES_CONFIG}"
            ))
        }
    }
    Ok(Some(requested.to_string()))
}

fn host_memory() -> Result<u64, String> {
    /*!
    Provide the memory of the host in bytes
    !*/
    let meminfo = std::fs::read_to_string(PROC_MEMINFO).map_err(
        |error| format!("Failed to read {PROC_MEMINFO}: {error}")
    )?;
    meminfo.lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|value| value.trim().strip_suffix("kB"))
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|kib| kib * 1024)
        .ok_or_else(|| format!("No MemTotal in {PROC_MEMINFO}"))
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
    fn test_get_tmpfs_size() {
        let memory = 8 << 30;
        // neither a request nor a limit, the default of the kernel
        assert_eq!(Ok(None), get_tmpfs_size(None, None, memory));
        // the limit is the default size
        assert_eq!(
            Ok(Some("2G".to_string())), get_tmpfs_size(None, Some("2G"), memory)
        );
        // a request without a limit, and one up to the limit
        assert_eq!(
            Ok(Some("4G".to_string())), get_tmpfs_size(Some("4G"), None, memory)
        );
        assert_eq!(
            Ok(Some("512M".to_string())),
            get_tmpfs_size(Some("512M"), Some("25%"), memory)
        );
        // a request above the limit is refused
        assert!(get_tmpfs_size(Some("3G"), Some("2G"), memory).is_err());
        assert!(get_tmpfs_size(Some("50%"), Some("2G"), memory).is_err());
        // as are invalid sizes
        assert!(get_tmpfs_size(Some("0"), None, memory).is_err());
        assert!(get_tmpfs_size(None, Some("lots"), memory).is_err());
    }

    #[test]
    fn test_host_memory() {
        assert!(host_memory().unwrap() > 0);
    }

    #[test]
    fn test_release_mounts_without_mounts() {
        let tmp_dir = tempfile::tempdir().unwrap();
        fs::create_dir(tmp_dir.path().join("myapp_merged")).unwrap();
        let user_dir = open_dir(
            libc::AT_FDCWD, tmp_dir.path().to_str().unwrap(), false
        ).unwrap();
        // directories which are no mounts and missing ones are fine
        assert!(release_mounts(&user_dir, "myapp", true));
    }
}
