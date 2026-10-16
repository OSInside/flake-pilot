FLAKE-CTL-PODMAN-LOAD(8)
========================

NAME
----

**flake-ctl podman load** - Load container to local registry

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl podman load --oci <OCI> [OPTIONS]

   OPTIONS:
       --oci <OCI>
       --use-addon-store


DESCRIPTION
-----------

Load the given OCI image into the local registry. The image is loaded
from exactly the given file path, the command fails if there is no
such file. The command is based on **podman load**. After completion
the container can be listed via:

.. code:: bash

   $ podman images

OPTIONS
-------

--oci <OCI>

  OCI image to load into local podman registry. The given
  container must be in the OCI tar format like it is produced
  when exporting containers from registries via **podman export**

--use-addon-store

  Store the image in the additional read-only image store
  /usr/lib/podman/storage instead of the local podman registry
  /var/lib/podman/storage. No data is written to the local podman
  registry in this case. The add-on store is configured in
  /etc/flakes/storage.conf, the containers of the flakes are
  created from its images but are stored in the local podman
  registry. Old images in the add-on store are not pruned because
  a container of a flake might still use them. The option is only
  supported as root

EXAMPLE
-------

.. code:: bash

   $ flake-ctl podman load --oci SOME.docker.tar

   $ flake-ctl podman load --oci SOME.docker.tar --use-addon-store

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
