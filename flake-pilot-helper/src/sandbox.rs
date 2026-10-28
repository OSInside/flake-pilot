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
//! The bubblewrap sandboxes with mount points below their root
//!
//! A sandbox runs on an overlay of a directory tree of the host,
//! see bubblewrap-pilot. The pilot mounts the overlay itself, in a
//! user and mount namespace of its own. The mounts of the host are
//! locked in such a namespace and the kernel refuses an overlay of
//! a tree with mount points below it, e.g the root of the host.
//! For such a tree the helper creates the mount namespace of the
//! sandbox instead.
//!
//! The helper moves into a new mount namespace, in which no mount
//! propagates to the host, and mounts the setup of the sandbox on
//! the directory of the caller below OVERLAY_BASE_DIR, e.g
//!
//! /var/tmp/bwrap_1000/  <- tmpfs
//!    ├── merged         <- overlay mount of the rootfs
//!    ├── upper          <- upper directory of the overlay
//!    ├── work           <- work directory of the overlay
//!    ├── lower          <- read only overlay of the rootfs
//!    └── empty          <- second layer of the read only overlay
//!
//! Then it drops its privileges for good, turns into the caller
//! and replaces itself with bwrap, which creates the sandbox in
//! that namespace. Nothing is mounted on the host, the mounts are
//! gone along with everything written to them once the sandbox has
//! terminated. The read only overlay of the rootfs is only created
//! with layers on top of the rootfs. Layers are mostly part of the
//! rootfs and an overlay refuses lower directories of which one
//! contains another.
//!
//! The rootfs and the layers are opened with the identity of the
//! caller, a tree the caller cannot reach is refused. The overlay
//! grants no access the caller does not have on the trees: access
//! to their content through the overlay is checked against their
//! own permissions. All mounts are created nosuid and nodev.
//!
//! The directory of the caller belongs to the caller. It is opened
//! without following a symbolic link and its owner is checked on
//! the open descriptor. Its parents belong to root and only root
//! may replace their entries, nobody else can swap the directory
//! on the path bwrap resolves. The mounts are created detached with the
//! file descriptor based mount API, prepared, and then attached to
//! these descriptors. No path is resolved twice. Apart from these
//! mounts the namespace provides the mounts of the host unchanged.
//! bwrap and everything it calls run with the no_new_privs flag,
//! no program run in the namespace can gain privileges. The
//! arguments of bwrap are passed on as they are, they are evaluated
//! with the privileges of the caller only.
//!
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::Command;

use flakes::config::get_sandbox_tmpfs_size;
use flakes::defaults::{
    BWRAP_PATH, OVERLAY_BASE_DIR, OVERLAY_USER_DIR_NAME,
    OVERLAY_MERGED_NAME, OVERLAY_UPPER_NAME, OVERLAY_WORK_NAME,
    OVERLAY_LOWER_NAME, OVERLAY_EMPTY_NAME
};
use flakes::tmpfs::{get_tmpfs_size, host_memory};

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

// Filesystem types of the setup
const TMPFS_TYPE: &str = "tmpfs";
const OVERLAY_TYPE: &str = "overlay";

// Permissions of the directories of the setup
const OVERLAY_DIR_MODE: libc::mode_t = 0o700;

// Max. number of layers a flake can stack on top of its rootfs
const MAX_LAYERS: usize = 16;

// Max. number of groups of the caller, NGROUPS_MAX of the kernel.
// setgroups() refuses more
const MAX_GROUPS: libc::c_int = 65536;

// Max. number of attempts to read the groups of the caller with a
// list of the size the previous attempt asked for
const MAX_GROUP_READS: usize = 4;

