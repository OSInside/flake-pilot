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
pub const VAR_EXPANSION_LIMIT: i32 = 10;
// Name and path of the program which creates the sandbox
pub const BWRAP: &str = "bwrap";
pub use flakes::defaults::BWRAP_PATH;
// Name of the process of a sandbox which is created through the
// helper, see overlay::has_mounts_below()
pub use flakes::defaults::SUDO_PROCESS_NAME;
pub const PROC_DIR: &str = "/proc";
// File name extension of the meta data files the pilot
// creates for its instances
pub const SANDBOX_ID_EXTENSION: &str = "bwrapid";
// Contents of the meta data file of an instance which was
// created but is not started yet
pub const SANDBOX_NOT_STARTED: &str = "0";
// Option which mounts the root filesystem of the sandbox. The root
// is the overlay of the rootfs created in the namespace of the
// sandbox, it is writable and keeps what is written to it in a
// tmpfs which is gone along with the sandbox
pub const BWRAP_BIND_OPTION: &str = "--bind";
// Option which stacks another layer on top of the rootfs. A flake
// can configure it to add layers to the root of the sandbox. The
// layers are part of the overlay created by the pilot or the
// helper and are not passed to bwrap
pub const BWRAP_OVERLAY_SRC_OPTION: &str = "--overlay-src";
// Option which selects the working directory in the sandbox
pub const BWRAP_CHDIR_OPTION: &str = "--chdir";
// Mount point of the root filesystem of the sandbox
pub const SANDBOX_ROOT: &str = "/";
// Name of the variable which provides the root filesystem of
// the sandbox. It resolves to the overlay mount of the rootfs
// in the namespace of the sandbox and can be referenced as
// %OVERLAYROOT in the options of the flake configuration
pub const OVERLAY_ROOT_VAR: &str = "OVERLAYROOT";
// Directories of the overlay setup of the sandboxes. The setup is
// mounted by the helper for a rootfs with mount points below it
// and is therefore maintained along with it in the common area
pub use flakes::defaults::{
    OVERLAY_BASE_DIR, OVERLAY_USER_DIR_NAME, OVERLAY_MERGED_NAME,
    OVERLAY_UPPER_NAME, OVERLAY_WORK_NAME, OVERLAY_LOWER_NAME,
    OVERLAY_EMPTY_NAME
};
// Permissions of the directories of the overlay setup. Nobody but
// the user of the sandbox has access to them
pub const OVERLAY_DIR_MODE: &str = "700";
// Filesystem types of the overlay setup
pub const TMPFS_TYPE: &str = "tmpfs";
pub const OVERLAY_TYPE: &str = "overlay";
// Files to setup the user namespace of the sandbox with
pub const PROC_SETGROUPS: &str = "/proc/self/setgroups";
pub const PROC_UID_MAP: &str = "/proc/self/uid_map";
pub const PROC_GID_MAP: &str = "/proc/self/gid_map";
// Mount table of the pilot, to find mount points below the rootfs
pub const PROC_MOUNTINFO: &str = "/proc/self/mountinfo";
// Pilot option which requests the size of the tmpfs which keeps the
// writable layer of the sandbox, e.g %tmpfs_size:2G. A size above
// the limit configured in /etc/flakes.yml is refused
pub const PILOT_TMPFS_SIZE_OPTION: &str = "%tmpfs_size";
// Pilot option which selects the working directory of the
// application in the sandbox
pub const PILOT_CHDIR_OPTION: &str = "%chdir";
// Working directory of the application if neither the flake
// configuration nor the caller provides one
pub const SANDBOX_WORKDIR: &str = "/";
// Options used to create the sandbox if the flake
// configuration provides none. The application runs in a session
// of its own, which keeps it from injecting input into the terminal
// of the caller, with its own IPC and host name namespace and with
// an environment of its own, see SANDBOX_ENVIRONMENT
pub const BWRAP_OPTIONS: [&str; 9] = [
    "--dev /dev",
    "--proc /proc",
    "--tmpfs /tmp",
    "--unshare-pid",
    "--unshare-ipc",
    "--unshare-uts",
    "--new-session",
    "--die-with-parent",
    "--clearenv"
];
// Environment variables of the caller the application gets in an
// environment cleared by --clearenv, unless the flake sets them
// itself with --setenv. A variable which is not set for the caller
// is not set either
pub const SANDBOX_ENVIRONMENT: [&str; 8] = [
    "PATH", "HOME", "USER", "LOGNAME", "TERM", "LANG", "LC_ALL", "TZ"
];
pub const BWRAP_CLEARENV_OPTION: &str = "--clearenv";
pub const BWRAP_SETENV_OPTION: &str = "--setenv";
