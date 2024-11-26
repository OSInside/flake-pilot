FLAKE-CTL-PODMAN-LOAD(8)
========================

NAME
----

**flake-ctl podman load** - Load container to local registry

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl podman load --oci <OCI>

   OPTIONS:
       --oci <OCI>


DESCRIPTION
-----------

Load the given OCI image into the local registry. If the provided
file path cannot be found and attempt is made to match the image using
the provided path as the base for a glob. If multiple images match the
glob the highest image in alpha numerical order will be loaded.
The command is based on **podman load**. After completion
the container can be listed via:

.. code:: bash

   $ podman images

OPTIONS
-------

--oci <OCI>

  OCI image to load into local podman registry. The given
  container must be in the OCI tar format like it is produced
  when exporting containers from registries via **podman export**

EXAMPLE
-------

.. code:: bash

   $ flake-ctl podman load --oci SOME.docker.tar

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