pub fn run(
    caller: &Caller, rootfs: &str, layers: &[String], size: Option<&str>,
    term: Option<&str>, args: &[String]
) -> bool {
    /*!
    Mount the root filesystem of a sandbox of the caller in a new
    mount namespace and replace the helper by bwrap, called with the
    given arguments and the privileges of the caller

    The rootfs is the lowest, read only layer of the overlay. The
    given layers are stacked on top of it in the given order, read
    only as well. The upper and the work directory live in a tmpfs
    of the given size, see flakes::tmpfs::get_tmpfs_size().

    bwrap gets a fresh environment, like the pilots the helper
    calls. The variables of the caller are provided to the
    application through the bwrap options
    !*/
    if let Err(error) = mount_root(caller, rootfs, layers, size) {
        error!("{error}");
        return false
    }
    if let Err(error) = drop_privileges(caller) {
        error!("{error}");
        return false
    }
    let term = term.filter(|term| match validate::terminal_type(term) {
        Ok(()) => true,
        Err(error) => {
            warn!("{error}, not passed on");
            false
        }
    });
    let mut call = Command::new(BWRAP_PATH);
    call.args(args)
        .env_clear()
        .env("PATH", crate::HELPER_PATH)
        .env("HOME", &caller.home)
        .env("USER", &caller.name)
        .env("LOGNAME", &caller.name);
    if let Some(term) = term {
        call.env("TERM", term);
    }
    let error = call.exec();
    error!("Failed to execute {BWRAP_PATH}: {error}");
    false
}

fn mount_root(
    caller: &Caller, rootfs: &str, layers: &[String], size: Option<&str>
) -> Result<(), String> {
    let size = get_tmpfs_size(
        size, get_sandbox_tmpfs_size().as_deref(), host_memory()?
    )?;
    if layers.len() > MAX_LAYERS {
        return Err(format!(
            "Up to {MAX_LAYERS} layers can be stacked on top of the rootfs"
        ))
    }
    validate::absolute_path(rootfs)?;
    for layer in layers {
        validate::absolute_path(layer)?;
    }

    // Everything below happens in the namespace of the sandbox. An
    // overlay refuses layers of the mounts of another namespace, the
    // directories are therefore opened in it
    enter_mount_namespace()?;
    let user_dir = open_user_dir(caller)?;
    let (rootfs_dir, layer_dirs) = open_layers_as(caller, rootfs, layers)?;

    // 1. The tmpfs, created along with its directories before it
    //    becomes visible
    let tmpfs = create_tmpfs(caller, size.as_deref(), ! layer_dirs.is_empty())
        .map_err(|error| format!("Failed to create the tmpfs: {error}"))?;
    attach_mount(&tmpfs, &user_dir).map_err(
        |error| format!("Failed to mount the tmpfs: {error}")
    )?;

    // 2. The overlay of the layers
    let merged_dir = open_dir(tmpfs.as_raw_fd(), OVERLAY_MERGED_NAME, false)
        .map_err(|error| format!("Failed to open {OVERLAY_MERGED_NAME}: {error}"))?;
    mount_layers(&tmpfs, &merged_dir, &layer_dirs, &rootfs_dir).map_err(
        |error| format!("Failed to mount the overlay of {rootfs}: {error}")
    )
}

fn enter_mount_namespace() -> Result<(), String> {
    /*!
    Move the helper into a new mount namespace and make all of its
    mounts private. No mount created in it propagates to the host
    and no mount of the host propagates into it
    !*/
    if unsafe { libc::unshare(libc::CLONE_NEWNS) } != 0 {
        return Err(format!(
            "Failed to create the mount namespace of the sandbox: {}",
            io::Error::last_os_error()
        ))
    }
    let root = to_c_string("/").map_err(|error| error.to_string())?;
    if unsafe {
        libc::mount(
            std::ptr::null(), root.as_ptr(), std::ptr::null(),
            libc::MS_REC | libc::MS_PRIVATE, std::ptr::null()
        )
    } != 0 {
        return Err(format!(
            "Failed to make the mounts of the namespace private: {}",
            io::Error::last_os_error()
        ))
    }
    Ok(())
}

fn open_layers_as(
    caller: &Caller, rootfs: &str, layers: &[String]
) -> Result<(OwnedFd, Vec<OwnedFd>), String> {
    /*!
    Open the rootfs and the layers with the identity of the caller.
    The caller has to be able to reach them itself. The topmost
    layer is the first one of the returned layers
    !*/
    let groups = caller_groups(caller)?;
    let switched = unsafe {
        libc::setgroups(groups.len(), groups.as_ptr()) == 0
            && libc::setegid(caller.gid) == 0
            && libc::seteuid(caller.uid) == 0
    };
    // The paths are resolved following symbolic links the caller may
    // control. They must never be resolved with the privileges of
    // root, unless the caller is root itself
    let result = if ! switched {
        Err(format!(
            "Failed to switch to the caller: {}", io::Error::last_os_error()
        ))
    } else if unsafe {
        libc::geteuid() != caller.uid || libc::getegid() != caller.gid
    } {
        Err(
            "Refusing to open the layers without the identity of the caller"
                .to_string()
        )
    } else {
        open_layers(rootfs, layers)
    };
    if unsafe { libc::seteuid(0) != 0 || libc::setegid(0) != 0 } {
        return Err(format!(
            "Failed to switch back from the caller: {}",
            io::Error::last_os_error()
        ))
    }
    result
}

