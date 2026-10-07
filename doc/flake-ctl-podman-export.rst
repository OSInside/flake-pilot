FLAKE-CTL-PODMAN-EXPORT(8)
==========================

NAME
----

**flake-ctl podman export** - Export container or OCI tarball to directory

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl podman export --container <CONTAINER> --directory <DIRECTORY> [OPTIONS]
       flake-ctl podman export --oci <OCI> --directory <DIRECTORY> [OPTIONS]

   OPTIONS:
       --container <CONTAINER>
       --oci <OCI>
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

Instead of a container from the local podman registry the image
can also be read from an OCI compliant tarball given via the **--oci**
option. In this case the local podman registry is not used. Both, the
OCI image layout and the docker archive layout as written by
**podman save** are supported, the tarball itself can be compressed.
The layers of the image are unpacked in order into the directory,
including the whiteout entries of a layer which delete data of the
layers below. Symlinks of the image are resolved inside of the
directory, a layer cannot write to a location outside of it. For an
image providing more than one platform the image matching the
architecture of the host is used. The data is unpacked through a
temporary workspace which is created next to the directory and
deleted after the export.

The directory is created if it does not exist yet. A directory which
exists is taken as an export which was done before. In this case the
export is not done again unless the **--force** option is
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

--oci <OCI>

  Path to an OCI compliant tarball. The file system of the image
  in the tarball is unpacked without the use of the local podman
  registry. The option cannot be combined with **--container**

--directory <DIRECTORY>

  Path to the directory the file system of the container or OCI
  tarball is exported to. The directory is created if it does not
  exist yet

--force

  Export even if the given directory exists. The file system of
  the container or OCI tarball is unpacked on top of the contents
  of that directory. Files which exist in both, the directory and
  the container, are taken from the container. All other files of
  the directory stay as they are

EXAMPLE
-------

.. code:: bash

   $ flake-ctl podman export --container leap --directory /var/lib/rootfs/leap

   $ flake-ctl podman export --oci leap.oci.tar --directory /var/lib/rootfs/leap

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
