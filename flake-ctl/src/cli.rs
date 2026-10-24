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
use clap::{AppSettings, ArgEnum, Args, Parser, Subcommand, ArgGroup};

/// flake-ctl - Manage Flake Applications
#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
#[clap(global_setting(AppSettings::PropagateVersion))]
#[clap(global_setting(AppSettings::UseLongFormatForHelpSubcommand))]
pub struct Cli {
    #[clap(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Load and register OCI applications
    Podman {
        #[clap(subcommand)]
        command: Podman,
    },
    /// Load and register FireCracker VM applications
    Firecracker {
        #[clap(subcommand)]
        command: Firecracker,
    },
    /// Register applications running in a bubblewrap sandbox
    Bubblewrap {
        #[clap(subcommand)]
        command: Bubblewrap,
    },
    /// Create the setup to run flake applications
    Init {
        /// Create the configuration files from scratch, even if
        /// they already exist. Existing files are overwritten
        #[clap(long)]
        force: bool,
    },
    /// List registered flake applications
    List {
        /// Output format. The table format is meant for humans,
        /// the json and csv formats are meant to be parsed by
        /// scripts and other programs
        #[clap(long, arg_enum, value_name = "FORMAT", default_value = "table")]
        format: ListFormat,
    }
}

#[derive(ArgEnum, Clone, Copy, Debug, PartialEq)]
pub enum ListFormat {
    /// Human readable table with a headline
    Table,
    /// Machine readable JSON list of flake records
    Json,
    /// Machine readable comma separated values with a header line
    Csv,
}

