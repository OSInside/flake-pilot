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
//! and replaces itself with bubblewrap-pilot, which calls bwrap to
//! create the sandbox in that namespace, see bubblewrap.rs. Nothing
//! is mounted on the host, the mounts are
//! gone along with everything written to them once the sandbox has
//! terminated. The read only overlay of the rootfs is only created
//! with layers on top of the rootfs. Layers are mostly part of the
//! rootfs and an overlay refuses lower directories of which one
//! contains another.
//!
//! The rootfs and the layers are opened with the identity of the
//! caller, a tree the caller cannot reach and read is refused. The
//! O_PATH descriptor of a directory only requires search permission
//! on the directories above it, the access to the directory itself
//! is therefore checked on the descriptor, still with the identity
//! of the caller, before the helper switches back to root. The overlay
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
//! The pilot and everything it calls run with the no_new_privs
//! flag, no program run in the namespace can gain privileges. The
//! arguments of bwrap are evaluated with the privileges of the
//! caller only.
//!
//! Mounts which persist on the host
//! --------------------------------
//! Nothing the application mounts in the sandbox reaches the host:
//! the namespace of the helper is private and bwrap makes the mounts
//! of its own namespace slaves, which receive but never send mount
//! events. Root can let all mounts of the application persist on
//! the host anyway, e.g for a flake which mounts a disk.
//!
//! For such a sandbox the helper does not replace itself with
//! bubblewrap-pilot. It runs the pilot in a child process, which
//! turns into bwrap, and waits for it as root. bwrap reports the
//! process ID of the sandbox through its info pipe and the helper
//! opens the mount namespace of the sandbox. The namespace
//! descriptor keeps the namespace, along with the mounts in it,
//! after the sandbox has terminated.
//!
//! Once bwrap has set up the sandbox, it waits for its block pipe
//! before it starts the application. The helper waits until the
//! sandbox blocks on the pipe, which the kernel shows in
//! /proc/PID/syscall, and records the mount table of the sandbox:
//! the mounts bwrap created for it. Then it lets the application
//! start.
//!
//! Once bwrap has terminated, the mounts of the namespace which
//! were not recorded are the mounts of the application. A mount the
//! host created meanwhile, which reached the sandbox as a slave of a
//! mount of the host, is not one of them, neither is a mount which
//! exists on the host at the same location already. The helper
//! moves into the namespace of the sandbox and clones the topmost of
//! these mounts, along with the mounts below them. Back in the
//! namespace of the host, it attaches the clones at the same
//! location. A missing mount point is created, a symbolic link on
//! the way to it is refused.
//!
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::fs::File;
use std::io::{Read, Write};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::thread::sleep;
use std::time::Duration;

use flakes::config::get_sandbox_tmpfs_size;
use flakes::defaults::{
    OVERLAY_BASE_DIR, OVERLAY_USER_DIR_NAME,
    OVERLAY_MERGED_NAME, OVERLAY_UPPER_NAME, OVERLAY_WORK_NAME,
    OVERLAY_LOWER_NAME, OVERLAY_EMPTY_NAME,
    SANDBOX_INFO_FD_VAR, SANDBOX_BLOCK_FD_VAR
};
use flakes::mountinfo::{self, Mount};
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
const OPEN_TREE_CLONE: libc::c_uint = 0x1;
const AT_RECURSIVE: libc::c_uint = 0x8000;

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

// Interval the helper checks whether bwrap has set up the sandbox
// in, it starts short and grows up to the max. interval
const BLOCK_POLL_MIN: Duration = Duration::from_millis(1);
const BLOCK_POLL_MAX: Duration = Duration::from_millis(50);

// Permissions of a mount point the helper creates on the host
const MOUNT_POINT_DIR_MODE: libc::mode_t = 0o755;
const MOUNT_POINT_FILE_MODE: libc::mode_t = 0o644;

// procfs, the mount table of the reading thread, and the mount
// table, the mount namespace and the current system call of a
// process. setns() moves the calling thread only, its mount table is
// the one of thread-self, self is the one of the main thread
const PROC_DIR: &str = "/proc";
const PROC_SELF_MOUNTINFO: &str = "thread-self/mountinfo";
const PROC_MOUNTINFO: &str = "mountinfo";
const PROC_NAMESPACE: &str = "ns/mnt";
const PROC_SYSCALL: &str = "syscall";

