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
pub const PODMAN_PILOT: &str =
    "/usr/bin/podman-pilot";
pub const FIRECRACKER_PILOT: &str =
    "/usr/bin/firecracker-pilot";
pub const BUBBLEWRAP_PILOT: &str =
    "/usr/bin/bubblewrap-pilot";
pub const FLAKE_TEMPLATE_CONTAINER:&str =
    "/etc/flakes/container-flake.yaml";
pub const FLAKE_TEMPLATE_FIRECRACKER:&str =
    "/etc/flakes/firecracker-flake.yaml";
pub const FLAKE_TEMPLATE_BUBBLEWRAP:&str =
    "/etc/flakes/bubblewrap-flake.yaml";
// Packages providing the pilot of an engine including
// its registration template. Used to tell the user which
// package to install if a template is missing
pub const FLAKE_PACKAGE_CONTAINER:&str =
    "flake-pilot-podman";
pub const FLAKE_PACKAGE_FIRECRACKER:&str =
    "flake-pilot-firecracker";
pub const FLAKE_PACKAGE_BUBBLEWRAP:&str =
    "flake-pilot-bubblewrap";
pub const FIRECRACKER_REGISTRY_DIR:&str =
    "/var/lib/firecracker";
// Name of the firecracker registry inside of the flakes
// directory of the calling user. Used in user mode only
pub const FIRECRACKER_REGISTRY_NAME:&str =
    "firecracker";
pub const FIRECRACKER_IMAGES_NAME:&str =
    "images";
pub const FIRECRACKER_STORAGE_NAME:&str =
    "storage";
pub const FIRECRACKER_INITRD_NAME:&str =
    "initrd";
pub const FIRECRACKER_KERNEL_NAME:&str =
    "kernel";
pub const FIRECRACKER_ROOTFS_NAME:&str =
    "rootfs";
// Name of the file that keeps the checksum record fetched from
// the image source. A pull of an image that is already in the
// registry compares against this record to find out whether the
// registered image is still up to date
pub const FIRECRACKER_SOURCE_CHECKSUM_NAME:&str =
    "source_checksum";
// Name of the podman setup directory inside of the flakes
// directory of the calling user. Used in user mode only
pub const PODMAN_REGISTRY_NAME:&str =
    "podman";
pub const PODMAN_STORAGE_CONF_NAME:&str =
    "storage.conf";
pub const PODMAN_STORAGE_NAME:&str =
    "storage";
pub const PODMAN_STORAGE_RUNROOT_NAME:&str =
    "runroot";
pub const PODMAN_STORAGE_DRIVER:&str =
    "overlay";
// Prefix of the name of the container instance created by
// 'flake-ctl podman export' to read the file system of a
// container from. The name is completed with the process ID of
// the running program such that exports running at the same
// time do not use the same instance name
pub const PODMAN_EXPORT_NAME_PREFIX:&str =
    "flake-ctl-export-";
// Prefix of the workspace used by 'flake-ctl podman export --oci'
// to unpack an OCI tarball. The workspace is created next to the
// target directory and is deleted after the export
pub const OCI_UNPACK_NAME_PREFIX:&str =
    ".flake-ctl-oci-";
// Markers of the OCI image layer format. A whiteout file deletes
// the file of the same name without the prefix from the layers
// below, the opaque whiteout deletes all contents of its directory
pub const OCI_WHITEOUT_PREFIX:&str =
    ".wh.";
pub const OCI_WHITEOUT_OPAQUE:&str =
    ".wh..wh..opq";
// Limits applied when reading an OCI tarball
pub const OCI_MAX_INDEX_DEPTH: usize = 8;
pub const OCI_MAX_SYMLINKS: usize = 40;
// Name of the process which keeps the container instance of a
// resume flake in running state. podman-pilot creates such an
// instance with a sleep entry point which allows to call the
// application in it multiple times. The container only stops
// if that process is gone
pub const PODMAN_RESUME_PROCESS_NAME:&str =
    "sleep";
pub const PODMAN_ENGINE:&str =
    "podman";
pub const FIRECRACKER_ENGINE:&str =
    "firecracker";
pub const BUBBLEWRAP_ENGINE:&str =
    "bubblewrap";
pub const FLAKE_LIST_COLUMNS:[&str; 5] =
    ["NAME", "ENGINE", "TARGET APP PATH", "HOST APP PATH", "CONFIG"];
pub const FLAKE_LIST_COLUMN_SPACING:&str =
    "  ";
pub const FLAKE_LIST_NO_VALUE:&str =
    "-";
pub const FLAKE_SHOW_COLUMNS:[&str; 6] =
    ["NAME", "USER", "ID", "STATUS", "IMAGE", "CONFIG"];