fn open_layers(
    rootfs: &str, layers: &[String]
) -> Result<(OwnedFd, Vec<OwnedFd>), String> {
    let rootfs_dir = open_dir(libc::AT_FDCWD, rootfs, true).map_err(
        |error| format!("Failed to open rootfs {rootfs}: {error}")
    )?;
    let mut layer_dirs: Vec<OwnedFd> = Vec::new();
    for layer in layers.iter().rev() {
        layer_dirs.push(open_dir(libc::AT_FDCWD, layer, true).map_err(
            |error| format!("Failed to open layer {layer}: {error}")
        )?);
    }
    Ok((rootfs_dir, layer_dirs))
}

fn drop_privileges(caller: &Caller) -> Result<(), String> {
    /*!
    Turn into the caller for good: its groups, its group and its
    user ID, with no way back. The no_new_privs flag keeps every
    program called from now on from gaining privileges, e.g
    through a setuid program
    !*/
    let groups = caller_groups(caller)?;
    let dropped = unsafe {
        libc::setgroups(groups.len(), groups.as_ptr()) == 0
            && libc::setresgid(caller.gid, caller.gid, caller.gid) == 0
            && libc::setresuid(caller.uid, caller.uid, caller.uid) == 0
            && libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0
    };
    if ! dropped {
        return Err(format!(
            "Failed to drop the privileges: {}", io::Error::last_os_error()
        ))
    }
    if ! caller.is_root() && unsafe { libc::setuid(0) } == 0 {
        return Err("The privileges could not be dropped".to_string())
    }
    Ok(())
}

fn caller_groups(caller: &Caller) -> Result<Vec<libc::gid_t>, String> {
    /*!
    Provide the groups the caller is a member of, as listed in the
    group database, along with its primary group

    getgrouplist() provides no error, a list too short is the only
    failure it reports, along with the required length. That length
    is bounded by MAX_GROUPS and the number of attempts by
    MAX_GROUP_READS, a group database which keeps growing or a
    bogus length cannot keep the helper in the loop
    !*/
    let name = to_c_string(&caller.name).map_err(|error| error.to_string())?;
    let mut count: libc::c_int = 32;
    for _ in 0..MAX_GROUP_READS {
        let mut groups: Vec<libc::gid_t> = vec![0; count as usize];
        let mut found = count;
        if unsafe {
            libc::getgrouplist(
                name.as_ptr(), caller.gid, groups.as_mut_ptr(), &mut found
            )
        } >= 0 {
            groups.truncate(found as usize);
            return Ok(groups)
        }
        // The list was too short, found tells the required length
        if found <= count {
            return Err(format!("Failed to read the groups of {}", caller.name))
        }
        if found > MAX_GROUPS {
            return Err(format!(
                "{} is a member of more than {MAX_GROUPS} groups", caller.name
            ))
        }
        count = found;
    }
    Err(format!(
        "Failed to read the groups of {}, their number keeps changing",
        caller.name
    ))
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
    in the given tmpfs and provide its root
    !*/
    let tmpfs_fd = tmpfs.as_raw_fd();
    let mount_point = open_dir(tmpfs_fd, OVERLAY_LOWER_NAME, false)?;
    let empty_dir = open_dir(tmpfs_fd, OVERLAY_EMPTY_NAME, false)?;
    let overlay = create_mount(OVERLAY_TYPE, &[(
        "lowerdir",
        &format!("{}:{}", fd_path(&empty_dir), fd_path(rootfs_dir))
    )])?;
    attach_mount(&overlay, &mount_point)?;
    Ok(overlay)
}

