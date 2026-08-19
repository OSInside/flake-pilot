FLAKE-CTL-INIT(8)
=================

NAME
----

**flake-ctl init** - Create the setup to run flake applications

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl init [OPTIONS]

   OPTIONS:
       --user
       --force
       --help

DESCRIPTION
-----------

Create the user specific setup to run flake applications.

The system wide setup is provided with the flake-pilot package.
A user who wants to register and run flake applications without
root privileges needs its own setup below the home directory.
The command creates it as follows:

1. The flakes directory of the calling user, $HOME/.config/flakes,
   with the sub directories `podman` and `firecracker` used by the
   respective engine

2. The user specific flakes configuration file
   $HOME/.config/flakes.yml. It points the flake registry and the
   podman storage of the user to the directories created in the
   first step. The meta data directories of the pilots are taken
   from the system wide configuration file /etc/flakes.yml

3. The podman storage configuration file
   $HOME/.config/flakes/podman/storage.conf. It points podman to
   a storage location of the calling user

The created files are meant to be adjusted as desired. Therefore
a file which already exists is kept and only reported unless the
**--force** option is given.

After the setup was created, flake applications can be registered
and run with the **--user** option of the respective flake-ctl
command. To let podman commands operate on the flake storage of
the user, export the storage configuration as it is shown at the
end of the setup:

.. code:: bash

   export CONTAINERS_STORAGE_CONF=$HOME/.config/flakes/podman/storage.conf

OPTIONS
-------

--user

  Create the setup for the calling user. This is the only setup
  the command can create. Calling it as the root user is an error

--force

  Create the configuration files from scratch, even if they
  already exist. Existing files are overwritten and adjustments
  made to them are lost

FILES
-----

* /etc/flakes.yml
* $HOME/.config/flakes.yml
* $HOME/.config/flakes
* $HOME/.config/flakes/podman/storage.conf

EXAMPLE
-------

.. code:: bash

   $ flake-ctl init --user

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