#[derive(Subcommand)]
pub enum Firecracker {
    /// Pull image
    #[clap(
        group(
            ArgGroup::new("pull")
                .required(false).args(&["force"])
        ),
        group(
            ArgGroup::new("pullkis")
                .required(false).args(&["kis-image"])
                .conflicts_with("rootfs")
                .conflicts_with("kernel")
                .conflicts_with("initrd")
        ),
        group(
            ArgGroup::new("pullrootfs")
                .required(false).args(&["rootfs", "kernel"])
                .multiple(true)
                .conflicts_with("kis-image")
        ),
        group(
            ArgGroup::new("action")
                .required(true).args(&["kis-image", "rootfs", "kernel"])
                .multiple(true)
        ),
    )]
    Pull {
        /// Image name used as local identifier
        #[clap(long)]
        name: String,

        /// Firecracker image built by KIWI as kis image type
        /// to pull into local image store. Either an https URL
        /// or a local file given as path or file:// URI.
        /// A checksum file named like the image plus a '.sha256'
        /// suffix must exist next to it. It verifies the image
        /// and is used to check an already pulled image for an
        /// update
        #[clap(long)]
        kis_image: Option<String>,

        /// Single rootfs image to pull into local image store
        #[clap(long, requires = "kernel")]
        rootfs: Option<String>,

        /// Single kernel image to pull into local image store
        #[clap(long, requires = "rootfs")]
        kernel: Option<String>,

        /// Single initrd image to pull into local image store
        #[clap(long)]
        initrd: Option<String>,

        /// Force pulling the image even if it already exists
        /// This will wipe existing data for the provided
        /// identifier
        #[clap(long)]
        force: bool,
    },
    /// Build KIS image through a kiwi boxbuild in a container
    Build {
        /// Path to the KIWI image description to build. The
        /// description has to build the kis image type
        #[clap(long)]
        kiwi_image: String,

        /// Path name of the KIS archive to create. A checksum
        /// file named like the archive plus a '.sha256' suffix
        /// is created next to it, such that the result can be
        /// pulled via 'firecracker pull --kis-image'. The build
        /// log is kept next to it with a '.log' suffix
        #[clap(long)]
        outfile: String,

        #[clap(flatten)]
        boxbuild: BoxbuildOptions,
    },
    /// Register VM application
    Register {
        /// A virtual machine name. The name must match with a
        /// name in the local firecracker registry
        #[clap(long)]
        vm: String,

        /// An absolute path to the application on the host.
        /// If not specified via the target option, the
        /// application will be called with that path inside
        /// of the VM.
        #[clap(long)]
        app: String,

        /// An absolute path to the application in the VM.
        /// Use this option if the application path on the host
        /// should be different to the application path inside
        /// of the VM.
        #[clap(long)]
        target: Option<String>,

        /// Resume the VM from previous execution.
        /// If the VM is still running, the app will be
        /// executed inside of this VM instance.
        #[clap(long)]
        resume: bool,

        /// Force using a vsock to communicate between guest and
        /// host if resume is set to false. In resume mode the
        /// vsock setup is always required.
        #[clap(long)]
        force_vsock: bool,

        /// Size of overlay write space in bytes.
        /// Optional suffixes: KiB/MiB/GiB/TiB (1024) or KB/MB/GB/TB (1000)
        #[clap(long)]
        overlay_size: Option<String>,

        /// Pilot option, and optional value, in the format
        /// %name or %name:value. Pilot options are not passed
        /// to the application call but control the behavior of
        /// firecracker-pilot. An option registered here is always
        /// effective and does not have to be given at call time.
        /// This option can be specified multiple times. For the
        /// list of available pilot options please consult the
        /// firecracker-pilot manual page.
        #[clap(long, multiple = true)]
        pilot_option: Option<Vec<String>>,

        /// Force writing the registration even if a registration
        /// of the same name already exists
        #[clap(long)]
        force: bool,
    },
    /// Show VM instances
    Show {
        /// Output format. The table format is meant for humans,
        /// the json and csv formats are meant to be parsed by
        /// scripts and other programs
        #[clap(long, arg_enum, value_name = "FORMAT", default_value = "table")]
        format: ListFormat,
    },
    /// Manage the host network setup for VM applications
    Network {
        #[clap(subcommand)]
        command: Network,
    },
    /// Manage volumes for VM applications
    Volume {
        #[clap(subcommand)]
        command: Volume,
    },
    /// Remove application registration or entire VM
    #[clap(group(
        ArgGroup::new("remove").required(true).args(&["vm", "app"]),
    ))]
    Remove {
        /// Remove all applications registered with the given
        /// VM and also remove the VM from the
        /// local firecracker registry
        #[clap(long)]
        vm: Option<String>,

        /// Application absolute path to be removed from host
        #[clap(long)]
        app: Option<String>,

        /// Force removing the registration, do not raise an
        /// error if no registration exists. Do not apply
        /// the check for a flake registered app and remove
        /// when present. Use with care !
        #[clap(long)]
        force: bool,
    },
}

/// Options of the kiwi system boxbuild command, passed along
/// to the boxbuild running in the kiwi container
#[derive(Args)]
#[clap(
    group(
        ArgGroup::new("arch")
            .required(false).args(&["x86-64", "aarch64"])
    ),
    group(
        ArgGroup::new("sharing")
            .required(false)
            .args(&["qemu-9p-sharing", "virtiofs-sharing", "sshfs-sharing"])
    ),
)]
pub struct BoxbuildOptions {
    /// Name of the box to use for the build process
    #[clap(long = "box", value_name = "NAME", default_value = "tumbleweed")]
    pub boxname: String,

    /// Number of GBs to reserve as main memory for the
    /// virtual machine. By default 8GB will be used
    #[clap(long, value_name = "VMGB")]
    pub box_memory: Option<String>,

    /// Name of console in the kernel settings for the
    /// virtual machine. By default set to hvc0
    #[clap(long, value_name = "TTYNAME")]
    pub box_console: Option<String>,

    /// Number of CPUs to use in the SMP setup.
    /// By default 4 CPUs will be used
    #[clap(long, value_name = "NUMBER")]
    pub box_smp_cpus: Option<u32>,

    /// In debug mode the started virtual machine will
    /// be kept open
    #[clap(long)]
    pub box_debug: bool,

