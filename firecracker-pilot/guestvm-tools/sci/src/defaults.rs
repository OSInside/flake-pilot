//
// Copyright (c) 2022 Elektrobit Automotive GmbH
// Copyright (c) 2023 Marcus Schäfer
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
use std::env;

pub const PROMPT: &str = "\\[\\]\\u@\\h: > ";
pub const SSHD: &str = "/usr/sbin/sshd";
pub const SWITCH_ROOT: &str = "/sbin/switch_root";
// Path of sci in the guest. sci calls itself through the switch
// root to become the init process of the overlay root
pub const SCI: &str = "/usr/sbin/sci";
// Marker in the environment of sci which tells that the switch
// root into the overlay was done already
pub const OVERLAY_SWITCHED: &str = "sci_overlay_switched";
pub const OVERLAY_ROOT: &str = "/overlayroot/rootfs";
pub const OVERLAY_UPPER: &str = "/overlayroot/rootfs_upper";
pub const OVERLAY_WORK: &str = "/overlayroot/rootfs_work";
pub const PROBE_MODULE: &str = "/sbin/modprobe";
// Mount table of the kernel
pub const PROC_MOUNTS: &str = "/proc/self/mounts";
pub const MOUNT_TOOL: &str = "mount";
// Filesystem type and separator of the volume specification
// given in the nfs=... cmdline variable
pub const NFS_FSTYPE: &str = "nfs";
pub const NFS_VOLUME_DELIMITER: char = ',';
pub const SYSTEMD_NETWORK_RESOLV_CONF: &str = "/run/systemd/resolve/resolv.conf";
pub const VM_QUIT: &str = "sci_quit";
pub const VHOST_TRANSPORT: &str = "vmw_vsock_virtio_transport";
pub const TERM_TYPE: &str = "xterm";
pub const TERM_TYPE_FALLBACK: [&str; 3] = ["xterm", "vt100", "linux"];
pub const TERMINFO_DIRS: [&str; 3] = [
    "/etc/terminfo", "/lib/terminfo", "/usr/share/terminfo"
];
pub const TERM_LINES: u16 = 24;
pub const TERM_COLUMNS: u16 = 80;
// Port the caller sends the window size of its terminal to
// whenever it got resized
pub const TERM_RESIZE_PORT: u32 = 53;
// Port of the caller which owns the console of the instance.
// The console is not connected through a vsock and therefore
// has no port of its own
pub const TERM_CONSOLE_PORT: u32 = 0;
pub const VM_PORT: u32 = 52;
pub const GUEST_CID: u32 = 3;
pub const RETRIES: u32 =
    60;
pub const VM_WAIT_TIMEOUT_MSEC: u64 =
    1000;
pub const REAP_INTERVAL_MSEC: u64 =
    250;

pub fn debug(message: &str) {
    if env::var("PILOT_DEBUG").is_ok() {
        debug!("{message}")
    };
}
