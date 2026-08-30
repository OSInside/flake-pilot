FLAKE-CTL-FIRECRACKER-NETWORK-REMOVE(8)
=======================================

NAME
----

**flake-ctl firecracker network remove** - Disconnect a VM application from the host network

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl firecracker network remove [OPTIONS] --app <APP>

   OPTIONS:
       --app <APP>
       --instance <INSTANCE>
       --help

DESCRIPTION
-----------

Delete the network setup of the given firecracker flake
application. All of the setup created with
**flake-ctl-firecracker-network-add**(8) is reverted:

1. The route from the TAP device to the outside world

   .. code:: bash

      sudo iptables -D FORWARD -i tap-<APP> -o <OUTGOING> -j ACCEPT

2. The TAP device of the application

   .. code:: bash

      sudo ip tuntap del tap-<APP> mode tap

   The address of the device and its link state are deleted
   along with it

3. The network setup in the flake configuration

   The ``ip=``, ``rd.route=`` and ``nameserver=`` options are
   deleted from the kernel commandline of the VM. Sections which
   became empty are dropped such that the configuration is the
   same as it was before the setup was created

The address of the application becomes available again and is
handed out to the next application which is connected.

As the setup changes the network configuration of the host, the
commands are called through **sudo**. The calling user therefore
needs the permission to run them as root.

A device or rule which is not present is not deleted. This allows
to call the command more than once and also covers the case that
the setup on the host was already flushed, e.g by a reboot.

The NAT setup of the host is shared by all VM applications and is
therefore not touched. It stays in place until the host is
rebooted or the rules are deleted by other means.

**NOTE:** The command does not check whether the application is
running. Deleting the network setup of a running instance cuts
its connection to the outside world.

INSTANCES
---------

Every instance of an application has its own address and its own
TAP device. Called with the ``--instance`` option only the setup
of that instance is deleted, all other instances stay connected.
The setup which is shared by them, the route to the gateway and
the name server, is deleted with the last one.

OPTIONS
-------

--app <APP>

  Absolute path of the application on the host

--instance <INSTANCE>

  The ``@NAME`` instance selector the network setup was created
  for. For convenience the plain name without the ``@`` prefix is
  accepted as well

FILES
-----

* /usr/share/flakes
* $HOME/.config/flakes
* /etc/flakes/network.yaml
* $HOME/.config/flakes/network.yaml

EXAMPLE
-------

.. code:: bash

   $ flake-ctl firecracker network remove --app $HOME/bin/claude

   $ flake-ctl firecracker network remove --app $HOME/bin/claude --instance @id1

SEE ALSO
--------

flake-ctl-firecracker-network-add(8), flake-ctl-firecracker-network-init(8), firecracker-pilot(8)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
