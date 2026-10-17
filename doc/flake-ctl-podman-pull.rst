FLAKE-CTL-PODMAN-PULL(8)
========================

NAME
----

**flake-ctl podman pull** - Fetch container from registry

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl podman pull --uri <URI> [OPTIONS]

   OPTIONS:
       --uri <URI>
       --use-addon-store

DESCRIPTION
-----------

Pull the container from the given registry URI into the local registry.
The command is based on **podman pull**. After completion
the container can be listed via:

.. code:: bash

   $ podman images

OPTIONS
-------

--uri <URI>

  Pull from URI into local podman registry. Consult the
  podman pull documentation for details on the URI format

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

   $ flake-ctl podman pull --uri opensuse/tumbleweed

   $ flake-ctl podman pull --uri opensuse/tumbleweed --use-addon-store

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