pub struct Host {
    /// Mount namespace of the host, the mounts are attached in it
    namespace: OwnedFd,
    /// procfs of the host, it provides the mount table of the
    /// namespace the helper is in, whichever that is
    proc_dir: OwnedFd,
}

impl Host {
    pub fn open(caller: &Caller) -> Result<Host, String> {
        /*!
        Keep what is needed of the host to attach mounts to it later,
        before the helper leaves its mount namespace

        Only root may let mounts persist on the host, for everybody
        else the sandbox would be a way to mount on the host
        !*/
        if ! caller.is_root() {
            return Err("Only root can let mounts persist on the host".to_string())
        }
        let namespace = open_file(
            libc::AT_FDCWD, &format!("{PROC_DIR}/thread-self/{PROC_NAMESPACE}")
        ).map_err(
            |error| format!("Failed to open the mount namespace of the host: {error}")
        )?;
        let proc_dir = open_dir(libc::AT_FDCWD, PROC_DIR, false).map_err(
            |error| format!("Failed to open {PROC_DIR}: {error}")
        )?;
        Ok(Host { namespace, proc_dir })
    }
}

// The sandbox of a flake whose mounts persist on the host
struct Sandbox {
    /// Mount namespace of the sandbox, it keeps the mounts of the
    /// application after the sandbox has terminated
    namespace: OwnedFd,
    /// Mounts bwrap created for the sandbox, recorded before the
    /// application started
    setup_mounts: Vec<Mount>,
}

pub fn run_persisting(
    host: &Host, start_pilot: impl FnOnce(&[(&str, String)])
) -> ExitCode {
    /*!
    Run bubblewrap-pilot in a child process, see the given call
    start_pilot(), and let the mounts the application created in the
    sandbox persist on the host once the sandbox has terminated, see
    persist_mounts()

    The pilot passes the info pipe, which bwrap reports the process
    ID of the sandbox to, and the block pipe, which bwrap waits for
    before it starts the application, on to bwrap. start_pilot()
    gets the environment variables which name them, it drops the
    privileges of the helper and replaces the child process by the
    pilot, which replaces itself by bwrap. The helper opens the mount
    namespace of the sandbox and records the mounts bwrap created
    for it in between, see open_sandbox(). A terminal interrupt is
    meant for the application, the helper ignores it and waits for
    bwrap.

    The exit code of bwrap is the one of the helper. If a mount
    could not be attached, the helper fails although bwrap succeeded
    !*/
    let pipes = create_pipe().and_then(|info| Ok((info, create_pipe()?)));
    let ((info_read, info_write), (block_read, block_write)) = match pipes {
        Ok(pipes) => pipes,
        Err(error) => {
            error!("Failed to create the pipes of bwrap: {error}");
            return ExitCode::FAILURE
        }
    };
    // bwrap gets the block pipe with the same descriptor number, it
    // is what the sandbox waits for, see wait_for_block()
    let block_fd = block_read.as_raw_fd();
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        error!("Failed to fork: {}", io::Error::last_os_error());
        return ExitCode::FAILURE
    }
    if pid == 0 {
        // bwrap inherits its ends of the pipes, all other
        // descriptors of the helper are closed on exec
        match clear_cloexec(&info_write).and_then(|_| clear_cloexec(&block_read)) {
            Ok(()) => start_pilot(&[
                (SANDBOX_INFO_FD_VAR, info_write.as_raw_fd().to_string()),
                (SANDBOX_BLOCK_FD_VAR, block_fd.to_string())
            ]),
            Err(error) => error!("Failed to pass the pipes to bwrap: {error}")
        }
        unsafe { libc::_exit(1) }
    }
    drop(info_write);
    drop(block_read);
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGQUIT, libc::SIG_IGN);
    }

    let mut exit_status = None;
    let sandbox = open_sandbox(info_read, host, pid, block_fd, &mut exit_status);
    // The application starts, also if the sandbox could not be
    // recorded: the helper does not decide whether it runs
    if let Err(error) = File::from(block_write).write_all(b"1") {
        debug!("Failed to start the application: {error}");
    }
    let status = match exit_status.map(Ok).unwrap_or_else(|| wait_for(pid)) {
        Ok(status) => status,
        Err(error) => {
            error!("Failed to wait for bwrap: {error}");
            return ExitCode::FAILURE
        }
    };
    let persisted = match sandbox {
        Ok(Some(sandbox)) => persist_mounts(&sandbox, host),
        // bwrap failed before it started the application, it
        // reported why
        Ok(None) => true,
        Err(error) => {
            error!("{error}");
            error!("No mount of the application persists on the host");
            false
        }
    };
    match libc::WIFEXITED(status).then(|| libc::WEXITSTATUS(status)) {
        Some(0) if ! persisted => ExitCode::FAILURE,
        Some(code) => ExitCode::from(code as u8),
        // bwrap was terminated by a signal
        None => ExitCode::FAILURE
    }
}

