FLAKE-CTL-FIRECRACKER-VOLUME-RELEASE(8)
=======================================

NAME
----

**flake-ctl firecracker volume release** - Remove a local path from the firecracker NFS exports

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl firecracker volume release [OPTIONS] --path <PATH>

   OPTIONS:
       --path <PATH>
       --app <APP>
       --instance <INSTANCE>
       --help

DESCRIPTION
-----------

Remove the flake-pilot managed NFS export entry for the given absolute
host path from ``/etc/exports`` and restart the ``nfs-server`` systemd
service so the updated export table becomes effective.

Called with an application, only the VM of that application, or of the
given instance of it, is removed from the entry. The entry itself is
removed once no VM is left.

Only entries previously managed by flake-pilot for the given path are
removed. As the command changes a system-wide NFS configuration, the
required privileged operations are executed through **sudo** when
needed.

OPTIONS
-------

--path <PATH>

  Absolute host path to remove from the flake-pilot managed NFS exports

--app <APP>

  An absolute path to the application on the host. Only the VM of
  this application is removed from the export. The VM is identified
  by the address allocated to it in the network state file, the
  application therefore has to be still connected to the network.
  The export of an application which was disconnected already is
  released for all VMs by calling the command without --app

--instance <INSTANCE>

  The @NAME instance selector the application is called with. Only
  the VM of that instance is removed from the export. Requires --app

FILES
-----

* /etc/exports
* /var/lib/firecracker/network-state.yaml

EXAMPLE
-------

.. code:: bash

   $ flake-ctl firecracker volume release --path /some/local/path

   $ flake-ctl firecracker volume release \
         --path /some/local/path --app $HOME/bin/claude

SEE ALSO
--------

flake-ctl-firecracker-volume-export(8), flake-ctl-firecracker-network-init(8), firecracker-pilot(8)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