// Columns of the show command of the firecracker engine. A VM
// instance provides the network, the NFS volumes and the storage
// volume attached to it in addition to the information all
// instances provide
pub const FLAKE_SHOW_VM_COLUMNS:[&str; 11] = [
    "NAME", "USER", "ID", "STATUS", "IMAGE", "CONFIG",
    "ADDRESS", "TAP", "VOLUMES", "STORAGE", "SIZE"
];
// Number of characters of the instance ID shown in the table
// format. Like podman does, the table shows the container ID
// abbreviated. The machine readable formats show it complete
pub const FLAKE_SHOW_ID_LEN: usize =
    12;
// File name extensions of the meta data files the pilots
// create for their instances. The extension of the podman
// engine is shared with podman-pilot and is therefore
// maintained in the common area
pub use flakes::defaults::PODMAN_ID_EXTENSION;
// Additional image store of the system wide podman setup, the
// common podman calls write into it
pub use flakes::defaults::PODMAN_ADDON_STORE;
// Location of the storage volumes and name of the vsock sockets
// firecracker-pilot creates for VM instances, shared with
// firecracker-pilot and therefore maintained in the common area
pub use flakes::defaults::{
    FIRECRACKER_OVERLAY_DIR, FIRECRACKER_STORAGE_DIR,
    FIRECRACKER_STORAGE_EXTENSION, FIRECRACKER_VSOCK_PREFIX
};
pub const FIRECRACKER_ID_EXTENSION:&str =
    "vmid";
pub const BUBBLEWRAP_ID_EXTENSION:&str =
    "bwrapid";
// Name of the process a firecracker VM ID file points to
pub const FIRECRACKER_PROCESS_NAME:&str =
    "firecracker";
// Names of the processes a bubblewrap sandbox ID file points to.
// Name of the process which runs the sandbox, and of the sudo
// process the helper runs in for a sandbox it creates
pub const BUBBLEWRAP_PROCESS_NAME:&str =
    "bwrap";
pub use flakes::defaults::SUDO_PROCESS_NAME;
pub const PROC_DIR:&str =
    "/proc";
pub const INSTANCE_RUNNING:&str =
    "running";
pub const INSTANCE_STOPPED:&str =
    "stopped";
pub const INSTANCE_UNKNOWN:&str =
    "unknown";
pub const SHA256_TOOL:&str =
    "sha256sum";
pub const TAR_TOOL:&str =
    "tar";
// Locations used by 'firecracker build' inside of the kiwi
// container. The image description and the build target
// directory are mounted there, the box cache below the home
// directory of the calling user is mounted to the box cache
// of root in the container
pub const KIWI_DESCRIPTION_DIR:&str =
    "/description";
pub const KIWI_TARGET_DIR:&str =
    "/result";
pub const KIWI_BOXES_NAME:&str =
    ".kiwi_boxes";
pub const KIWI_BOXES_DIR:&str =
    "/root/.kiwi_boxes";
// Exit code of the build in the box, stored in the target
// directory by the box
pub const KIWI_RESULT_CODE_NAME:&str =
    "result.code";
// Log of the build in the box, stored in the target directory
// by the box. It is kept next to the outfile of the build
pub const KIWI_RESULT_LOG_NAME:&str =
    "result.log";
// Result file of a kiwi build in a box container, the key of
// the KIS archive in there names the file the build has produced.
// A build in a box VM provides only the result bundle
pub const KIWI_RESULT_NAME:&str =
    "kiwi.result.json";
pub const KIWI_RESULT_KIS_ARCHIVE:&str =
    "kis_archive";
// Program used to kill a process inside of a container. It is
// called through 'podman exec' and is therefore expected to be
// part of the container, like the sleep program it kills
pub const KILL_TOOL:&str =
    "kill";
// Programs given by their absolute path, maintained in the
// common area along with the other programs flake-pilot calls
pub use flakes::defaults::{IP_TOOL, MV_TOOL};
// Setup of the host network and the NFS exports. It is created by
// the helper, flake-ctl reads it and is maintained along with the
// helper in the common area
pub use flakes::defaults::{
    NETWORK_STATE, NETWORK_PREFIX_LEN, NETWORK_DNS, NETWORK_RANGES,
    NETWORK_PREFERRED
};
// Kernel commandline option sci reads the NFS volumes of a VM
// from. The option provides all volumes of an instance as a
// list of NAME_OR_IP:HOST_PATH:GUEST_PATH elements
pub const NFS_VOLUME_BOOT_ARG:&str =
    "nfs";
pub const NFS_VOLUME_DELIMITER:char =
    ',';
// Name of the network interface inside of the VM. Firecracker
// provides exactly one interface to the guest
pub const NETWORK_GUEST_INTERFACE:&str =
    "eth0";
pub const TEMP_DIR:&str =
    "/var/tmp";
pub const TEMP_PREFIX:&str =
    "flake-ctl-";