    /// Build in container instead of a VM. Options related
    /// to building in a VM will have no effect
    #[clap(long)]
    pub container: bool,

    /// Specify a KIWI version to use for the build. The
    /// referenced KIWI will be fetched from pip and replaces
    /// the box installed KIWI version
    #[clap(long, value_name = "VERSION")]
    pub kiwi_version: Option<String>,

    /// Optional host path to share with the box. The same
    /// path as it is present on the host will also be
    /// available inside of the box during build time
    #[clap(long, value_name = "PATH")]
    pub shared_path: Option<String>,

    /// Skip check for available box update. The option has
    /// no effect if the selected box does not yet exist
    #[clap(long)]
    pub no_update_check: bool,

    /// Run box with snapshot mode switched off. This causes
    /// the box disk file to be modified by the build process
    /// and allows to keep a persistent package cache as part
    /// of the box
    #[clap(long)]
    pub no_snapshot: bool,

    /// Run box without hardware acceleration. By default
    /// KVM acceleration is activated
    #[clap(long)]
    pub no_accel: bool,

    /// Select 9p backend to use for sharing data between
    /// the host and the box
    #[clap(long = "9p-sharing")]
    pub qemu_9p_sharing: bool,

    /// Select virtiofsd backend to use for sharing data
    /// between the host and the box
    #[clap(long)]
    pub virtiofs_sharing: bool,

    /// Select sshfs backend to use for sharing data between
    /// the host and the box
    #[clap(long)]
    pub sshfs_sharing: bool,

    /// Name of ssh key to authorize for connection.
    /// By default id_rsa is used
    #[clap(long, value_name = "NAME")]
    pub ssh_key: Option<String>,

    /// Port number to use to forward the guest SSH port
    /// to the host. By default 10022 is used
    #[clap(long, value_name = "PORT")]
    pub ssh_port: Option<u16>,

    /// Select box for the x86_64 architecture. If no
    /// architecture is selected the host architecture is
    /// used for selecting the box. The selected box
    /// architecture also specifies the target architecture
    /// for the image build with that box
    #[clap(long = "x86_64")]
    pub x86_64: bool,

    /// Select box for the aarch64 architecture, see --x86_64
    #[clap(long)]
    pub aarch64: bool,

    /// Machine name used by QEMU. By default no specific
    /// value is used here and qemu selects its default
    /// machine type. For cross arch builds it is required
    /// to specify a machine name
    #[clap(long, value_name = "QEMU_MACHINE")]
    pub machine: Option<String>,

    /// CPU type used by QEMU. By default the host CPU type
    /// is used. On cross arch builds it is required to
    /// specify the CPU emulation the box should use
    #[clap(long, value_name = "QEMU_CPU")]
    pub cpu: Option<String>,
}

