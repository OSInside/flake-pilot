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
       --force
       --help

DESCRIPTION
-----------

Create the user specific setup to run flake applications.

The system wide setup is provided with the flake-pilot package.
A user who wants to register and run flake applications without
root privileges needs its own setup below the home directory.
This is the only setup the command can create. The command
creates the setup as follows:

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

In addition the command checks the meta data directories which
are shared by the pilots of all users, see `podman_ids_dir`,
`firecracker_ids_dir` and `bubblewrap_ids_dir` in /etc/flakes.yml.
Every user keeps a private directory below them. A meta data
directory has to belong to root and must have the permissions
1777, like /tmp. The owner of such a directory is allowed to delete
and replace the private directories of all other users. A
directory which is set up differently is reported as an error,
along with the call to fix it.

The pilots do not create a meta data directory and refuse to run
if it does not exist or is set up differently. The flake-pilot
package lets systemd-tmpfiles create the default meta data
directory /tmp/flakes at boot. A meta data directory which does
not exist is created by the command as root, through sudo for a
caller other than root. Called as the root user, the command only
checks and creates the meta data directories. This is the way to
provide them on a host without systemd, or for a meta data
directory other than /tmp/flakes.

After the setup was created, flake applications can be registered
and run by the respective flake-ctl command. The user mode is
detected automatically from the caller. Every flake-ctl command
called by a user other than root operates on the setup of that
user. To let podman commands operate on the flake storage of
the user, export the storage configuration as it is shown at the
end of the setup:

.. code:: bash

   export CONTAINERS_STORAGE_CONF=$HOME/.config/flakes/podman/storage.conf

OPTIONS
-------

--force

  Create the configuration files from scratch, even if they
  already exist. Existing files are overwritten and adjustments
  made to them are lost

FILES
-----

* /etc/flakes.yml
* /usr/lib/tmpfiles.d/flake-pilot.conf
* /tmp/flakes
* $HOME/.config/flakes.yml
* $HOME/.config/flakes
* $HOME/.config/flakes/podman/storage.conf

EXAMPLE
-------

.. code:: bash

   $ flake-ctl init

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
