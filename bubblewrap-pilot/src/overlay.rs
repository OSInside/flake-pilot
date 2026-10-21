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

use flakes::config::get_sandbox_tmpfs_size;
use flakes::error::FlakeError;
use flakes::tmpfs::{get_tmpfs_size, host_memory};
use flakes::user::{User, chmod, mkdir};

use uzers::get_current_uid;

use std::ffi::CString;
use std::fs::{self, DirBuilder, File};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;

pub fn mount(
    rootfs: &str, layers: &[String], size: Option<&str>, user: User
) -> Result<String, FlakeError> {
    /*!
    Provide the root filesystem of the sandbox as overlay mount

    The rootfs of the flake is the lowest, read only layer of an
    overlay filesystem, the given layers are stacked on top of it.
    Its upper and work directory live in a tmpfs. Thus the rootfs
    on the host stays untouched and nothing written in the sandbox
    ends up on disk.

    None of the mounts is created on the host. The pilot moves into
    a user and mount namespace of its own first, which the sandbox
    is created in. The mounts only exist in this namespace, nobody
    on the host can see or access them, and they are gone along
    with everything written to them once the sandbox and the pilot
    have terminated. Mounting in a namespace of its own requires no
    privileges, the user of the sandbox creates them itself.

    The tmpfs is mounted on the directory of the user, which is
    created for this purpose and stays empty on the host. The
    setup of the user with the ID 1000 looks as follows in the
    namespace:

    /var/tmp/bwrap_1000/  <- tmpfs
       ├── merged         <- overlay mount of the rootfs
       ├── upper          <- upper directory of the overlay
       ├── work           <- work directory of the overlay
       ├── lower          <- read only overlay of the rootfs
       └── empty          <- second layer of the read only overlay

    The read only overlay of the rootfs is only created if layers
    are stacked on top of the rootfs. Layers are mostly part of the
    rootfs and an overlay refuses lower directories of which one
    contains another. The read only overlay provides the rootfs to
    the overlay on other directories.

    Each sandbox has a namespace of its own, sandboxes of the same
    user therefore do not share this setup although it is mounted
    on the same directory.

    The mounts of the host are locked in the namespace of a user.
    The kernel refuses an overlay of a directory with mount points
    below it, the overlay would reveal what they cover. Such a
    rootfs, e.g the root of the host, is mounted by the helper, see
    has_mounts_below()

    The merged directory is returned. It becomes the writable root
    filesystem bwrap creates the sandbox from
    !*/
    let size = get_tmpfs_size(
        size, get_sandbox_tmpfs_size().as_deref(),
        host_memory().map_err(setup_error)?
    ).map_err(|message| FlakeError::IOError {
        kind: "Invalid tmpfs size".to_string(),
        message
    })?;

    // 1. Create the mount point of the tmpfs
    let user_dir = create_user_dir(user)?;

    // 2. Move into the namespace of the sandbox
    enter_namespace()?;

    // 3. Open the rootfs and the layers. They are given to the
    //    overlay through descriptors, a path is passed to it in an
    //    option string in which characters like ':' and ',' are
    //    separators. They are opened in the namespace, the overlay
    //    refuses layers of the mounts of another namespace. The
    //    topmost layer is the first one of the lower directories
    let rootfs_dir = open_dir(rootfs)?;
    let mut layer_dirs: Vec<File> = Vec::new();
    for layer in layers.iter().rev() {
        layer_dirs.push(open_dir(layer)?);
    }

    // 4. Mount the tmpfs and create the directories of the setup
    let mut tmpfs_options = format!("mode={}", defaults::OVERLAY_DIR_MODE);
    if let Some(size) = size {
        tmpfs_options.push_str(&format!(",size={size}"));
    }
    mount_filesystem(defaults::TMPFS_TYPE, &user_dir, &tmpfs_options)?;
    let mut dir_names = vec![
        defaults::OVERLAY_MERGED_NAME,
        defaults::OVERLAY_UPPER_NAME,
        defaults::OVERLAY_WORK_NAME
    ];
    if ! layer_dirs.is_empty() {
        dir_names.push(defaults::OVERLAY_LOWER_NAME);
        dir_names.push(defaults::OVERLAY_EMPTY_NAME);
    }
    for name in dir_names {
        let directory = get_dir(name);
        DirBuilder::new().mode(0o700).create(&directory).map_err(
            |error| mount_error(&format!("Failed to create {directory}"), error)
        )?;
    }

    // 5. Mount the read only overlay of the rootfs if layers are
    //    stacked on top of it and the overlay of the layers
    let rootfs_layer = if layer_dirs.is_empty() {
        fd_path(&rootfs_dir)
    } else {
        let lower_dir = get_dir(defaults::OVERLAY_LOWER_NAME);
        mount_filesystem(defaults::OVERLAY_TYPE, &lower_dir, &format!(
            "userxattr,lowerdir={}:{}",
            get_dir(defaults::OVERLAY_EMPTY_NAME), fd_path(&rootfs_dir)
        ))?;
        lower_dir
    };
    let lowerdir: Vec<String> = layer_dirs.iter()
        .map(fd_path)
        .chain(std::iter::once(rootfs_layer))
        .collect();
    let merged_dir = get_dir(defaults::OVERLAY_MERGED_NAME);
    mount_filesystem(defaults::OVERLAY_TYPE, &merged_dir, &format!(
        "userxattr,lowerdir={},upperdir={},workdir={}",
        lowerdir.join(":"),
        get_dir(defaults::OVERLAY_UPPER_NAME),
        get_dir(defaults::OVERLAY_WORK_NAME)
    ))?;
    Ok(merged_dir)
}

