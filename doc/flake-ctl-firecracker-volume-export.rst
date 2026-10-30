FLAKE-CTL-FIRECRACKER-VOLUME-EXPORT(8)
======================================

NAME
----

**flake-ctl firecracker volume export** - Export a local path through NFS to the VM of an application

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl firecracker volume export --path <PATH> --app <APP>

   OPTIONS:
       --path <PATH>
       --app <APP>
       --instance <INSTANCE>
       --help

DESCRIPTION
-----------

Export the given absolute host path through NFS to the VM of the given
application. The command writes a flake-pilot managed entry to
``/etc/exports`` which allows exactly that VM as a client. The VM is
identified by its address in the private firecracker network, read
from the address allocations in the network state file
``/var/lib/firecracker/network-state.yaml`` which
**flake-ctl-firecracker-network-add**\(8) writes. The ``ip=`` boot
option in the flake configuration is not used for this, the flake
configuration of a user belongs to that user while the state file
belongs to root. The application, or the given instance of it,
therefore has to be connected to the network first. Exporting the same path for further applications or instances
adds their VMs to the entry.

Only a path owned by a user can be exported: called by a user, the
path has to be owned by that user. Called by root, the path has to be
owned by a user other than root, a path owned by root is refused. The
path must not lead through a symbolic link, the resolved path has to
be given instead.

The entry is written with the ``all_squash`` option and the user and
group ID of the owner of the path as ``anonuid`` and ``anongid``. Every
access of the VM, no matter which user performs it inside of the VM,
including ``root``, is therefore performed as the owner of the path.
The VM gets the permissions of that user on the host and nothing more.
Clients have to connect from a privileged port, which the NFS client
of the guest does when ``sci`` mounts the volume as ``root``.

An entry written by former versions of flake-pilot for the whole
private network is replaced when the path is exported again.

If the ``nfs-server`` systemd service is not running yet, it is
started. If it is already running, the export table is reloaded so the
new export becomes effective without waiting for a service restart.

The command expects an existing directory path. As it modifies a
system-wide NFS configuration, the export is created by the helper,
see **flake-pilot-helper**\(8), called through **sudo** for a caller
who is not root. The helper checks the directory, its owner and the
address of the VM again.

OPTIONS
-------

--path <PATH>

  Absolute directory path on the host to export through NFS

--app <APP>

  An absolute path to the application on the host. The path is
  exported to the VM of this application

--instance <INSTANCE>

  The @NAME instance selector the application is called with. The
  path is exported to the VM of that instance, which has its own
  address if it was connected with the same selector

FILES
-----

* /etc/exports
* /var/lib/firecracker/network-state.yaml
* /etc/flakes/network.yaml
* $HOME/.config/flakes/network.yaml
* /usr/share/flakes
* $HOME/.config/flakes

EXAMPLE
-------

.. code:: bash

   $ flake-ctl firecracker volume export \
         --path /some/local/path --app $HOME/bin/claude

SEE ALSO
--------

flake-ctl-firecracker-volume-release(8), flake-ctl-firecracker-network-add(8), flake-ctl-firecracker-volume-add(8), firecracker-pilot(8)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