fn open_sandbox(
    info: OwnedFd, host: &Host, bwrap_pid: libc::pid_t, block_fd: RawFd,
    exit_status: &mut Option<libc::c_int>
) -> Result<Option<Sandbox>, String> {
    /*!
    Open the mount namespace of the sandbox whose process ID bwrap
    reports through the given info pipe and record the mounts bwrap
    created for it, once the sandbox waits for the given block pipe,
    see wait_for_block()

    None if bwrap failed before it started the application. It
    closes the info pipe without a report if it fails before it
    creates the sandbox. If bwrap terminated meanwhile, its wait
    status is stored in exit_status
    !*/
    let mut report = String::new();
    File::from(info).read_to_string(&mut report).map_err(
        |error| format!("Failed to read the process ID of the sandbox: {error}")
    )?;
    if report.is_empty() {
        return Ok(None)
    }
    let pid = child_pid(&report).ok_or_else(
        || format!("No process ID of the sandbox in: {report:?}")
    )?;
    let namespace = open_file(
        host.proc_dir.as_raw_fd(), &format!("{pid}/{PROC_NAMESPACE}")
    ).map_err(|error| format!(
        "Failed to open the mount namespace of the sandbox: {error}"
    ))?;
    if ! wait_for_block(host, pid, block_fd, bwrap_pid, exit_status)? {
        return Ok(None)
    }
    let setup_mounts = read_mounts(
        &host.proc_dir, &format!("{pid}/{PROC_MOUNTINFO}")
    )?;
    Ok(Some(Sandbox { namespace, setup_mounts }))
}

fn wait_for_block(
    host: &Host, pid: u32, block_fd: RawFd, bwrap_pid: libc::pid_t,
    exit_status: &mut Option<libc::c_int>
) -> Result<bool, String> {
    /*!
    Wait until the sandbox of the given process ID reads from the
    given block pipe

    bwrap reads from the block pipe once it has set up the sandbox,
    right before it starts the application. The current system call
    of the sandbox, see proc_pid_syscall(5), is checked until it is
    the read from the pipe. false if the sandbox or bwrap terminated
    before, bwrap reported why
    !*/
    let syscall_file = format!("{pid}/{PROC_SYSCALL}");
    let mut delay = BLOCK_POLL_MIN;
    loop {
        let mut syscall = String::new();
        let read = open_file(host.proc_dir.as_raw_fd(), &syscall_file)
            .map(File::from)
            .and_then(|mut file| file.read_to_string(&mut syscall));
        match read {
            Ok(_) if reads_from(&syscall, block_fd) => return Ok(true),
            Ok(_) => { },
            Err(error) if error.kind() == io::ErrorKind::NotFound
                || error.raw_os_error() == Some(libc::ESRCH) =>
            {
                return Ok(false)
            },
            Err(error) => return Err(format!(
                "Failed to read the system call of the sandbox: {error}"
            ))
        }
        match try_wait_for(bwrap_pid) {
            Ok(Some(status)) => {
                *exit_status = Some(status);
                return Ok(false)
            },
            Ok(None) => { },
            Err(error) => return Err(format!("Failed to check bwrap: {error}"))
        }
        sleep(delay);
        delay = (delay * 2).min(BLOCK_POLL_MAX);
    }
}

fn reads_from(syscall: &str, fd: RawFd) -> bool {
    /*!
    Check if the given content of /proc/PID/syscall is a read from
    the given descriptor. It starts with the number of the system
    call and its first argument in hex, e.g '0 0x5 ...' on x86_64,
    or reads 'running' if the process is not in a system call
    !*/
    let mut fields = syscall.split_whitespace();
    let number = fields.next().and_then(|number| number.parse::<libc::c_long>().ok());
    let argument = fields.next()
        .and_then(|argument| argument.strip_prefix("0x"))
        .and_then(|argument| u64::from_str_radix(argument, 16).ok());
    number == Some(libc::SYS_read) && argument == Some(fd as u64)
}

