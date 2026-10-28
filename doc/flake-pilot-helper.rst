FLAKE-PILOT-HELPER(8)
=====================

NAME
----

**flake-pilot-helper** - Perform the privileged actions of flake-pilot

SYNOPSIS
--------

.. code:: bash

   USAGE:
       sudo /usr/libexec/flake-pilot/flake-pilot-helper <SUBCOMMAND> [--debug]

   SUBCOMMANDS:
       network-init --network <NETWORK> --outgoing-interface <IF> [--host-resolver] [--host-dhcp] [--egress-deny <NET>]...
       network-cleanup
       network-connect --config-file <FILE> [--instance <NAME>] [--uid <UID>] [--prefer <ADDR>] [--exclude <ADDR>]...
       network-disconnect --config-file <FILE> [--instance <NAME>] [--uid <UID>]
       volume-export --path <PATH> --config-file <FILE> [--instance <NAME>]
       volume-release --path <PATH> [--config-file <FILE> [--instance <NAME>]]
       podman-pilot --app <PATH> [--term <TERM>] -- [ARGS]...
       bubblewrap-pilot --app <PATH> [--size <SIZE>] [--term <TERM>] -- [ARGS]...
       meta-dirs-init

DESCRIPTION
-----------

The helper performs every action of flake-pilot which needs root
privileges. It is not meant to be called by hand: **flake-ctl**\(8),
**podman-pilot**\(8) and **bubblewrap-pilot**\(8) call it through
**sudo** when they need root privileges. It is the only program
flake-pilot calls through sudo.

The helper belongs to root and has to be called through sudo or by
root directly. It refuses to run as a setuid program. It takes the
calling user from the SUDO_UID variable sudo provides and clears its
environment before it does anything else.

Every subcommand validates its arguments and checks the authority
of the calling user over the resource it operates on. The checks are
made against data which belongs to root, e.g the host wide network
state /var/lib/firecracker/network-state.yaml or the system wide flake
registry. Names are checked against a strict pattern before they
become part of a path, a device name or a program argument. No
argument is passed on to a program unchecked, except for the
application arguments of **bubblewrap-pilot**, which are only
evaluated after the helper has dropped its privileges to the ones of
the caller. The registration of a bubblewrap flake of the caller is
read with the identity of the caller.

The helper only prints errors. With **--debug** it prints its log
messages as well. The callers pass the option if PILOT_DEBUG is set
in their environment, the helper does not see that environment. The
option is given after the subcommand: a sudo rule can be limited to
subcommands by their name in the first argument. In debug mode the
**podman-pilot** and **bubblewrap-pilot** subcommands run their pilot
with PILOT_DEBUG set.

A sudo rule for the helper therefore does not make a user equivalent
to root. What the rule grants is described in the reference policy,
/usr/share/doc/packages/flake-pilot/flake-pilot.sudoers, and in the
chapter "Privileges and Trust" of the flake-pilot user guide.

SUBCOMMANDS
-----------

network-init
  Prepare the host for the network of the firecracker VMs. The
  network has to be one of the private networks flake-pilot selects
  from and must not collide with a network of the host, the
  interface has to exist. Both are recorded in the network state
  along with the services of the host the VMs use and the networks
  of the egress policy, which only adds drop rules. Called by
  **flake-ctl-firecracker-network-init**\(8)

network-cleanup
  Delete the network setup of the host, see
  **flake-ctl-firecracker-network-cleanup**\(8)

network-connect
  Connect a flake instance to the network of the VMs. The flake
  configuration has to belong to the caller or to the system wide
  registry. The address is allocated within the recorded network and
  written to stdout. The TAP device is created for the caller, only
  root can name another user. Called by
  **flake-ctl-firecracker-network-add**\(8)

network-disconnect
  Disconnect a flake instance from the network of the VMs, see
  **flake-ctl-firecracker-network-remove**\(8)

volume-export
  Export a directory through NFS to the VM of a flake instance. The
  directory has to belong to the caller, root can export a directory
  of any user other than root. The VM is identified by the address
  allocated to it in the network state. Called by
  **flake-ctl-firecracker-volume-export**\(8)

volume-release
  Remove a directory from the NFS exports. Only the user the export
  maps every access to, and root, can release it. Called by
  **flake-ctl-firecracker-volume-release**\(8)

podman-pilot
  Run a podman flake registered system wide for the caller. The
  application has to be a link to podman-pilot which belongs to
  root, and its registration has to belong to root. The helper
  replaces itself by podman-pilot running as root with a fresh
  environment. Called by **podman-pilot**\(8)

bubblewrap-pilot
  Run a bubblewrap flake with mount points below its root filesystem
  for the caller. For a flake registered system wide the application
  has to be a link to bubblewrap-pilot which belongs to root, and its
  registration has to belong to root. For a flake of the caller the
  application has to be a link to bubblewrap-pilot. The flakes
  configuration of the caller, ~/.config/flakes.yml, and the
  registration below its flakes_dir are read with the identity of the
  caller. A system wide registration takes precedence, like for the
  pilot. The rootfs and the layers, the **--overlay-src** sources of
  the bwrap options, are taken from the registration and mounted in a
  new mount namespace. All mounts of the namespace are made private,
  nothing propagates to or from the host. The directory of the caller
  below /var/tmp is opened without following a symbolic link and has
  to belong to the caller. Its parents have to belong to root and
  must not be writable for others unless they are sticky, like
  /var/tmp. The rootfs and the layers are opened with the identity of
  the caller. The overlay of them and the tmpfs which keeps its
  writable layer are mounted nosuid and nodev, the tmpfs belongs to
  the caller. Its size is **--size**, which must not exceed the
  sandbox_tmpfs_size limit of /etc/flakes.yml, or the limit itself.
  The helper then drops its privileges to the caller for good, sets
  no_new_privs and replaces itself by bubblewrap-pilot, called as the
  application with the given arguments and a fresh environment. The
  pilot takes the bwrap options and the application in the sandbox
  from the registration as well and replaces itself by bwrap. The
  arguments of the caller are only passed to the application.
  **--app** is given once, right after the subcommand, which allows a
  sudo rule to grant single applications. Called by
  **bubblewrap-pilot**\(8)

meta-dirs-init
  Create the meta data directories of the pilots which are
  configured in /etc/flakes.yml. Called by **flake-ctl-init**\(8)

FILES
-----

* /usr/libexec/flake-pilot/flake-pilot-helper
* /var/lib/firecracker/network-state.yaml
* /var/lib/firecracker/network-state.lock
* /etc/exports
* /etc/flakes.yml

AUTHOR
------

Marcus Schäfer
