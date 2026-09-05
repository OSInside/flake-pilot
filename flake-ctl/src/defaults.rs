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
pub const PODMAN_PATH:&str =
    "/usr/bin/podman";
pub const FLAKE_TEMPLATE_CONTAINER:&str =
    "/etc/flakes/container-flake.yaml";
pub const FLAKE_TEMPLATE_FIRECRACKER:&str =
    "/etc/flakes/firecracker-flake.yaml";
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
pub const FIRECRACKER_SCI:&str =
    "/usr/lib/flake-pilot/sci";
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
pub const PODMAN_ENGINE:&str =
    "podman";
pub const FIRECRACKER_ENGINE:&str =
    "firecracker";
pub const FLAKE_LIST_COLUMNS:[&str; 5] =
    ["NAME", "ENGINE", "TARGET APP PATH", "HOST APP PATH", "CONFIG"];
pub const FLAKE_LIST_COLUMN_SPACING:&str =
    "  ";
pub const FLAKE_LIST_NO_VALUE:&str =
    "-";
pub const FLAKE_SHOW_COLUMNS:[&str; 6] =
    ["NAME", "USER", "ID", "STATUS", "IMAGE", "CONFIG"];
// Number of characters of the instance ID shown in the table
// format. Like podman does, the table shows the container ID
// abbreviated. The machine readable formats show it complete
pub const FLAKE_SHOW_ID_LEN: usize =
    12;
// File name extensions of the meta data files the pilots
// create for their instances
pub const PODMAN_ID_EXTENSION:&str =
    "cid";
pub const FIRECRACKER_ID_EXTENSION:&str =
    "vmid";
// Name of the process a firecracker VM ID file points to
pub const FIRECRACKER_PROCESS_NAME:&str =
    "firecracker";
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
pub const IPTABLES_TOOL:&str =
    "iptables";
pub const IP_TOOL:&str =
    "ip";
pub const NFS_EXPORTFS_TOOL:&str =
    "exportfs";
pub const NFS_EXPORTS_FILE:&str =
    "/etc/exports";
pub const NFS_SERVER_SERVICE:&str =
    "nfs-server";
pub const NFS_EXPORT_OPTIONS:&str =
    "rw,sync,no_subtree_check,no_root_squash,insecure";
// Kernel commandline option sci reads the NFS volumes of a VM
// from. The option provides all volumes of an instance as a
// list of NAME_OR_IP:HOST_PATH:GUEST_PATH elements
pub const NFS_VOLUME_BOOT_ARG:&str =
    "nfs";
pub const NFS_VOLUME_DELIMITER:char =
    ',';
// Kernel switch to turn the host into a router. Required to
// forward the traffic of a VM from its TUN/TAP device to the
// outgoing interface of the host
pub const PROC_IP_FORWARD:&str =
    "/proc/sys/net/ipv4/ip_forward";
// Record of the host network setup created by
// 'flake-ctl firecracker network init'
pub const NETWORK_CONFIG:&str =
    "/etc/flakes/network.yaml";
// User specific location of that record, relative to the
// home directory of the user calling the program
pub const NETWORK_CONFIG_USER:&str =
    ".config/flakes/firecracker/network.yaml";
// Setup of the private network between the host and the VMs.
// The addresses only exist between the TAP device of an instance
// and the VM behind it. The traffic to the outside world is
// masqueraded and therefore leaves the host with the address of
// the outgoing interface.
//
// The network to use is selected when the host setup is created
// and is recorded along with it. The preferred network is taken
// if the host is not connected to it already, in all other cases
// a free network from the ranges below is taken
pub const NETWORK_PREFERRED:&str =
    "172.16.0.0";
// Ranges of private networks a free network is searched in if
// the preferred network conflicts with a network of the host.
// Each entry provides the first network of the range and the
// number of networks in it. The networks of a range follow each
// other in steps of NETWORK_PREFIX_LEN, e.g 172.16.0.0 is
// followed by 172.16.1.0. The ranges are searched in the order
// they are listed here
pub const NETWORK_RANGES:&[(&str, u32)] = &[
    // 172.16.0.0 ... 172.31.255.0
    ("172.16.0.0", 4096),
    // 192.168.0.0 ... 192.168.255.0
    ("192.168.0.0", 256),
    // 10.0.0.0 ... 10.255.255.0
    ("10.0.0.0", 65536)
];
pub const NETWORK_PREFIX_LEN:u8 =
    24;
// Prefix length of the host route which connects the address of
// a single VM instance with the TAP device of that instance
pub const NETWORK_HOST_PREFIX_LEN:u8 =
    32;
pub const NETWORK_DNS:&str =
    "8.8.8.8";
// Name of the network interface inside of the VM. Firecracker
// provides exactly one interface to the guest
pub const NETWORK_GUEST_INTERFACE:&str =
    "eth0";
pub const TEMP_DIR:&str =
    "/var/tmp";
pub const TEMP_PREFIX:&str =
    "flake-ctl-";