fn child_pid(report: &str) -> Option<u32> {
    /*!
    Provide the process ID of the sandbox out of the JSON report
    bwrap writes to its info pipe, e.g {"child-pid": 42, ...}
    !*/
    report.split("\"child-pid\":").nth(1)?
        .trim_start()
        .split(|c: char| ! c.is_ascii_digit())
        .next()?
        .parse().ok()
}

fn persist_mounts(sandbox: &Sandbox, host: &Host) -> bool {
    /*!
    Attach the mounts the application created in the sandbox to the
    host, at the same location, see select_mounts()

    The mounts of the host are read first. The mounts of the
    application are cloned in the mount namespace of the sandbox,
    where their location is the one in the sandbox, and attached in
    the namespace of the host
    !*/
    let mut persisted = true;
    if let Err(error) = enter_namespace(&host.namespace) {
        error!("Failed to enter the namespace of the host: {error}");
        return false
    }
    let host_mounts = match read_mounts(&host.proc_dir, PROC_SELF_MOUNTINFO) {
        Ok(mounts) => mounts,
        Err(error) => {
            error!("{error}");
            return false
        }
    };
    if let Err(error) = enter_namespace(&sandbox.namespace) {
        error!("Failed to enter the namespace of the sandbox: {error}");
        return false
    }
    let mounts = match read_mounts(&host.proc_dir, PROC_SELF_MOUNTINFO) {
        Ok(mounts) => mounts,
        Err(error) => {
            error!("{error}");
            return false
        }
    };
    let mut clones: Vec<(PathBuf, OwnedFd)> = Vec::new();
    for mount in select_mounts(&mounts, &sandbox.setup_mounts, &host_mounts) {
        match clone_mount(&mount.mount_point) {
            Ok(clone) => clones.push((mount.mount_point.clone(), clone)),
            Err(error) => {
                error!("Failed to clone {}: {error}", mount.mount_point.display());
                persisted = false
            }
        }
    }
    if let Err(error) = enter_namespace(&host.namespace) {
        error!("Failed to return to the namespace of the host: {error}");
        return false
    }
    for (mount_point, clone) in clones {
        match attach_on_host(&clone, &mount_point) {
            Ok(()) => debug!("{} persists on the host", mount_point.display()),
            Err(error) => {
                error!(
                    "Failed to persist {} on the host: {error}",
                    mount_point.display()
                );
                persisted = false
            }
        }
    }
    persisted
}

fn select_mounts<'a>(
    mounts: &'a [Mount], setup_mounts: &[Mount], host_mounts: &[Mount]
) -> Vec<&'a Mount> {
    /*!
    Select the mounts of the given sandbox mount table which persist
    on the host: the mounts the application created. These are the
    mounts which are not among the given mounts bwrap created for the
    sandbox, except:

    * a slave of a peer group of the host. It is a mount the host
      created while the sandbox ran, which reached the sandbox
      through the propagation bwrap set up

    * a mount which exists on the host already, at the same location
      and of the same filesystem tree

    A mount ID is only reused once the mount is gone. A mount of the
    application which got the ID of a mount of bwrap the application
    unmounted is told apart by its location and its filesystem tree.
    Only the topmost mounts are selected, the mounts below them are
    cloned along with them
    !*/
    let host_groups: HashSet<u64> = host_mounts.iter()
        .filter_map(|host_mount| host_mount.shared)
        .collect();
    let persisting: Vec<&Mount> = mounts.iter()
        .filter(|mount| ! setup_mounts.iter().any(|setup_mount| {
            setup_mount.id == mount.id
                && setup_mount.device == mount.device
                && setup_mount.root == mount.root
                && setup_mount.mount_point == mount.mount_point
        }))
        .filter(|mount| ! mount.master.is_some_and(
            |group| host_groups.contains(&group)
        ))
        .filter(|mount| ! host_mounts.iter().any(|host_mount| {
            host_mount.mount_point == mount.mount_point
                && host_mount.device == mount.device
                && host_mount.root == mount.root
        }))
        .collect();
    persisting.iter()
        .filter(|mount| ! persisting.iter().any(|other| other.id == mount.parent_id))
        .copied()
        .collect()
}

