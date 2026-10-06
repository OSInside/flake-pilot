FLAKE-CTL-FIRECRACKER-REMOVE(8)
===============================

NAME
----

**flake-ctl firecracker remove** - Remove application registration and/or entire VM

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl firecracker remove <--vm <VM>|--app <APP>>

   OPTIONS:
       --app <APP>
       --vm <VM>

DESCRIPTION
-----------

Remove registration(s). The registry to remove from is detected
from the caller. Called as any user other than root the user
specific, rootless registry of that user is used.

The command operates in two modes:

1. Remove an application registration provided via **--app**

   In this mode the command deletes the specified application if it
   is a link pointing to `/usr/bin/firecracker-pilot`. It then also
   deletes the application configuration from `/usr/share/flakes`
   and the drop-in directory `/etc/flakes/APP.d`, respectively from
   `~/.config/flakes` in user mode. Finally the
   meta data of the instances of the application is deleted, see
   below

2. Remove a VM including all its registered applications via **--vm**

   In this mode the command deletes all application registrations
   using the specified VM along with the meta data of their
   instances. At the end also the specified VM will be removed
   from the local firecracker registry

**firecracker-pilot**(8) keeps the meta data of an instance after
the instance is gone and reuses it when the instance is started
again. This is the VM ID file and the vsock sockets of the
instance which are stored below `/tmp/flakes` in a private
directory of the user the instance belongs to, e.g
`/tmp/flakes/1000/myapp.vmid` and `/tmp/flakes/1000/sci_cmd_myapp.sock`.
The meta data is deleted only when the flake it belongs to gets
removed. This covers the instance of the application itself and
the instances started with the pilot option **@NAME**, e.g
`myapp@one.vmid`, for all users whose meta data directory can be
read. The meta data of other flakes is not touched. The storage
volume of an instance, see **flake-ctl-firecracker-show**(8), is
not deleted

A registration which is still in use is not removed. This is the
case if

* an instance of the flake is still running, as it is shown by
  **flake-ctl-firecracker-show**(8). The VM would stay behind
  without the configuration it was created from. Stop the
  instance(s) first, the registration can be removed afterwards

* a TAP device of the flake is still present on the host, see
  **flake-ctl-firecracker-network-add**(8). The device belongs to
  the network configuration of the flake and has to be deleted
  with **flake-ctl-firecracker-network-remove**(8) first. The
  command reports the call to use for each of the devices

OPTIONS
-------

--app <APP>

  Application absolute path to be removed from host

--vm <VM>

  VM basename as provided via **ls -1 /var/lib/firecracker/images**
  respectively **ls -1 ~/.config/flakes/firecracker/images**
  in user mode

FILES
-----

* /usr/share/flakes
* /etc/flakes/APP.d
* /var/lib/firecracker/images
* ~/.config/flakes
* ~/.config/flakes/firecracker/images

EXAMPLE
-------

.. code:: bash

   $ flake-ctl firecracker remove --app /usr/bin/apt-get

   $ flake-ctl firecracker remove --vm SOME_FIRECRACKER_VM

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
