//
// Copyright (c) 2023 SUSE Software Solutions Germany GmbH
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
pub const FLAKES_CONFIG: &str = "/etc/flakes.yml";
pub const FLAKES_DIR: &str = "/usr/share/flakes";
// Local adaptions of system wide registrations, read last
pub const FLAKES_CONFIG_DIR: &str = "/etc/flakes";
// User specific locations, relative to the home directory
// of the user calling the program
pub const FLAKES_CONFIG_USER: &str = ".config/flakes.yml";
pub const FLAKES_DIR_USER: &str = ".config/flakes";
pub const PODMAN_IDS_DIR: &str = "/tmp/flakes";
pub const FIRECRACKER_IDS_DIR: &str = "/tmp/flakes";
pub const BUBBLEWRAP_IDS_DIR: &str = "/tmp/flakes";
// Permissions of the meta data directories shared by the pilots
// of all users. Like /tmp everybody can create an entry in them
// but only delete the own ones. The directories are not created
// by the pilots, see IO::private_dir
pub const META_DIR_MODE: &str = "1777";
// Configuration which lets systemd-tmpfiles create the default
// meta data directory, provided by the flake-pilot package
pub const META_DIR_TMPFILES_CONF: &str =
    "/usr/lib/tmpfiles.d/flake-pilot.conf";
pub const PODMAN_STORAGE_CONF: &str = "/etc/flakes/storage.conf";
// Additional read-only image store of the system wide podman
// setup, see additionalimagestores in PODMAN_STORAGE_CONF. Images
// are only written into it on request of flake-ctl
pub const PODMAN_ADDON_STORE: &str = "/usr/lib/podman/storage";
pub const PODMAN_PATH: &str = "/usr/bin/podman";
// Container which provides kiwi to build images through
// kiwi system boxbuild, see the kiwi_container setting
pub const KIWI_CONTAINER: &str = "public.ecr.aws/b9k1j9y6/kiwi:latest";
// The helper performs every action of flake-pilot which needs
// root privileges. It belongs to root, provides a fixed set of
// commands and validates their arguments. A caller who is not
// root runs it through sudo, which is the only program flake-pilot
// calls through sudo. See the flake-pilot-helper crate
pub const HELPER: &str = "/usr/libexec/flake-pilot/flake-pilot-helper";
pub const SUDO_TOOL: &str = "/usr/bin/sudo";
// Name of the sudo process. bubblewrap-pilot records it as the
// process of a sandbox which is created through the helper
pub const SUDO_PROCESS_NAME: &str = "sudo";
// Program which creates the bubblewrap sandboxes, called by
// bubblewrap-pilot
pub const BWRAP_PATH: &str = "/usr/bin/bwrap";
// The pilots of the engines, called by the helper on behalf of
// a user, see the podman-pilot and bubblewrap-pilot commands of
// the helper
pub const PODMAN_PILOT: &str = "/usr/bin/podman-pilot";
pub const BUBBLEWRAP_PILOT: &str = "/usr/bin/bubblewrap-pilot";
// Environment variable the helper hands the user ID of its caller
// to a pilot it runs as root on behalf of that user. It is only
// read by a process which runs as root
pub const CALLER_UID_VAR: &str = "FLAKE_PILOT_CALLER_UID";
// Prefix of the private meta data directory a pilot running as
// root keeps the instances a user started of system wide flakes
// in, e.g /tmp/flakes/system_1000. It belongs to root
pub const SYSTEM_META_DIR_PREFIX: &str = "system_";
// Programs called by root, by the helper or by a pilot the helper
// started. They are given by their absolute path, a program looked
// up in a search path could be replaced. The locations are the ones
// of a merged /usr as it is used by all distributions flake-pilot
// is packaged for
pub const MV_TOOL: &str = "/usr/bin/mv";
pub const RSYNC_TOOL: &str = "/usr/bin/rsync";
pub const SYSTEMCTL_TOOL: &str = "/usr/bin/systemctl";
pub const IPTABLES_TOOL: &str = "/usr/sbin/iptables";
pub const IP_TOOL: &str = "/usr/sbin/ip";
pub const SYSCTL_TOOL: &str = "/usr/sbin/sysctl";
pub const NFS_EXPORTFS_TOOL: &str = "/usr/sbin/exportfs";
// File name extension of the meta data files podman-pilot
// creates for its container instances
pub const PODMAN_ID_EXTENSION: &str = "cid";
// Options used for all rsync calls.
pub const RSYNC_OPTIONS: [&str; 2] = ["-a", "-v"];
// Prefix of the TAP device a VM instance is connected to
pub const TAP_DEVICE_PREFIX: &str = "tap-";
// Chains of the host firewall setup created by 'flake-ctl
// firecracker network init'. All rules of the setup live in
// them, each chain is entered by one jump rule from the builtin
// chain of the same purpose. This keeps the setup apart from
// the other rules of the host and allows to delete it as a whole
pub const FIREWALL_NAT_CHAIN: &str = "FLAKE_PILOT_NAT";
pub const FIREWALL_FORWARD_CHAIN: &str = "FLAKE_PILOT_FORWARD";
pub const FIREWALL_INPUT_CHAIN: &str = "FLAKE_PILOT_INPUT";
// Chain of the egress policy of the VM network. The traffic of the
// VMs to the outside world passes it before it is forwarded, see
// NETWORK_EGRESS_PRESETS
pub const FIREWALL_EGRESS_CHAIN: &str = "FLAKE_PILOT_EGRESS";
// Max. size of a network interface name including the
// terminating zero byte as defined by IFNAMSIZ in linux/if.h
pub const IFNAMSIZ: usize = 16;
// Number of hex digits used to make shortened
// network interface names unique
pub const IFNAME_HASH_LEN: usize = 6;
// Record of the host network setup created by
// 'flake-ctl firecracker network init'. The record is written
// by flake-ctl and read by the pilot and therefore has to be
// looked up at the same place by both of them
pub const NETWORK_CONFIG: &str = "/etc/flakes/network.yaml";
// User specific location of that record, relative to the
// home directory of the user calling the program
pub const NETWORK_CONFIG_USER: &str =
    ".config/flakes/firecracker/network.yaml";
