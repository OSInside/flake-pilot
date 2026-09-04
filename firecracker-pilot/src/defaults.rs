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
pub const FIRECRACKER: &str =
    "firecracker";
pub const IMAGE_ROOT: &str =
    "image";
pub const IMAGE_OVERLAY: &str =
    "overlayroot";
pub const OVERLAY_ROOT: &str =
    "overlayroot/rootfs";
pub const OVERLAY_UPPER: &str =
    "overlayroot/rootfs_upper";
pub const OVERLAY_WORK: &str =
    "overlayroot/rootfs_work";
pub const FIRECRACKER_OVERLAY_DIR:&str =
    ".config/flakes/firecracker/storage";
pub const FIRECRACKER_STORAGE_DIR:&str =
    "storage";
pub const FIRECRACKER_TEMPLATE:&str =
    "/etc/flakes/firecracker.json";
pub const FIRECRACKER_VSOCK_PREFIX: &str =
    "sci_cmd_";
pub const FIRECRACKER_VSOCK_PORT_START: u32 = 49200;
pub const GC_THRESHOLD: usize = 1;
pub const TERM_NAME_MAX_LEN: usize = 32;
// Port of the instance the window size of the caller's terminal
// is sent to whenever it got resized
pub const TERM_RESIZE_PORT: u32 = 53;
// Port used to address the session which runs on the console of
// the instance. The console is not connected through a vsock and
// therefore has no port of its own
pub const TERM_CONSOLE_PORT: u32 = 0;
// Interval to check the window size of the caller's terminal for
pub const TERM_RESIZE_POLL_MSEC: u64 = 250;
// Time to wait for the instance to take the new window size
pub const TERM_RESIZE_TIMEOUT_MSEC: u64 = 200;
pub const VM_CID: u32 = 3;
pub const VM_PORT: u32 =
    52;
pub const RETRIES: u32 =
    60;
pub const VM_WAIT_TIMEOUT_MSEC: u64 =
    1000;
