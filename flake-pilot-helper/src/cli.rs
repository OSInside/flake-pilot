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
    /// Run a bubblewrap flake with mount points below its root
    /// filesystem for the caller, registered system wide or by the
    /// caller. The root filesystem, the bwrap options and the
    /// application are taken from the registration
    BubblewrapPilot {
        /// Path of the application on the host, the link to
        /// bubblewrap-pilot the flake is called by. Given only once,
        /// a sudo rule can name the application right after the
        /// command
        #[clap(long)]
        app: String,

        /// Size of the tmpfs which keeps the writable layer, e.g 2G or
        /// 25%. It must not exceed the limit configured as
        /// sandbox_tmpfs_size in /etc/flakes.yml, which is the
        /// default size
        #[clap(long)]
        size: Option<String>,

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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(
            std::iter::once("flake-pilot-helper").chain(args.iter().copied())
        )
    }

    #[test]
    fn test_bubblewrap_pilot_app() {
        // A sudo rule like "bubblewrap-pilot --app /usr/bin/aws *"
        // matches the arguments as one string. Only a call which
        // gives the application in exactly these arguments is
        // accepted, anything matching the rule otherwise is refused
        let cli = parse(&[
            "bubblewrap-pilot", "--app", "/usr/bin/aws", "--term", "vt220",
            "--", "--app", "/usr/bin/bash", "s3", "ls"
        ]).unwrap();
        match cli.command {
            Commands::BubblewrapPilot { app, args, .. } => {
                assert_eq!("/usr/bin/aws", app);
                assert_eq!(vec!["--app", "/usr/bin/bash", "s3", "ls"], args);
            },
            _ => panic!("bubblewrap-pilot not parsed")
        }
        // the application given another time
        assert!(parse(&[
            "bubblewrap-pilot", "--app", "/usr/bin/aws", "--app", "/usr/bin/bash"
        ]).is_err());
        assert!(parse(&[
            "bubblewrap-pilot", "--app", "/usr/bin/aws", "--app=/usr/bin/bash"
        ]).is_err());
        // the words of the rule joined differently
        assert!(parse(&["bubblewrap-pilot --app", "/usr/bin/aws"]).is_err());
        assert!(parse(&["bubblewrap-pilot", "--app /usr/bin/aws", "--"]).is_err());
        // arguments of the application before the separator
        assert!(parse(&[
            "bubblewrap-pilot", "--app", "/usr/bin/aws", "/usr/bin/bash"
        ]).is_err());
    }
}