fn clone_mount(mount_point: &Path) -> io::Result<OwnedFd> {
    /*!
    Clone the mount on the given mount point of the namespace the
    helper is in, along with the mounts below it. The clone is
    detached and provided through the descriptor of its root
    !*/
    let path = to_c_string(&mount_point.to_string_lossy())?;
    syscall_fd(unsafe {
        libc::syscall(
            libc::SYS_open_tree, libc::AT_FDCWD, path.as_ptr(),
            OPEN_TREE_CLONE | libc::O_CLOEXEC as libc::c_uint | AT_RECURSIVE
        )
    })
}

fn attach_on_host(clone: &OwnedFd, mount_point: &Path) -> Result<(), String> {
    /*!
    Attach the given detached mount to the given mount point of the
    host

    The path is resolved from the root one component at a time,
    without following a symbolic link. Missing components are
    created as directories, the last one as a directory or a file,
    the type of the root of the mount. A mount on the root itself
    is not attached, it would hide the filesystem of the host
    !*/
    let components = mount_point_components(mount_point)?;
    let mut target = open_dir(libc::AT_FDCWD, "/", false)
        .map_err(|error| format!("Failed to open /: {error}"))?;
    let is_dir = file_type(clone).map_err(|error| error.to_string())? == libc::S_IFDIR;
    for (index, component) in components.iter().enumerate() {
        let last = index + 1 == components.len();
        target = open_mount_point(&target, component, ! last || is_dir).map_err(
            |error| format!("Failed to open the mount point {component}: {error}")
        )?;
    }
    attach_mount(clone, &target).map_err(|error| error.to_string())
}

fn mount_point_components(mount_point: &Path) -> Result<Vec<String>, String> {
    /*!
    Provide the components of the given mount point below the root.
    The mount table lists absolute, normalized paths, anything else
    is refused, and so is the root itself
    !*/
    let mut components = mount_point.components();
    if components.next() != Some(Component::RootDir) {
        return Err("Not an absolute path".to_string())
    }
    let components: Vec<String> = components
        .map(|component| match component {
            Component::Normal(name) => Ok(name.to_string_lossy().to_string()),
            _ => Err("Not a normalized path".to_string())
        })
        .collect::<Result<_, _>>()?;
    if components.is_empty() {
        return Err("A mount on / cannot persist on the host".to_string())
    }
    Ok(components)
}

fn open_mount_point(
    directory: &OwnedFd, name: &str, is_dir: bool
) -> io::Result<OwnedFd> {
    /*!
    Open the given directory or file below the given directory,
    without following a symbolic link. It is created if missing
    !*/
    let fd = directory.as_raw_fd();
    let c_name = to_c_string(name)?;
    let open = || -> io::Result<OwnedFd> {
        let mut flags = libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        if is_dir {
            flags |= libc::O_DIRECTORY;
        }
        syscall_fd(unsafe { libc::openat(fd, c_name.as_ptr(), flags) } as libc::c_long)
    };
    let mount_point = match open() {
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => {
            let created = if is_dir {
                unsafe { libc::mkdirat(fd, c_name.as_ptr(), MOUNT_POINT_DIR_MODE) }
            } else {
                let file = unsafe {
                    libc::openat(
                        fd, c_name.as_ptr(),
                        libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY
                            | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                        MOUNT_POINT_FILE_MODE as libc::c_uint
                    )
                };
                if file >= 0 {
                    unsafe { libc::close(file) };
                }
                file
            };
            if created < 0 {
                return Err(io::Error::last_os_error())
            }
            open()?
        },
        result => result?
    };
    // A symbolic link opened as O_PATH is the link itself
    let expected = if is_dir { libc::S_IFDIR } else { libc::S_IFREG };
    if file_type(&mount_point)? != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            if is_dir { "not a directory" } else { "not a regular file" }
        ))
    }
    Ok(mount_point)
}