pub fn create_user_dir(user: User) -> Result<String, FlakeError> {
    /*!
    Create the directory the overlay setup of the calling user is
    mounted on and provide its path

    The directory is created first, to not leave its permissions
    to the umask of the caller. It also closes the directories
    created by former versions to other users
    !*/
    let user_dir = get_user_dir();
    mkdir(&user_dir, defaults::OVERLAY_DIR_MODE, user)?;
    chmod(&user_dir, defaults::OVERLAY_DIR_MODE, user)?;
    Ok(user_dir)
}

pub fn has_mounts_below(
    rootfs: &str, layers: &[String]
) -> Result<bool, FlakeError> {
    /*!
    Check if there are mount points below the rootfs or one of the
    layers

    An overlay of such a directory cannot be mounted in the
    namespace of a user. The mounts of the host are locked in it,
    and the kernel refuses an overlay which would reveal what is
    covered by them. Such a root filesystem of a sandbox is mounted
    by the helper instead, see its sandbox-run command
    !*/
    let mountinfo = fs::read_to_string(defaults::PROC_MOUNTINFO).map_err(
        |error| mount_error(
            &format!("Failed to read {}", defaults::PROC_MOUNTINFO), error
        )
    )?;
    for directory in std::iter::once(rootfs).chain(layers.iter().map(String::as_str)) {
        let directory = fs::canonicalize(directory).map_err(
            |error| mount_error(&format!("Failed to resolve {directory}"), error)
        )?;
        if mounts_below(&directory, &mountinfo) {
            return Ok(true)
        }
    }
    Ok(false)
}

pub fn mounts_below(directory: &Path, mountinfo: &str) -> bool {
    /*!
    Check if the given mount table, in the format of
    /proc/self/mountinfo, has a mount point below the given
    directory. A mount on the directory itself does not count
    !*/
    mountinfo.lines()
        .filter_map(|line| line.split(' ').nth(4))
        .map(unescape_mount_point)
        .any(|mount_point| {
            let mount_point = Path::new(&mount_point);
            mount_point != directory && mount_point.starts_with(directory)
        })
}

fn unescape_mount_point(mount_point: &str) -> String {
    /*!
    Resolve the octal escapes of a mount point in the mount table,
    e.g \040 for a space
    !*/
    let bytes = mount_point.as_bytes();
    let mut unescaped = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let escape = bytes.get(index + 1..index + 4).filter(
            |digits| bytes[index] == b'\\'
                && digits.iter().all(|digit| (b'0'..=b'7').contains(digit))
        );
        match escape {
            Some(digits) => {
                unescaped.push(digits.iter().fold(
                    0_u8, |byte, digit| byte.wrapping_mul(8).wrapping_add(digit - b'0')
                ));
                index += 4
            },
            None => {
                unescaped.push(bytes[index]);
                index += 1
            }
        }
    }
    String::from_utf8_lossy(&unescaped).to_string()
}

