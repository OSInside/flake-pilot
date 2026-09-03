FLAKE-CTL-FIRECRACKER-VOLUME-EXPORT(8)
======================================

NAME
----

**flake-ctl firecracker volume export** - Export a local path through NFS for firecracker guests

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl firecracker volume export --path <PATH>

   OPTIONS:
       --path <PATH>
       --help

DESCRIPTION
-----------

Export the given absolute host path through NFS for firecracker guest
access. The command writes a flake-pilot managed entry to
``/etc/exports`` for the private firecracker network ``172.16.0.0/24``.

If the ``nfs-server`` systemd service is not running yet, it is
started. If it is already running, the export table is reloaded so the
new export becomes effective without waiting for a service restart.

The command expects an existing directory path. As it modifies a
system-wide NFS configuration, the required privileged operations are
executed through **sudo** when needed.

OPTIONS
-------

--path <PATH>

  Absolute directory path on the host to export through NFS

FILES
-----

* /etc/exports

EXAMPLE
-------

.. code:: bash

   $ flake-ctl firecracker volume export --path /some/local/path

SEE ALSO
--------

flake-ctl-firecracker-volume-release(8), flake-ctl-firecracker-network-init(8), firecracker-pilot(8)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
