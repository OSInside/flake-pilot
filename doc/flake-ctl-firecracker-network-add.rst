FLAKE-CTL-FIRECRACKER-NETWORK-ADD(8)
====================================

NAME
----

**flake-ctl firecracker network add** - Connect a VM application to the host network

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl firecracker network add [OPTIONS] --app <APP>

   OPTIONS:
       --app <APP>
       --instance <INSTANCE>
       --help

DESCRIPTION
-----------

Connect the given firecracker flake application to the network of
the host. The flake configuration of the application is looked up
from the given application path and receives a static network
setup. The TAP device the application expects is created and
routed to the outgoing interface of the host.

The command performs the following steps:

1. Write the network setup to the flake configuration

   The kernel commandline of the VM is extended by the options
   which configure its network:

   .. code:: yaml

      boot_args:
        - ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off
        - rd.route=172.16.0.1/24::eth0
        - nameserver=8.8.8.8

   Gateway, netmask and name server are not configurable, they
   describe the private network between the host and the VMs and
   are compiled into the command. The address is a free address
   of that network, see ADDRESSES below

2. Create the TAP device of the application

   .. code:: bash

      sudo ip tuntap add tap-<APP> mode tap

3. Connect the TAP device to the outgoing interface

   .. code:: bash

      sudo ip addr add 172.16.0.1/24 dev tap-<APP>
      sudo ip link set tap-<APP> up
      sudo iptables -A FORWARD -i tap-<APP> -o <OUTGOING> -j ACCEPT

As the setup changes the network configuration of the host, the
commands are called through **sudo**. The calling user therefore
needs the permission to run them as root.

The outgoing interface is the one the host setup was created for
with **flake-ctl-firecracker-network-init**(8). If there is no
record of it, e.g because the host setup was created manually, the
interface of the default route is used.

The steps only take effect if the host is prepared for NAT
networking. Please run **flake-ctl firecracker network init**
first.

A device, address or rule which is already present is not created
again. This allows to call the command more than once, e.g to
create the setup again after a reboot of the host. Only the flake
configuration is persistent, the setup on the host is not.

Please note, the network setup of a VM also requires an initrd
which provides support for ``systemd-(networkd, resolved)`` and
which was created by ``dracut`` such that the ``boot_args`` become
effective. See the ``Firecracker Networking`` section of the
flake-pilot README for the complete picture.

INSTANCES
---------

A TAP device cannot be shared across VM instances. Therefore every
instance of an application, created by calling it with the
``@NAME`` selector, needs its own address and its own TAP device.
Called with the ``--instance`` option the command creates the setup
for that instance:

* the address is written to the ``instance`` section of the flake
  configuration, keyed by the selector. It takes the place of the
  global ``ip=`` setting when the application is called with
  that selector

* the TAP device is named after the application and the selector,
  e.g ``tap-claude_id1`` for ``claude @id1``

ADDRESSES
---------

The address handed out to an application is the lowest free
address of the ``172.16.0.0/24`` network. An address which is
already used by another flake registration is never handed out
twice, and an application which is already configured keeps the
address it has.

Only registrations which can be read take part in this. In user
mode these are the registrations of the calling user and the
system wide ones.

OPTIONS
-------

--app <APP>

  Absolute path of the application on the host. The application
  has to be registered as a VM application, see
  **flake-ctl-firecracker-register**(8)

--instance <INSTANCE>

  The ``@NAME`` instance selector the application is called with.
  For convenience the plain name without the ``@`` prefix is
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

   $ flake-ctl firecracker network add --app $HOME/bin/claude

   $ flake-ctl firecracker network add --app $HOME/bin/claude --instance @id1

SEE ALSO
--------

flake-ctl-firecracker-network-init(8), flake-ctl-firecracker-network-remove(8), flake-ctl-firecracker-register(8), firecracker-pilot(8)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