pub fn get_user_dir() -> String {
    /*!
    Provide the directory the overlay setup of the calling user
    is mounted on

    The base directory of the setups is shared between all users
    of the system. Each of them therefore gets a directory of its
    own below it, named after its user ID, e.g /var/tmp/bwrap_1000.
    The directory belongs to the user and is not accessible for
    others, it is only used as mount point in the namespace of the
    sandbox
    !*/
    format!(
        "{}/{}_{}",
        defaults::OVERLAY_BASE_DIR,
        defaults::OVERLAY_USER_DIR_NAME,
        get_current_uid()
    )
}

pub fn get_dir(name: &str) -> String {
    /*!
    Provide the path of the given directory of the overlay setup,
    e.g /var/tmp/bwrap_1000/merged
    !*/
    format!("{}/{}", get_user_dir(), name)
}

fn enter_namespace() -> Result<(), FlakeError> {
    /*!
    Move the pilot into a user and mount namespace of its own

    The user and the group of the caller are the only ones mapped
    into the user namespace, as themselves. This grants the right
    to mount in the mount namespace, no other privileges. The
    mounts of the host are copied into the namespace. Mounts
    created in it are private and do not propagate to the host.

    A process can only enter a new user namespace as long as it
    has a single thread, the pilot does not start any
    !*/
    let uid = unsafe { libc::geteuid() };
    let gid = unsafe { libc::getegid() };
    if unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS) } != 0 {
        return Err(mount_error(
            "Failed to create the namespace of the sandbox",
            io::Error::last_os_error()
        ))
    }
    // The groups can only be mapped once setgroups is denied
    for (file, content) in [
        (defaults::PROC_SETGROUPS, "deny".to_string()),
        (defaults::PROC_UID_MAP, format!("{uid} {uid} 1")),
        (defaults::PROC_GID_MAP, format!("{gid} {gid} 1"))
    ] {
        fs::write(file, content).map_err(
            |error| mount_error(&format!("Failed to write {file}"), error)
        )?;
    }
    let root = to_c_string("/")?;
    if unsafe {
        libc::mount(
            std::ptr::null(), root.as_ptr(), std::ptr::null(),
            libc::MS_REC | libc::MS_PRIVATE, std::ptr::null()
        )
    } != 0 {
        return Err(mount_error(
            "Failed to make the mounts of the namespace private",
            io::Error::last_os_error()
        ))
    }
    Ok(())
}

fn mount_filesystem(
    fstype: &str, target: &str, options: &str
) -> Result<(), FlakeError> {
    /*!
    Mount a filesystem of the given type with the given options
    on the given directory, nosuid and nodev
    !*/
    let source = to_c_string(fstype)?;
    let c_target = to_c_string(target)?;
    let c_options = to_c_string(options)?;
    if unsafe {
        libc::mount(
            source.as_ptr(), c_target.as_ptr(), source.as_ptr(),
            libc::MS_NOSUID | libc::MS_NODEV,
            c_options.as_ptr() as *const libc::c_void
        )
    } != 0 {
        return Err(mount_error(
            &format!("Failed to mount {fstype} on {target}"),
            io::Error::last_os_error()
        ))
    }
    Ok(())
}

fn open_dir(path: &str) -> Result<File, FlakeError> {
    /*!
    Open the given directory as O_PATH descriptor
    !*/
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_DIRECTORY)
        .open(path)
        .map_err(|error| mount_error(&format!("Failed to open {path}"), error))
}

fn fd_path(directory: &File) -> String {
    /*!
    Provide the path the kernel resolves to the given descriptor
    !*/
    format!("/proc/self/fd/{}", directory.as_raw_fd())
}

fn to_c_string(value: &str) -> Result<CString, FlakeError> {
    CString::new(value).map_err(|_| FlakeError::IOError {
        kind: "Invalid path".to_string(),
        message: format!("{value:?} contains a NUL byte")
    })
}

fn mount_error(message: &str, error: io::Error) -> FlakeError {
    FlakeError::IOError {
        kind: "Sandbox setup failed".to_string(),
        message: format!("{message}: {error}")
    }
}

fn setup_error(message: String) -> FlakeError {
    FlakeError::IOError {
        kind: "Sandbox setup failed".to_string(),
        message
    }
}