// Location of the storage volumes firecracker-pilot creates
// for VM instances with an overlay_size. The volumes are
// written by the pilot and read by flake-ctl and therefore
// have to be looked up at the same place by both of them.
// The volumes are stored relative to the home directory of
// the user the instance belongs to. Without a home directory
// they are stored below the private meta data directory of
// that user
pub const FIRECRACKER_OVERLAY_DIR: &str =
    ".config/flakes/firecracker/storage";
pub const FIRECRACKER_STORAGE_DIR: &str = "storage";
// File name extension of a VM storage volume
pub const FIRECRACKER_STORAGE_EXTENSION: &str = "ext4";
// Prefix of the vsock sockets firecracker-pilot creates in the
// private meta data directory of the user to talk to a VM
// instance. The socket of an instance is named PREFIXNAME.sock,
// the sockets of the commands called in it PREFIXNAME.sock_PORT.
// flake-ctl deletes them along with the flake they belong to
pub const FIRECRACKER_VSOCK_PREFIX: &str = "sci_cmd_";
pub const NFS_EXPORTS_FILE: &str =
    "/etc/exports";
pub const NFS_SERVER_SERVICE: &str =
    "nfs-server";
// Every access of a VM is mapped to the owner of the exported
// path, the anonuid and anongid options naming that owner are
// added per export
pub const NFS_EXPORT_OPTIONS: &str =
    "rw,sync,no_subtree_check,all_squash";
// Kernel switch to turn the host into a router. Required to
// forward the traffic of a VM from its TUN/TAP device to the
// outgoing interface of the host
pub const SYSCTL_IP_FORWARD: &str =
    "net.ipv4.ip_forward";
// Location the kernel reports the state of that switch at
pub const PROC_IP_FORWARD: &str =
    "/proc/sys/net/ipv4/ip_forward";
// Location of the IPv6 setup of a network interface. The setup
// of the TAP devices is IPv4 only, IPv6 is switched off on them
// such that a VM cannot reach the host through an IPv6 link
// local address, which the IPv4 firewall setup does not cover
pub const PROC_IPV6_CONF: &str =
    "/proc/sys/net/ipv6/conf";
// Record of the host wide network state: the network setup of
// the host, the addresses handed out to the VM instances of all
// users and whether flake-pilot has turned on IP forwarding. The
// file belongs to root and is only written by the helper, a user
// cannot change the setup or the addresses of other users
pub const NETWORK_STATE: &str =
    "/var/lib/firecracker/network-state.yaml";