#[derive(Subcommand)]
pub enum Network {
    /// Prepare the host for NAT based VM networking
    Init {
        /// Name of the host interface the traffic of the VMs
        /// is routed to the outside world through, e.g eth0.
        /// If not specified, the interface of the IPv4 default
        /// route is used
        #[clap(long)]
        outgoing_interface: Option<String>,

        /// The host runs a DNS resolver which answers queries
        /// on the gateway address of the VM network. The VMs
        /// connected afterwards use it as their name server and
        /// may reach the DNS port of the host. Without this
        /// option the VMs use a public name server and the DNS
        /// port of the host is closed to them
        #[clap(long)]
        host_resolver: bool,

        /// The host runs a DHCP server for the VMs on the
        /// gateway address of the VM network. The VMs connected
        /// afterwards may reach the DHCP port of the host. The
        /// VMs are still configured with a static address, a
        /// guest which requests one by DHCP has to be set up for
        /// it. Without this option the DHCP port of the host is
        /// closed to them
        #[clap(long)]
        host_dhcp: bool,

        /// Network the traffic of the VMs to the outside world
        /// must not go to. Either link-local, which covers the
        /// metadata service of a cloud at 169.254.169.254, private,
        /// which covers the private networks of RFC 1918, or a
        /// network as ADDRESS[/PREFIX_LEN]. Can be specified
        /// multiple times. Without this option the VMs may reach
        /// every network the host reaches through the outgoing
        /// interface
        #[clap(long, multiple_occurrences = true)]
        egress_deny: Vec<String>,
    },
    /// Connect a VM application to the host network
    Add {
        /// An absolute path to the application on the host.
        /// The application must be registered as a VM
        /// application and its flake configuration gets the
        /// network setup written to it
        #[clap(long)]
        app: String,

        /// The @NAME instance selector the application is
        /// called with. Every instance needs its own network
        /// setup, therefore the command has to be called for
        /// each of them
        #[clap(long)]
        instance: Option<String>,

        /// Name of the user who runs the VM. The TAP device of
        /// the instance belongs to this user, nobody else can
        /// connect a VM to it. Defaults to the calling user.
        /// Another user can only be named for a system wide
        /// flake, which is set up by root and run by any user
        #[clap(long)]
        user: Option<String>,
    },
    /// Disconnect a VM application from the host network
    Remove {
        /// An absolute path to the application on the host.
        /// The network setup of the application is deleted
        /// from the host and from its flake configuration
        #[clap(long)]
        app: String,

        /// The @NAME instance selector the network setup was
        /// created for. Only the setup of that instance is
        /// deleted
        #[clap(long)]
        instance: Option<String>,

        /// Name of the user the network setup was created for.
        /// Defaults to the calling user
        #[clap(long)]
        user: Option<String>,
    },
    /// Delete the network setup of the host: the firewall setup,
    /// the TAP devices of all applications and IP forwarding if
    /// it was enabled by init. The addresses of the applications
    /// are kept for the next setup
    Cleanup,
}