fn read_mounts(proc_dir: &OwnedFd, path: &str) -> Result<Vec<Mount>, String> {
    /*!
    Read the given mount table of the given procfs, e.g
    thread-self/mountinfo, the mount table of the namespace the
    helper is in, relative to its root
    !*/
    let mut mountinfo = String::new();
    open_file(proc_dir.as_raw_fd(), path)
        .map(File::from)
        .and_then(|mut file| file.read_to_string(&mut mountinfo))
        .map_err(|error| format!("Failed to read the mount table {path}: {error}"))?;
    Ok(mountinfo::parse(&mountinfo))
}

fn enter_namespace(namespace: &OwnedFd) -> io::Result<()> {
    /*!
    Move the helper into the given mount namespace, its root and
    working directory become the root of the namespace
    !*/
    if unsafe { libc::setns(namespace.as_raw_fd(), libc::CLONE_NEWNS) } != 0 {
        return Err(io::Error::last_os_error())
    }
    Ok(())
}

fn wait_for(pid: libc::pid_t) -> io::Result<libc::c_int> {
    /*!
    Wait for the given child process to terminate and provide its
    wait status
    !*/
    let mut status: libc::c_int = 0;
    loop {
        if unsafe { libc::waitpid(pid, &mut status, 0) } >= 0 {
            return Ok(status)
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error)
        }
    }
}

fn try_wait_for(pid: libc::pid_t) -> io::Result<Option<libc::c_int>> {
    /*!
    Provide the wait status of the given child process if it has
    terminated, without waiting for it
    !*/
    let mut status: libc::c_int = 0;
    loop {
        match unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) } {
            0 => return Ok(None),
            result if result > 0 => return Ok(Some(status)),
            _ => {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error)
                }
            }
        }
    }
}

fn create_pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    /*!
    Create a pipe, closed on exec, and provide its read and its
    write end
    !*/
    let mut fds: [libc::c_int; 2] = [-1; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error())
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

fn clear_cloexec(descriptor: &OwnedFd) -> io::Result<()> {
    if unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_SETFD, 0) } != 0 {
        return Err(io::Error::last_os_error())
    }
    Ok(())
}

fn open_file(dirfd: RawFd, name: &str) -> io::Result<OwnedFd> {
    /*!
    Open the given file relative to dirfd for reading
    !*/
    let name = to_c_string(name)?;
    syscall_fd(unsafe {
        libc::openat(dirfd, name.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC)
    } as libc::c_long)
}

fn file_type(descriptor: &OwnedFd) -> io::Result<libc::mode_t> {
    Ok(fstat(descriptor)?.st_mode & libc::S_IFMT)
}

pub fn mount_root(
    caller: &Caller, rootfs: &str, layers: &[String], size: Option<&str>
) -> Result<(), String> {
    /*!
    Mount the root filesystem of a sandbox of the caller in a new
    mount namespace, the helper stays in it

    The rootfs is the lowest, read only layer of the overlay. The
    given layers are stacked on top of it in the given order, read
    only as well. The upper and the work directory live in a tmpfs
    of the given size, see flakes::tmpfs::get_tmpfs_size()
    !*/
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
    //    becomes visible. It is attached before the overlay is
    //    created: an overlay refuses upper, work and lower
    //    directories of a detached mount, and nothing can be
    //    attached into a detached mount (up to Linux 6.14)
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
    The caller has to be able to reach and read them itself, see
    open_layer(). The topmost layer is the first one of the
    returned layers
    !*/
    // The paths are resolved following symbolic links the caller may
    // control
    as_caller(caller, || open_layers(rootfs, layers))
}

pub fn as_caller<T>(
    caller: &Caller, action: impl FnOnce() -> Result<T, String>
) -> Result<T, String> {
    /*!
    Run the given action with the identity of the caller, see
    drop_privileges_temporarily(), and switch back to root
    afterwards. A path the caller may control must never be
    resolved with the privileges of root, unless the caller is
    root itself
    !*/
    let result = drop_privileges_temporarily(caller).and_then(|()| action());
    // Also after a partial switch, which leaves the helper with a
    // part of the identity of the caller
    restore_privileges()?;
    result
}

