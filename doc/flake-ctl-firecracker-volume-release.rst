FLAKE-CTL-FIRECRACKER-VOLUME-RELEASE(8)
=======================================

NAME
----

**flake-ctl firecracker volume release** - Remove a local path from the firecracker NFS exports

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl firecracker volume release --path <PATH>

   OPTIONS:
       --path <PATH>
       --help

DESCRIPTION
-----------

Remove the flake-pilot managed NFS export entry for the given absolute
host path from ``/etc/exports`` and restart the ``nfs-server`` systemd
service so the updated export table becomes effective.

Only entries previously managed by flake-pilot for the given path are
removed. As the command changes a system-wide NFS configuration, the
required privileged operations are executed through **sudo** when
needed.

OPTIONS
-------

--path <PATH>

  Absolute host path to remove from the flake-pilot managed NFS exports

FILES
-----

* /etc/exports

EXAMPLE
-------

.. code:: bash

   $ flake-ctl firecracker volume release --path /some/local/path

SEE ALSO
--------

flake-ctl-firecracker-volume-export(8), flake-ctl-firecracker-network-init(8), firecracker-pilot(8)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