// Lock file which serializes the changes of the network state
// between concurrent calls. It belongs to root like the state
pub const NETWORK_STATE_LOCK: &str =
    "/var/lib/firecracker/network-state.lock";
// Number of attempts to lock a lock file which got replaced
// while waiting for its lock
pub const LOCK_ATTEMPTS: u32 = 10;
// Services of the host a VM may reach through its gateway: NFS
// (version 4, which needs no other port) for the volumes exported
// to it, DNS and DHCP if the host setup was created for a resolver
// or a DHCP server on the host. Any other traffic of a VM to the
// host is dropped
pub const NETWORK_DHCP_SERVER_PORT: &str =
    "67";
pub const NETWORK_DHCP_CLIENT_PORT: &str =
    "68";
pub const NETWORK_DNS_PORT: &str =
    "53";
pub const NETWORK_NFS_PORT: &str =
    "2049";
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
pub const NETWORK_PREFERRED: &str =
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
pub const NETWORK_PREFIX_LEN: u8 =
    24;
// Prefix length of the host route which connects the address of
// a single VM instance with the TAP device of that instance
pub const NETWORK_HOST_PREFIX_LEN: u8 =
    32;
// Name server of the VMs unless the host setup was created for
// a resolver on the host, which the VMs use through their gateway
pub const NETWORK_DNS: &str =
    "8.8.8.8";
// Named sets of networks the egress policy of the VM network can
// deny, see 'flake-ctl firecracker network init --egress-deny'.
// The traffic of the VMs to them is dropped instead of being
// forwarded to the outgoing interface:
//
// link-local: the link local network, which provides the metadata
//   service of the cloud the host runs in at 169.254.169.254.
//   Without the policy a VM reaches it with the identity of the
//   host
// private: the private networks of RFC 1918, e.g the local
//   network of the host and the services in it
pub const NETWORK_EGRESS_PRESETS: &[(&str, &[&str])] = &[
    ("link-local", &["169.254.0.0/16"]),
    ("private", &["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"])
];
// Directory the overlay setups of the bubblewrap sandboxes are
// mounted in. It is shared between all users of the system, the
// setup of a user is therefore mounted on a directory of its own
// below it, see OVERLAY_USER_DIR_NAME. The setup is mounted in the
// mount namespace of the sandbox, by bubblewrap-pilot or by the
// helper, and is maintained along with the helper in the common
// area
pub const OVERLAY_BASE_DIR: &str = "/var/tmp";
// Name of the directory the tmpfs of the overlay setup of one user
// is mounted on. It is suffixed with the ID of that user, e.g
// /var/tmp/bwrap_1000. On the host it stays empty, the tmpfs is
// only mounted in the namespace of the sandbox
pub const OVERLAY_USER_DIR_NAME: &str = "bwrap";
// Names of the directories of the overlay setup in the tmpfs, e.g
// /var/tmp/bwrap_1000/merged. The lower directory is the mount
// point of the read only overlay of the rootfs, which is stacked
// on the empty directory. It is only used with layers on top of
// the rootfs
pub const OVERLAY_MERGED_NAME: &str = "merged";
pub const OVERLAY_UPPER_NAME: &str = "upper";
pub const OVERLAY_WORK_NAME: &str = "work";
pub const OVERLAY_LOWER_NAME: &str = "lower";
pub const OVERLAY_EMPTY_NAME: &str = "empty";
// Environment variable the helper sets for a bubblewrap-pilot it
// starts in the mount namespace of a sandbox, after it mounted the
// root filesystem of the sandbox and dropped its privileges. The
// pilot then only calls bwrap
pub const SANDBOX_MOUNTED_VAR: &str = "FLAKE_PILOT_SANDBOX_MOUNTED";
// Option which stacks another layer on top of the rootfs of a
// sandbox. A flake can configure it to add layers to the root of
// the sandbox. The layers are part of the overlay created by
// bubblewrap-pilot or the helper and are not passed to bwrap
pub const BWRAP_OVERLAY_SRC_OPTION: &str = "--overlay-src";
// Max. number of rounds a %VAR reference in a bwrap option of a
// flake is expanded in, a value can reference another variable
pub const VAR_EXPANSION_LIMIT: i32 = 10;
// Memory information of the kernel, the base of a tmpfs size given
// as percentage
pub const PROC_MEMINFO: &str = "/proc/meminfo";
