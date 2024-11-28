FLAKE-CTL-PODMAN-REGISTER(8)
============================

NAME
----

**flake-ctl podman register** - Register container application

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl podman register [OPTIONS] --container <CONTAINER> --app <APP>

   OPTIONS:
       --app <APP>
       --attach
       --base <BASE>
       --check_host_dependencies
       --container <CONTAINER>
       --include-tar <INCLUDE_TAR>...
       --include-path <INCLUDE_PATH>...
       --info
       --layer <LAYER>...
       --opt <OPT>...
       --resume
       --target <TARGET>

DESCRIPTION
-----------

Register the given application to run inside of the specified container.
The registration process is two fold:

1. Create the application symlink pointing to `/usr/bin/podman-pilot`
2. Create the application default configuration below `/usr/share/flakes`.
   Each application registered is called a **flake**

On successful completion the registered *--app* name can be called
like a normal application on this host.

For further details about the flake configuration please refer to
the **podman-pilot** manual page.

NOTE
----

References made to the name of a container or container name imply the
path of the container as it is known in the local registry. The value shown
in the **REPOSITORY** column shown by the **podman images** command.

OPTIONS
-------

--app <APP>

  An absolute path to the application on the host. If not
  specified via the target option, the application will be
  called with that path inside of the container

--attach

  Attach to the container if still running, rather than executing
  the app again. Only makes sense for interactive sessions like a
  shell application

--base <BASE>

  Name of the base container. The name must match with a name in
  the local podman registry. Applications registered with a base
  container are merged into one prior creating the container
  instance. Using of this option is only useful if the specified
  container name references a delta container which was built
  against the specified base container. Such delta containers
  can be created with KIWI.

--check_host_dependencies

  Check if the container has dependencies to the host When using
  a base container this check is enabled by default.

--include-tar <INCLUDE_TAR>...

  Name of a tar file to be included on top of the container instance.
  This option can be specified multiple times

--include-path <INCLUDE_PATH>...

  Name of a file or directory to be included on top of the container
  instance. This option can be specified multiple times

--info

  Print registration information from container if provided

--layer <LAYER>...

  Name of an additional container layer on top of the specified
  base container. This option can be specified multiple times. The
  resulting layer list is evaluated in the order of the arguments
  as they were provided on the command line

--opt <OPT>...

  Container runtime option, and optional value, used to create the
  container. This option can be specified multiple times.
  If no options are specified the container always starts with
  terminal emulation activated "-ti". In addition if none of
  --resume or --attach is set, the container will be deleted by
  default "--rm". If runtime option(s) are specified none of the
  default settings will apply. See the example section for further
  details.

--resume

  Resume the container from previous execution. If the container is
  still running, the app will be executed inside of this container
  instance

--container <CONTAINER>

  A container name. The name must match with a name in the local
  podman registry

--target <TARGET>

  An absolute path to the application in the container. Use this option
  if the application path on the host should be different to the
  application path inside of the container. Set this option to an empty string
  if the default entrypoint of the container should
  be called.

FILES
-----

* /usr/share/flakes
* /etc/flakes

EXAMPLE
-------

.. code:: bash

   $ flake-ctl podman register --container SOME_APT_CONTAINER \
       --app /usr/bin/apt-get

   $ flake-ctl podman register --container SOME_APT_CONTAINER \
       --app /usr/bin/apt-get \
       --opt '\-ti' \
       --opt '\--rm' \
       --opt '\--storage-opt size=10G'

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
