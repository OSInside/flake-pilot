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
use clap::{AppSettings, Parser, Subcommand};
use std::net::Ipv4Addr;

/// Perform the actions of flake-pilot which need root privileges
///
/// Called through sudo by flake-ctl and the pilots. Every command
/// validates its arguments and the authority of its caller.
#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
#[clap(global_setting(AppSettings::PropagateVersion))]
pub struct Cli {
    #[clap(subcommand)]
    pub command: Commands,

    /// Show the log messages of the helper, not only its errors.
    /// The callers pass it if PILOT_DEBUG is set. It is given
    /// after the command, a sudo rule can be limited to commands
    /// by their name in the first argument
    #[clap(long, global = true)]
    pub debug: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Prepare the host for the network of the firecracker VMs
    NetworkInit {
        /// Private network of the VMs, ADDRESS/PREFIX_LEN. It has to
        /// be one of the private networks flake-pilot selects from
        /// and must not collide with a network of the host
        #[clap(long)]
        network: String,

        /// Interface the traffic of the VMs leaves the host through
        #[clap(long)]
        outgoing_interface: String,

        /// The host runs the DNS resolver of the VMs
        #[clap(long)]
        host_resolver: bool,

        /// The host runs a DHCP server for the VMs
        #[clap(long)]
        host_dhcp: bool,

        /// Network the traffic of the VMs to the outside world must
        /// not go to: link-local, private or ADDRESS[/PREFIX_LEN].
        /// Given more than once, all of them are denied
        #[clap(long, multiple_occurrences = true)]
        egress_deny: Vec<String>,
    },
    /// Delete the network setup of the host
    NetworkCleanup,
    /// Connect a flake instance to the network of the VMs. The
    /// allocated address is written to stdout
    NetworkConnect {
        /// Flake configuration of the application. It has to belong
        /// to the caller or to the system wide registry
        #[clap(long)]
        config_file: String,

        /// The @NAME instance selector of the application
        #[clap(long)]
        instance: Option<String>,

        /// ID of the user who runs the VM. Only root can name
        /// another user than itself
        #[clap(long)]
        uid: Option<u32>,

        /// Address to keep if it is free, e.g the one in the flake
        /// configuration
        #[clap(long)]
        prefer: Option<Ipv4Addr>,

        /// Address not to hand out, e.g one configured for a flake
        /// which was connected before the allocations were recorded
        #[clap(long, multiple_occurrences = true)]
        exclude: Vec<Ipv4Addr>,
    },
    /// Disconnect a flake instance from the network of the VMs
    NetworkDisconnect {
        /// Flake configuration of the application
        #[clap(long)]
        config_file: String,

        /// The @NAME instance selector of the application
        #[clap(long)]
        instance: Option<String>,

        /// ID of the user who runs the VM. Only root can name
        /// another user than itself
        #[clap(long)]
        uid: Option<u32>,
    },
    /// Export a directory through NFS to the VM of a flake instance
    VolumeExport {
        /// Directory to export. It has to belong to the caller,
        /// root can export a directory of any other user
        #[clap(long)]
        path: String,

        /// Flake configuration of the application
        #[clap(long)]
        config_file: String,

        /// The @NAME instance selector of the application
        #[clap(long)]
        instance: Option<String>,
    },
    /// Remove a directory from the NFS exports
    VolumeRelease {
        /// Exported directory
        #[clap(long)]
        path: String,

        /// Flake configuration of the application whose VM is
        /// removed from the export. Without it the export is deleted
        #[clap(long)]
        config_file: Option<String>,

        /// The @NAME instance selector of the application
        #[clap(long, requires = "config-file")]
        instance: Option<String>,
    },
    /// Run bwrap for the caller in a mount namespace with the root
    /// filesystem of the sandbox. Used for a rootfs with mount
    /// points below it, which bubblewrap-pilot cannot overlay in
    /// a user namespace, and for the sandboxes of root whose mounts
    /// persist on the host
    SandboxRun {
        /// Directory tree which is the lowest layer of the overlay
        #[clap(long)]
        rootfs: String,

        /// Directory tree stacked on top of the rootfs, read only.
        /// Given more than once, the layers are stacked in the
        /// given order
        #[clap(long = "layer", multiple_occurrences = true)]
        layers: Vec<String>,

        /// Size of the tmpfs which keeps the writable layer, e.g 2G or
        /// 25%. It must not exceed the limit configured as
        /// sandbox_tmpfs_size in /etc/flakes.yml, which is the
        /// default size
        #[clap(long)]
        size: Option<String>,

        /// Terminal type of the caller
        #[clap(long)]
        term: Option<String>,

        /// Directory whose mounts persist on the host: a mount the
        /// application creates at or below it in the sandbox is
        /// attached to the host at the same location when the
        /// sandbox ends. Only root may give it, more than once
        #[clap(long = "persist-mount", multiple_occurrences = true)]
        persist_dirs: Vec<String>,

        /// Arguments of bwrap
        #[clap(last = true)]
        args: Vec<String>,
    },
    /// Create the meta data base directories of the pilots, as
    /// configured system wide
    MetaDirsInit,
    /// Run a system wide podman flake for the caller
    PodmanPilot {
        /// Path of the application on the host, the link to
        /// podman-pilot the flake is called by
        #[clap(long)]
        app: String,

        /// Terminal type of the caller
        #[clap(long)]
        term: Option<String>,

        /// Arguments of the application
        #[clap(last = true)]
        args: Vec<String>,
    },
}

pub fn parse_args() -> Cli {
    Cli::parse()
}