fn create_tmpfs(
    caller: &Caller, size: Option<&str>, with_layers: bool
) -> io::Result<OwnedFd> {
    /*!
    Create the tmpfs of the sandbox, of the given size if any, with
    its directories created before it becomes visible, and provide
    its root
    !*/
    let uid = caller.uid.to_string();
    let gid = caller.gid.to_string();
    let mut options = vec![
        ("mode", "0700"), ("uid", uid.as_str()), ("gid", gid.as_str())
    ];
    if let Some(size) = size {
        options.push(("size", size));
    }
    let tmpfs = create_mount(TMPFS_TYPE, &options)?;
    let mut dir_names = vec![
        OVERLAY_MERGED_NAME, OVERLAY_UPPER_NAME, OVERLAY_WORK_NAME
    ];
    if with_layers {
        dir_names.push(OVERLAY_LOWER_NAME);
        dir_names.push(OVERLAY_EMPTY_NAME);
    }
    for name in dir_names {
        make_dir_for(&tmpfs, name, caller)?;
    }
    Ok(tmpfs)
}

fn open_user_dir(caller: &Caller) -> Result<OwnedFd, String> {
    /*!
    Open the directory which keeps the sandbox setups of the caller

    The helper itself attaches the tmpfs to the opened directory and
    never resolves its path again. bwrap however is given the path
    of the merged directory and resolves it with the privileges of
    the caller. Nobody but the caller and root may therefore replace
    the directory of the caller on that path: it has to belong to
    the caller, an owner only root can change, and the base
    directory and all of its parents have to be protected, see
    open_root_owned_dir(). What the directory contains on the host
    does not matter, the tmpfs covers it in the namespace
    !*/
    let base_dir = open_root_owned_dir(OVERLAY_BASE_DIR)?;
    let name = format!("{OVERLAY_USER_DIR_NAME}_{}", caller.uid);
    let user_dir = open_dir(base_dir.as_raw_fd(), &name, false).map_err(
        |error| format!("Failed to open {OVERLAY_BASE_DIR}/{name}: {error}")
    )?;
    let attributes = fstat(&user_dir).map_err(|error| error.to_string())?;
    if attributes.st_uid != caller.uid {
        return Err(format!(
            "{OVERLAY_BASE_DIR}/{name} has to belong to the caller"
        ))
    }
    Ok(user_dir)
}

fn open_root_owned_dir(path: &str) -> Result<OwnedFd, String> {
    /*!
    Open the given absolute directory, every component of which has
    to be a directory owned by root, not a symbolic link. Nobody but
    root may replace the entries of a component: it must not be
    writable for others, unless it is sticky like /var/tmp, where
    only the owner of an entry may rename or remove it
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
        if attributes.st_mode & 0o022 != 0
            && attributes.st_mode & libc::S_ISVTX == 0
        {
            return Err(format!(
                "{component} of {path} is writable for others and not sticky"
            ))
        }
    }
    Ok(directory)
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
    use std::os::unix::fs::PermissionsExt;
    use uzers::get_current_uid;

    fn caller() -> Caller {
        Caller::from_uid(get_current_uid()).unwrap()
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
        // writable for others is refused, unless sticky
        fs::set_permissions(
            tmp_dir.path(), fs::Permissions::from_mode(0o777)
        ).unwrap();
        assert!(open_root_owned_dir(path).is_err());
        fs::set_permissions(
            tmp_dir.path(), fs::Permissions::from_mode(0o1777)
        ).unwrap();
        assert_eq!(get_current_uid() == 0, open_root_owned_dir(path).is_ok());
    }

    #[test]
    fn test_caller_groups() {
        let caller = caller();
        // the primary group is always part of the groups
        assert!(caller_groups(&caller).unwrap().contains(&caller.gid));
    }

    #[test]
    fn test_open_layers() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let base = tmp_dir.path().to_str().unwrap();
        fs::create_dir(format!("{base}/layer")).unwrap();
        let layers = vec![format!("{base}/layer"), base.to_string()];
        let (_, layer_dirs) = open_layers(base, &layers).unwrap();
        assert_eq!(2, layer_dirs.len());
        assert!(open_layers(&format!("{base}/none"), &[]).is_err());
    }
}
