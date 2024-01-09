FLAKE-CTL-PODMAN-PULL(8)
========================

NAME
----

**flake-ctl podman pull** - Fetch container from registry

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl podman pull --uri <URI>

   OPTIONS:
       --uri <URI>

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

EXAMPLE
-------

.. code:: bash

   $ flake-ctl podman pull --uri opensuse/tumbleweed

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
