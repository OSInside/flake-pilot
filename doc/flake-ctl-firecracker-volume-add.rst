FLAKE-CTL-FIRECRACKER-VOLUME-ADD(8)
====================================

NAME
----

**flake-ctl firecracker volume add** - Provide a volume to a firecracker VM application

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl firecracker volume add [OPTIONS] --app <APP> --volume <VOLUME>...

   OPTIONS:
       --app <APP>
       --volume <VOLUME>...
       --instance <INSTANCE>
       --help

DESCRIPTION
-----------

Provide the given host path(s) to the given firecracker flake
application through NFS. The flake configuration of the
application is looked up from the given application path and
receives an ``nfs=`` boot option of the form:

.. code:: yaml

   boot_args:
     - nfs=172.16.0.1:/some/local/path:/some/guest/path

The server the volume is mounted from is the gateway address of
the private network between the host and the VMs. That address
is read from the ``rd.route=`` boot option which is written to
the flake configuration by
**flake-ctl-firecracker-network-add**(8). The network setup
therefore has to be created for the application, or for the
given instance, before a volume can be added.

Multiple volumes are folded into a single ``nfs=`` option as a
comma separated list, which is the format **sci**(8) reads the
volumes of a VM instance from.

The host path of a volume also has to be exported through NFS,
see **flake-ctl-firecracker-volume-export**(8). The application
does not have to be running when the command is called, the
volume becomes available the next time the instance is started.

A volume which is already configured with the given guest path
keeps the place it has in the ``nfs=`` list, only the server or
host path part of the entry is updated if it changed. A volume
which is not yet configured is appended to the list.

As the command only modifies the flake configuration of the
application no privileged operations are required.

INSTANCES
---------

Called with the ``--instance`` option the volumes are written to
the section of that instance and take the place of the volumes
configured for the application itself, they are only mounted
when the application is called with that selector. The gateway
address is also looked up from the network setup of that
instance, falling back to the one of the application if the
instance has none of its own.

OPTIONS
-------

--app <APP>

  Absolute path of the application on the host. The application
  has to be registered as a VM application, see
  **flake-ctl-firecracker-register**(8), and connected to the
  host network, see **flake-ctl-firecracker-network-add**(8)

--volume <VOLUME>

  A volume to provide to the application, in the format
  ``/some/local/path:/some/guest/path``. This option can be
  specified multiple times

--instance <INSTANCE>

  The ``@NAME`` instance selector the application is called
  with. For convenience the plain name without the ``@`` prefix
  is accepted as well

FILES
-----

* /usr/share/flakes
* $HOME/.config/flakes

EXAMPLE
-------

.. code:: bash

   $ flake-ctl firecracker volume export --path /some/local/path

   $ flake-ctl firecracker volume add \
         --app $HOME/bin/claude --volume /some/local/path:/some/guest/path

SEE ALSO
--------

flake-ctl-firecracker-volume-remove(8), flake-ctl-firecracker-volume-export(8), flake-ctl-firecracker-network-add(8), sci(8)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
