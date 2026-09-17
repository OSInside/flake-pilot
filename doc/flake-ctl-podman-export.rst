FLAKE-CTL-PODMAN-EXPORT(8)
==========================

NAME
----

**flake-ctl podman export** - Export container to directory

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl podman export --container <CONTAINER> --directory <DIRECTORY> [OPTIONS]

   OPTIONS:
       --container <CONTAINER>
       --directory <DIRECTORY>
       --force

DESCRIPTION
-----------

Export the file system of the given container into the given directory.
The container must exist in the local podman registry and can be
listed via:

.. code:: bash

   $ podman images

The export creates a container instance from the given container,
reads its file system through **podman export** and unpacks the data
into the directory. The instance is not started and is deleted after
the export. The sequence is equivalent to:

.. code:: bash

   $ podman create --name INSTANCE CONTAINER
   $ podman export INSTANCE | tar -x -C DIRECTORY
   $ podman rm INSTANCE

The directory is created if it does not exist yet. A directory which
exists is taken as an export which was done before. In this case the
container is not exported again unless the **--force** option is
given. An export which failed does not leave the created directory
behind, thus a directory only exists if the export in it is complete.

The resulting directory tree is a plain file system tree and not an
OCI container anymore. It can for example be used as the root file
system of a sandbox application, see **flake-ctl-bubblewrap-register**(8)

OPTIONS
-------

--container <CONTAINER>

  A container name. The name must match with a name in the
  local podman registry

--directory <DIRECTORY>

  Path to the directory the file system of the container is
  exported to. The directory is created if it does not exist
  yet

--force

  Export the container even if the given directory exists. The
  file system of the container is unpacked on top of the contents
  of that directory. Files which exist in both, the directory and
  the container, are taken from the container. All other files of
  the directory stay as they are

EXAMPLE
-------

.. code:: bash

   $ flake-ctl podman export --container leap --directory /var/lib/rootfs/leap

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