#[derive(Subcommand)]
pub enum Volume {
    /// Export a local path through NFS to the VM of an application
    Export {
        /// An absolute path on the host to export via NFS. The
        /// path must be owned by the calling user, or by a user
        /// other than root if called by root. Every access of the
        /// VM is performed as the owner of the path
        #[clap(long)]
        path: String,

        /// An absolute path to the application on the host. The
        /// path is exported to the VM of this application, which
        /// is identified by its address in the private network
        #[clap(long)]
        app: String,

        /// The @NAME instance selector the application is
        /// called with. The path is exported to the VM of that
        /// instance
        #[clap(long)]
        instance: Option<String>,
    },
    /// Remove an exported local path from the NFS configuration
    Release {
        /// An absolute path on the host to remove from the NFS exports
        #[clap(long)]
        path: String,

        /// An absolute path to the application on the host. Only
        /// the VM of this application is removed from the export.
        /// Without it, the export is deleted
        #[clap(long)]
        app: Option<String>,

        /// The @NAME instance selector the application is
        /// called with. Only the VM of that instance is removed
        /// from the export
        #[clap(long, requires = "app")]
        instance: Option<String>,
    },
    /// Provide a volume to a VM application
    Add {
        /// An absolute path to the application on the host.
        /// The application must be registered as a VM
        /// application and its flake configuration gets the
        /// volume setup written to it
        #[clap(long)]
        app: String,

        /// A volume to provide to the application in the format
        /// /some/local/path:/some/guest/path. The local path is
        /// mounted on the guest path inside of the VM. This
        /// option can be specified multiple times
        #[clap(long, multiple = true, required = true)]
        volume: Vec<String>,

        /// The @NAME instance selector the application is
        /// called with. The volumes are provided to that
        /// instance only and take the place of the volumes
        /// configured for the application itself
        #[clap(long)]
        instance: Option<String>,
    },
    /// Delete a volume from a VM application
    Remove {
        /// An absolute path to the application on the host.
        /// The volume setup is deleted from the flake
        /// configuration of the application
        #[clap(long)]
        app: String,

        /// A volume to delete from the application in the format
        /// /some/local/path:/some/guest/path. This option can be
        /// specified multiple times
        #[clap(long, multiple = true, required = true)]
        volume: Vec<String>,

        /// The @NAME instance selector the volumes were
        /// added for. Only the volumes of that instance
        /// are deleted
        #[clap(long)]
        instance: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum Bubblewrap {
    /// Register sandbox application
    Register {
        /// An absolute path to the root filesystem tree on the
        /// host. The tree is mounted as the read-only layer of
        /// an overlay which becomes the root filesystem of the
        /// sandbox the application runs in
        #[clap(long)]
        rootfs: String,

        /// An absolute path to the application on the host.
        /// If not specified via the target option, the
        /// application will be called with that path inside
        /// of the sandbox.
        #[clap(long)]
        app: String,

        /// An absolute path to the application in the sandbox.
        /// Use this option if the application path on the host
        /// should be different to the application path inside
        /// of the sandbox.
        #[clap(long)]
        target: Option<String>,

        /// Sandbox runtime option, and optional value, used to
        /// create the sandbox, e.g "--ro-bind /etc/hosts /etc/hosts".
        /// The options are passed to the bwrap call in addition to
        /// the default options of the sandbox. This option can be
        /// specified multiple times.
        #[clap(long, multiple = true)]
        opt: Option<Vec<String>>,

        /// Pilot option, and optional value, in the format
        /// %name or %name:value. Pilot options are not passed
        /// to the application call but control the behavior of
        /// bubblewrap-pilot. An option registered here is always
        /// effective and does not have to be given at call time.
        /// This option can be specified multiple times. For the
        /// list of available pilot options please consult the
        /// bubblewrap-pilot manual page.
        #[clap(long, multiple = true)]
        pilot_option: Option<Vec<String>>,

        /// Force writing the registration even if a registration
        /// of the same name already exists
        #[clap(long)]
        force: bool,
    },
    /// Show sandbox instances
    Show {
        /// Output format. The table format is meant for humans,
        /// the json and csv formats are meant to be parsed by
        /// scripts and other programs
        #[clap(long, arg_enum, value_name = "FORMAT", default_value = "table")]
        format: ListFormat,
    },
    /// Remove application registration
    Remove {
        /// Application absolute path to be removed from host
        #[clap(long)]
        app: String,

        /// Force removing the registration, do not raise an
        /// error if no registration exists. Do not apply
        /// the check for a flake registered app and remove
        /// when present. Use with care !
        #[clap(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub enum Podman {
    /// Pull container
    Pull {
        /// OCI image to pull from remote registry into local podman registry
        #[clap(long)]
        uri: String,

        /// Store the image in the additional read-only image
        /// store /usr/lib/podman/storage instead of the local
        /// podman registry. Only supported as root
        #[clap(long)]
        use_addon_store: bool,
    },
    /// Load container
    Load {
        /// OCI image to load into local podman registry
        #[clap(long)]
        oci: String,

        /// Store the image in the additional read-only image
        /// store /usr/lib/podman/storage instead of the local
        /// podman registry. Only supported as root
        #[clap(long)]
        use_addon_store: bool,
    },
    /// Export container or OCI tarball to directory
    #[clap(group(
        ArgGroup::new("export").required(true).args(&["container", "oci"]),
    ))]
    Export {
        /// A container name. The name must match with a
        /// name in the local podman registry
        #[clap(long)]
        container: Option<String>,

        /// Path to an OCI compliant tarball. The file system
        /// of the image in the tarball is unpacked without
        /// the use of the local podman registry
        #[clap(long)]
        oci: Option<String>,

        /// Path to the directory the file system of the
        /// container or OCI tarball is exported to. The
        /// directory is created if it does not exist yet
        #[clap(long)]
        directory: String,

        /// Export even if the given directory exists. The
        /// file system of the container or OCI tarball is
        /// unpacked on top of the contents of that directory
        #[clap(long)]
        force: bool,
    },
    /// Show container instances
    Show {
        /// Output format. The table format is meant for humans,
        /// the json and csv formats are meant to be parsed by
        /// scripts and other programs
        #[clap(long, arg_enum, value_name = "FORMAT", default_value = "table")]
        format: ListFormat,
    },
    /// Remove application registration or entire container
    #[clap(group(
        ArgGroup::new("remove").required(true).args(&["container", "app"]),
    ))]
    Remove {
        /// Remove all applications registered with the given
        /// container and also remove the container from the
        /// local podman registry
        #[clap(long)]
        container: Option<String>,

        /// Application absolute path to be removed from host
        #[clap(long)]
        app: Option<String>,

        /// Force removing the registration, do not raise an
        /// error if no registration exists. Do not apply
        /// the check for a flake registered app and remove
        /// when present. Use with care !
        #[clap(long)]
        force: bool,
    },
    /// Stop and delete the container instance of a resume flake
    Reset {
        /// An absolute path to the application on the host.
        /// The application must be registered as a container
        /// application with the resume option set
        #[clap(long)]
        app: String,

        /// The @NAME instance selector the application is
        /// called with. Every instance runs in its own
        /// container, therefore the command has to be called
        /// for each of them
        #[clap(long)]
        instance: Option<String>,
    },
    /// Register container application
    #[clap(
        group(
            ArgGroup::new("register")
                .required(false).args(&["resume", "attach"])
        ),
        group(
            ArgGroup::new("application")
                .required(true).args(&["app", "info"])
        )
    )]
    Register {
        /// A container name. The name must match with a
        /// name in the local podman registry
        #[clap(long)]
        container: String,

        /// An absolute path to the application on the host.
        /// If not specified via the target option, the
        /// application will be called with that path inside
        /// of the container.
        #[clap(long)]
        app: Option<String>,

        /// An absolute path to the application in the container.
        /// Use this option if the application path on the host
        /// should be different to the application path inside
        /// of the container. Set this option to just "/"
        /// if the default entrypoint of the container should
        /// be called.
        #[clap(long)]
        target: Option<String>,

        /// Name of the base container. The name must match with a
        /// name in the local podman registry
        #[clap(long)]
        base: Option<String>,

        /// Check if the container has dependencies to the host
        /// When using a base container this check is enabled
        /// by default
        #[clap(long)]
        check_host_dependencies: bool,

        /// Name of an additional container layer on top of
        /// the specified base container. This option can be
        /// specified multiple times. The resulting layer list
        /// is evaluated in the order of the arguments as they
        /// were provided on the command line.
        #[clap(long, multiple = true)]
        layer: Option<Vec<String>>,

        /// Resume the container from previous execution.
        /// If the container is still running, the app will be
        /// executed inside of this container instance.
        #[clap(long)]
        resume: bool,

        /// Attach to the container if still running, rather than
        /// executing the app again. Only makes sense for interactive
        /// sessions like a shell application.
        #[clap(long)]
        attach: bool,

        /// Container runtime option, and optional value, used to
        /// create the container. This option can be
        /// specified multiple times.
        #[clap(long, multiple = true)]
        opt: Option<Vec<String>>,

        /// Pilot option, and optional value, in the format
        /// %name or %name:value. Pilot options are not passed
        /// to the application call but control the behavior of
        /// podman-pilot. An option registered here is always
        /// effective and does not have to be given at call time.
        /// This option can be specified multiple times. For the
        /// list of available pilot options please consult the
        /// podman-pilot manual page.
        #[clap(long, multiple = true)]
        pilot_option: Option<Vec<String>>,

        /// Print registration information from container if provided
        #[clap(long)]
        info: bool,

        /// Force writing the registration even if a registration
        /// of the same name already exists
        #[clap(long)]
        force: bool,
    },
}

pub fn parse_args() -> Cli {
    Cli::parse()
}