fn drop_privileges_temporarily(caller: &Caller) -> Result<(), String> {
    /*!
    Take on the groups, the group and the user ID of the caller as
    the effective identity, until restore_privileges() is called.
    The real and the saved user ID stay root, which allows the way
    back. This changes access checks, not what a process the helper
    starts may do, see drop_privileges() for that.

    Succeeds only if the effective identity is the one of the caller
    afterwards. restore_privileges() has to be called also if this
    failed, the switch may have happened in part
    !*/
    let groups = caller_groups(caller)?;
    let switched = unsafe {
        libc::setgroups(groups.len(), groups.as_ptr()) == 0
            && libc::setegid(caller.gid) == 0
            && libc::seteuid(caller.uid) == 0
    };
    if ! switched {
        return Err(format!(
            "Failed to switch to the caller: {}", io::Error::last_os_error()
        ))
    }
    if unsafe {
        libc::geteuid() != caller.uid || libc::getegid() != caller.gid
    } {
        return Err(
            "The effective identity is not the one of the caller".to_string()
        )
    }
    Ok(())
}

fn restore_privileges() -> Result<(), String> {
    /*!
    Switch back to root from drop_privileges_temporarily(). The
    groups of the caller are dropped, the helper continues as root
    without supplementary groups, which keeps them from taking part
    in any later access check
    !*/
    // The user ID first, changing the group and the groups needs
    // the privileges of root
    let restored = unsafe {
        libc::seteuid(0) == 0
            && libc::setegid(0) == 0
            && libc::setgroups(0, std::ptr::null()) == 0
    };
    if ! restored {
        return Err(format!(
            "Failed to switch back from the caller: {}",
            io::Error::last_os_error()
        ))
    }
    Ok(())
}

fn open_layers(
    rootfs: &str, layers: &[String]
) -> Result<(OwnedFd, Vec<OwnedFd>), String> {
    let rootfs_dir = open_layer(rootfs).map_err(
        |error| format!("Failed to open rootfs {rootfs}: {error}")
    )?;
    let mut layer_dirs: Vec<OwnedFd> = Vec::new();
    for layer in layers.iter().rev() {
        layer_dirs.push(open_layer(layer).map_err(
            |error| format!("Failed to open layer {layer}: {error}")
        )?);
    }
    Ok((rootfs_dir, layer_dirs))
}

fn open_layer(path: &str) -> io::Result<OwnedFd> {
    /*!
    Open the given directory of the rootfs or of a layer, see
    open_dir(), and check that the effective identity may read
    and search it, see check_access(). Called with the identity
    of the caller, the overlay is created with the privileges of
    root later on and must not provide a tree the caller cannot
    read itself
    !*/
    let directory = open_dir(libc::AT_FDCWD, path, true)?;
    check_access(&directory, libc::R_OK | libc::X_OK)?;
    Ok(directory)
}

