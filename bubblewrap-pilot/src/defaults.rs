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
pub const BWRAP_PATH: &str = "/usr/bin/bwrap";
pub const PROC_DIR: &str = "/proc";
// File name extension of the meta data files the pilot
// creates for its instances
pub const SANDBOX_ID_EXTENSION: &str = "bwrapid";
// Contents of the meta data file of an instance which was
// created but is not started yet
pub const SANDBOX_NOT_STARTED: &str = "0";
// Options which mount the root filesystem of the sandbox. The
// root is the overlay of the rootfs created on the host. It is
// provided to bwrap as the read only layer of another overlay
// which makes the root writable inside of the sandbox
pub const BWRAP_OVERLAY_SRC_OPTION: &str = "--overlay-src";
pub const BWRAP_OVERLAY_OPTION: &str = "--overlay";
// Option which selects the working directory in the sandbox
pub const BWRAP_CHDIR_OPTION: &str = "--chdir";
// Mount point of the root filesystem of the sandbox
pub const SANDBOX_ROOT: &str = "/";
// Name of the variable which provides the root filesystem of
// the sandbox. It resolves to the overlay mount of the rootfs
// on the host and can be referenced as %OVERLAYROOT in the
// options of the flake configuration
pub const OVERLAY_ROOT_VAR: &str = "OVERLAYROOT";
// Directories of the overlay setups of the sandboxes. The setup
// is mounted by the helper and is therefore maintained along with
// it in the common area
pub use flakes::defaults::{
    OVERLAY_BASE_DIR, OVERLAY_USER_DIR_NAME, OVERLAY_MERGED_NAME,
    OVERLAY_TMPFS_NAME, OVERLAY_RW_NAME, OVERLAY_WORK_NAME,
    OVERLAY_UPPER_NAME
};
// Permissions of the directories of the overlay setup
pub const OVERLAY_DIR_MODE: &str = "755";
// Pilot option which selects the working directory of the
// application in the sandbox
pub const PILOT_CHDIR_OPTION: &str = "%chdir";
// Working directory of the application if neither the flake
// configuration nor the caller provides one
pub const SANDBOX_WORKDIR: &str = "/";
// Options used to create the sandbox if the flake
// configuration provides none
pub const BWRAP_OPTIONS: [&str; 5] = [
    "--dev /dev",
    "--proc /proc",
    "--tmpfs /tmp",
    "--unshare-pid",
    "--die-with-parent"
];
