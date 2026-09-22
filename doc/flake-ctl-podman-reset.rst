FLAKE-CTL-PODMAN-RESET(8)
=========================

NAME
----

**flake-ctl podman reset** - Stop and delete the container instance of a resume flake

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl podman reset [OPTIONS] --app <APP>

   OPTIONS:
       --app <APP>
       --instance <INSTANCE>
       --help

DESCRIPTION
-----------

Stop and delete the container instance of an application which
was registered with the **--resume** option of
**flake-ctl-podman-register**(8).

A resume flake does not create a new container for every call of
the application. The container of the first call is kept in
running state and all further calls are done inside of that
instance. This is done by starting the container with a **sleep**
process as its entry point which keeps it up. The application
itself is then called through **podman exec**.

Consequently the instance of a resume flake outlives the
application and keeps whatever it has written to its file system.
This command deletes that instance which lets the next call of
the application start from a freshly created container.

The command only operates on a registration which is configured
with ``resume: true``. For any other registration it exits with
an error, there is no instance which would survive the call of
the application.

Resetting an instance means to:

1. kill the **sleep** process which keeps the container of the
   instance in running state
2. stop the container
3. delete the container
4. delete the container ID file of the instance below `/tmp/flakes`

An application which is not running provides no instance. In
this case the command has nothing to do and succeeds.

OPTIONS
-------

--app <APP>

  An absolute path to the application on the host. The
  application must be registered as a container application
  with the resume option set

--instance <INSTANCE>

  The **@NAME** instance selector the application is called
  with. Each instance of a resume flake runs in its own
  container, therefore the command has to be called for each of
  them. Without this option the container of the application
  itself is deleted. For convenience the selector is also
  accepted without its leading **@** marker

FILES
-----

* /tmp/flakes
* /usr/share/flakes
* $HOME/.config/flakes
* /etc/flakes.yml

EXAMPLE
-------

.. code:: bash

   $ flake-ctl podman reset --app /usr/bin/mybash

   $ flake-ctl podman reset --app /usr/bin/mybash --instance one

SEE ALSO
--------

flake-ctl-podman-register(8), flake-ctl-podman-show(8), podman-pilot(8)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