pub fn drop_privileges(caller: &Caller) -> Result<(), String> {
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

fn check_access(descriptor: &OwnedFd, mode: libc::c_int) -> io::Result<()> {
    /*!
    Check that the effective identity has the given access to the
    file of the given descriptor, an O_PATH descriptor as well

    faccessat2() checks the file the descriptor refers to, through
    AT_EMPTY_PATH, the path is not resolved again. AT_EACCESS makes
    it use the effective IDs, the real user ID of the helper stays
    root, see drop_privileges_temporarily(). The system call is used
    directly, faccessat() of the C library falls back to compare the
    mode bits on its own when the kernel does not provide it
    !*/
    syscall_ok(unsafe {
        libc::syscall(
            libc::SYS_faccessat2, descriptor.as_raw_fd(), c"".as_ptr(), mode,
            libc::AT_EACCESS | libc::AT_EMPTY_PATH
        )
    })
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
    fn test_child_pid() {
        let report = "{\n    \"child-pid\": 4242,\n    \"mnt-namespace\": 4026532\n}\n";
        assert_eq!(Some(4242), child_pid(report));
        assert_eq!(None, child_pid("{}"));
    }

    #[test]
    fn test_select_mounts() {
        let host_mounts = mountinfo::parse("\
            22 1 0:21 / / rw shared:1 - btrfs /dev/vda2 rw\n\
            40 22 8:33 / /run/media/usb rw shared:5 - vfat /dev/sdc1 rw\n");
        // the mounts bwrap created for the sandbox: the root, a
        // recursive bind of /run of the host and two tmpfs
        let setup_mounts = mountinfo::parse("\
            100 99 0:50 / / rw - overlay overlay rw\n\
            101 100 0:21 /run /run rw master:1 - btrfs /dev/vda2 rw\n\
            107 100 0:22 / /tmp rw - tmpfs tmpfs rw\n\
            108 100 0:23 / /var/tmp rw - tmpfs tmpfs rw\n");
        let mounts = mountinfo::parse("\
            100 99 0:50 / / rw - overlay overlay rw\n\
            101 100 0:21 /run /run rw master:1 - btrfs /dev/vda2 rw\n\
            102 101 8:33 / /run/media/usb rw master:5 - vfat /dev/sdc1 rw\n\
            103 100 8:17 / /mnt rw - ext4 /dev/sdb1 rw\n\
            104 103 0:60 / /mnt/data/tmp rw - tmpfs tmpfs rw\n\
            105 107 8:18 / /tmp/disk rw - ext4 /dev/sdb2 rw\n\
            107 100 0:22 / /tmp rw - tmpfs tmpfs rw\n\
            108 100 8:19 / /srv/disk rw - ext4 /dev/sdb3 rw\n\
            109 100 8:33 / /media/usb rw master:5 - vfat /dev/sdc1 rw\n\
            110 101 8:33 / /run/media/usb rw - vfat /dev/sdc1 rw\n");
        let selected: Vec<u64> = select_mounts(&mounts, &setup_mounts, &host_mounts)
            .iter().map(|mount| mount.id).collect();
        // The mounts of bwrap stay in the sandbox. Of the mounts of the
        // application, /mnt/data/tmp is cloned along with /mnt. 108 got
        // the ID of the tmpfs on /var/tmp the application unmounted,
        // it is a mount of the application nevertheless. The USB stick
        // the host mounted meanwhile reached the sandbox as slave, also
        // at another location, and the application mounted it at the
        // location of the host again, it exists on the host already
        assert_eq!(vec![103, 105, 108], selected);
        // without mounts of the application nothing persists
        assert!(select_mounts(&setup_mounts, &setup_mounts, &host_mounts).is_empty());
    }

    #[test]
    fn test_reads_from() {
        let syscall = |number: libc::c_long, fd: u64| format!(
            "{number} 0x{fd:x} 0x7ffd5c4e8c27 0x1 0x0 0x0 0x0 0x7ffd5c4e8bf0 0x7f2a\n"
        );
        // the sandbox blocks on the read from the block pipe
        assert!(reads_from(&syscall(libc::SYS_read, 5), 5));
        assert!(reads_from(&syscall(libc::SYS_read, 10), 10));
        // a read from another descriptor, e.g the one bwrap waits on
        // for its parent, or another system call is not
        assert!(! reads_from(&syscall(libc::SYS_read, 4), 5));
        assert!(! reads_from(&syscall(libc::SYS_write, 5), 5));
        // the sandbox is not in a system call
        assert!(! reads_from("running\n", 5));
        assert!(! reads_from("-1 0x7ffd5c4e8bf0 0x7f2a\n", 5));
        assert!(! reads_from("", 5));
    }

    #[test]
    fn test_mount_point_components() {
        assert_eq!(
            Ok(vec!["mnt".to_string(), "my disk".to_string()]),
            mount_point_components(Path::new("/mnt/my disk"))
        );
        // the root of the host is never mounted over
        assert!(mount_point_components(Path::new("/")).is_err());
        assert!(mount_point_components(Path::new("mnt")).is_err());
        assert!(mount_point_components(Path::new("/mnt/../etc")).is_err());
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

    #[test]
    fn test_open_layers_access() {
        // The O_PATH descriptor of a directory which cannot be read
        // or searched is provided, the access check refuses it
        let tmp_dir = tempfile::tempdir().unwrap();
        let base = tmp_dir.path().to_str().unwrap();
        let layer = format!("{base}/layer");
        fs::create_dir(&layer).unwrap();
        let is_root = get_current_uid() == 0;
        for mode in [0o300, 0o600, 0o000] {
            fs::set_permissions(
                &layer, fs::Permissions::from_mode(mode)
            ).unwrap();
            assert!(open_dir(libc::AT_FDCWD, &layer, true).is_ok());
            assert_eq!(is_root, open_layers(&layer, &[]).is_ok(), "{mode:o}");
            assert_eq!(
                is_root, open_layers(base, &[layer.clone()]).is_ok(), "{mode:o}"
            );
        }
        fs::set_permissions(&layer, fs::Permissions::from_mode(0o500)).unwrap();
        assert!(open_layers(&layer, &[]).is_ok());
    }
}
