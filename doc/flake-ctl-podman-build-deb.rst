FLAKE-CTL-PODMAN-BUILD-DEB(8)
=============================

NAME
----

**flake-ctl podman build-deb** - Build debian package from OCI image

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl podman build-deb [OPTIONS] --oci <OCI> --repo <REPO>

   OPTIONS:
       --app <APP>...
       --oci <OCI>
       --repo <REPO>

DESCRIPTION
-----------

The build-deb command takes an OCI tar container and packages it into a debian (.deb)
package. The produced package will be placed into a local debian repository such
that tools like **apt** can consume it. If provided via the **--app** option, the
package provides post install and removal scripts which registers/removes the
application for the container at install/uninstall time of the package.

OPTIONS
-------

--app <APP>...

  An absolute path to the application on the host
  and optional absolute path to the application in the
  container. The path spec is separated by a semicolon.
  This option can be specified multiple times.

  For example:

  --app /usr/bin/myapp;/usr/bin/ls

  Registers /usr/bin/myapp and calls /usr/bin/ls inside

  --app /usr/bin/aws;/

  Registers /usr/bin/aws and calls the default entrypoint

--oci <OCI>

  OCI image to load into local podman registry

--repo <REPO>

  Output directory to store package(s) as local debian repository

FILES
-----

* /usr/share/podman-pilot/container.spec.in
* /usr/bin/oci-deb

EXAMPLE
-------

.. code:: bash

   $ flake-ctl podman build-deb --oci SOME.docker.tar \
       --repo ${HOME}/localrepo \
       --app /usr/bin/myapp

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
