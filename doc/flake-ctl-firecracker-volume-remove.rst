FLAKE-CTL-FIRECRACKER-VOLUME-REMOVE(8)
=======================================

NAME
----

**flake-ctl firecracker volume remove** - Delete a volume from a firecracker VM application

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl firecracker volume remove [OPTIONS] --app <APP> --volume <VOLUME>...

   OPTIONS:
       --app <APP>
       --volume <VOLUME>...
       --instance <INSTANCE>
       --help

DESCRIPTION
-----------

Delete the given volume(s) from the flake configuration of the
given firecracker flake application. This reverts what
**flake-ctl-firecracker-volume-add**(8) has performed. A volume
is matched by its host and guest path, no matter which server it
is currently provided from.

The ``nfs=`` boot option is updated to no longer list the given
volume(s). If no volume is left the option is deleted from the
configuration such that it is left as it was before the volumes
were added.

As the command only modifies the flake configuration of the
application no privileged operations are required. The NFS
export of the host path, see
**flake-ctl-firecracker-volume-release**(8), and the network
setup, see **flake-ctl-firecracker-network-remove**(8), are not
touched, they may still be needed by other applications or
instances.

INSTANCES
---------

Called with the ``--instance`` option only the volumes configured
for that instance are deleted. The instance section is dropped
along with its last volume, leaving the configuration as it was
before the volumes were added for that instance.

OPTIONS
-------

--app <APP>

  Absolute path of the application on the host

--volume <VOLUME>

  A volume to delete from the application, in the format
  ``/some/local/path:/some/guest/path``. This option can be
  specified multiple times

--instance <INSTANCE>

  The ``@NAME`` instance selector the volumes were added for.
  For convenience the plain name without the ``@`` prefix is
  accepted as well

FILES
-----

* /usr/share/flakes
* $HOME/.config/flakes

EXAMPLE
-------

.. code:: bash

   $ flake-ctl firecracker volume remove \
         --app $HOME/bin/claude --volume /some/local/path:/some/guest/path

SEE ALSO
--------

flake-ctl-firecracker-volume-add(8), flake-ctl-firecracker-volume-release(8), flake-ctl-firecracker-network-remove(8), sci(8)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
