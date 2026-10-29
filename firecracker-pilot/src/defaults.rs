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
// The location of the storage volumes and the name of the vsock
// sockets are shared with flake-ctl and are therefore maintained
// in the common area
pub use flakes::defaults::{
    FIRECRACKER_OVERLAY_DIR, FIRECRACKER_STORAGE_DIR,
    FIRECRACKER_STORAGE_EXTENSION, FIRECRACKER_VSOCK_PREFIX
};
pub const FIRECRACKER_TEMPLATE:&str =
    "/etc/flakes/firecracker.json";
pub const FIRECRACKER_VSOCK_PORT_START: u32 = 49200;
// Location the kernel reports the network interfaces of the
// host below. A TAP device created as part of the host setup
// with 'flake-ctl firecracker network add' shows up here
pub const SYS_CLASS_NET: &str =
    "/sys/class/net";
// Tool maintained in the common area: iptables looks up the
// netfilter rules of the host setup created with 'flake-ctl
// firecracker network init'. It is called with the privileges of
// the caller and can only read them if that is root
pub use flakes::defaults::IPTABLES_TOOL;
// Kernel commandline option which provides the NFS volumes of a
// VM. The option is written by 'flake-ctl firecracker volume add'
// and is read by sci inside of the guest. It provides all volumes
// of an instance as a list of NAME_OR_IP:HOST_PATH:GUEST_PATH
// elements
pub const NFS_VOLUME_BOOT_ARG: &str =
    "nfs";
pub const NFS_VOLUME_DELIMITER: char =
    ',';
// Kernel commandline options which tell sci inside of the guest to
// keep the instance running for further commands (resume) and to
// receive the command through a vsock (force_vsock). The pilot sets
// them from the runtime section it acts upon. Registrations of
// former versions of 'flake-ctl firecracker register' also contain
// them in the boot_args, these are not used
pub const SCI_RESUME_BOOT_ARG: &str =
    "sci_resume";
pub const SCI_FORCE_VSOCK_BOOT_ARG: &str =
    "sci_force_vsock";
// Kernel commandline option which tells sci inside of the guest the
// name of the user to call the application as. The pilot sets it
// from the user setting of the runtime section
pub const SCI_USER_BOOT_ARG: &str =
    "sci_user";
// Statistics of the NFS server of the kernel. The record shows up
// when the nfsd module is loaded and provides the number of
// server threads which is zero as long as no server was started
pub const NFS_SERVER_STAT: &str =
    "/proc/net/rpc/nfsd";
// Name of the systemd service which runs the NFS server of the
// host. The volumes of a VM are exported by this server, it is
// started by 'flake-ctl firecracker volume add'
pub const NFS_SERVER_SERVICE: &str =
    "nfs-server";
pub const TERM_NAME_MAX_LEN: usize = 32;
// Character which ends the input of a command in the guest, the
// EOF character of a terminal (Ctrl-D). It is sent once the input
// of the caller has ended
pub const TERM_EOF: u8 = 0x04;
// Port of the instance the window size of the caller's terminal
// is sent to whenever it got resized
pub const TERM_RESIZE_PORT: u32 = 53;
// Port used to address the session which runs on the console of
// the instance. The console is not connected through a vsock and
// therefore has no port of its own
pub const TERM_CONSOLE_PORT: u32 = 0;
// Port the instance sends the exit status of the command to, which
// was run on the console of the instance. A command called through
// a vsock sends its exit status to the port the pilot listens on
// for this command
pub const EXIT_STATUS_PORT: u32 = 54;
// Time to wait for the exit status of a command called through a
// vsock once its output has ended
pub const EXIT_STATUS_TIMEOUT_MSEC: u64 = 1000;
// If standard output and standard error of the caller are not the
// same terminal, the standard error of the command is received on
// a port of its own. It is the port of the command's session with
// this bit set. The console of the instance uses TERM_CONSOLE_PORT
// for this. Session ports which already have the bit set, e.g one
// chosen through %port, get no such channel
pub const STDERR_PORT_FLAG: u32 = 0x8000_0000;
// Time to wait for more data on the standard error channel of a
// command once its session is over
pub const STDERR_DRAIN_TIMEOUT_MSEC: u64 = 1000;
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
