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
       --user <USER>
       --help

DESCRIPTION
-----------

Connect the given firecracker flake application to the network of
the host. The flake configuration of the application is looked up
from the given application path and receives a static network
setup. The TAP device the application expects is created for the
user who runs the VM and is connected to the firewall setup of the
host created by **flake-ctl-firecracker-network-init**\(8).

The command performs the following steps:

1. Write the network setup to the flake configuration

   The kernel commandline of the VM is extended by the options
   which configure its network:

   .. code:: yaml

      boot_args:
        - ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off
        - rd.route=172.16.0.1/24::eth0
        - nameserver=8.8.8.8

   Gateway and netmask describe the private network between the
   host and the VMs. That network is the one selected with
   **flake-ctl-firecracker-network-init**(8), the example above
   shows the preferred network ``172.16.0.0/24``. The name server
   is the public name server ``8.8.8.8``, compiled into the command.
   If the host setup was created with ``--host-resolver`` it is the
   gateway, e.g ``nameserver=172.16.0.1``. The address is allocated
   in the network state file, see ADDRESSES below

2. Create the TAP device of the application for the user with the
   ID UID who runs the VM, see USERS below, and switch IPv6 off on it

   .. code:: bash

      sudo /usr/sbin/ip tuntap add tap-<UID>_<APP> mode tap user <UID>
      sudo /usr/sbin/sysctl -w net.ipv6.conf.tap-<UID>_<APP>.disable_ipv6=1

   firecracker runs as the user who calls the application. Only the
   user the device belongs to can connect a VM to it. The firewall
   setup of the host is IPv4 only, without IPv6 on the device a VM
   cannot reach the host through an IPv6 link local address

3. Make the TAP device the gateway of the VM

   .. code:: bash

      sudo /usr/sbin/ip addr add 172.16.0.1/24 dev tap-<UID>_<APP>
      sudo /usr/sbin/ip link set tap-<UID>_<APP> up

4. Route the address of the application to its TAP device

   .. code:: bash

      sudo /usr/sbin/ip route add 172.16.0.2/32 dev tap-<UID>_<APP>

   Every TAP device provides the same gateway address and thus
   the same route to the private network. Without a host route
   for the address of the application the traffic to it would be
   sent to the device that route points to, which is not
   necessarily the device the application is connected to

5. Allow the VM the DHCP and NFS service of the host

   .. code:: bash

      sudo /usr/sbin/iptables -I FLAKE_PILOT_INPUT 1 -i tap-<UID>_<APP> -p udp --sport 68 --dport 67 -j ACCEPT
      sudo /usr/sbin/iptables -I FLAKE_PILOT_INPUT 1 -i tap-<UID>_<APP> -s 172.16.0.2 -d 172.16.0.1 -p tcp --dport 2049 -j ACCEPT

   If the host setup was created with ``--host-resolver``, see
   **flake-ctl-firecracker-network-init**\(8), the VM is allowed
   the DNS service of the host as well:

   .. code:: bash

      sudo /usr/sbin/iptables -I FLAKE_PILOT_INPUT 1 -i tap-<UID>_<APP> -s 172.16.0.2 -d 172.16.0.1 -p udp --dport 53 -j ACCEPT
      sudo /usr/sbin/iptables -I FLAKE_PILOT_INPUT 1 -i tap-<UID>_<APP> -s 172.16.0.2 -d 172.16.0.1 -p tcp --dport 53 -j ACCEPT

   Any other traffic of the VM to the host is dropped by the firewall
   setup. NFS is allowed in version 4, which needs no other port,
   for the volumes exported to the VM with
   **flake-ctl-firecracker-volume-export**\(8). Apart from DHCP, which
   is used before the VM has an address, the traffic has to come from
   the address of the VM and through its own TAP device. A VM can
   therefore not use the services of the host in the name of another
   VM

As the setup changes the network configuration of the host, the
commands are called through **sudo**. The calling user therefore
needs the permission to run them as root.

Private network and outgoing interface are the ones the host setup
was created for with **flake-ctl-firecracker-network-init**(8). If
there is no record of them, e.g because the host setup was created
manually, the interface of the default route and a free private
network are used.

The command requires the firewall setup of the host to be active
and refuses to run otherwise. Please run
**flake-ctl firecracker network init** first, e.g after a reboot of
the host.

A device, address, route or rule which is already present is not
created again. This allows to call the command more than once, e.g to
create the setup again after a reboot of the host. Only the flake
configuration and the address allocation are persistent, the setup
on the host is not.

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

* the TAP device is named after the user, the application and the
  selector, e.g ``tap-1000_claude`` for ``claude`` and the user with
  the ID ``1000``. A name which is longer than the kernel allows for
  network interfaces is shortened and made unique by a hash of the
  full name, e.g ``tap-1000_1a2b3c`` for ``claude @id1``

USERS
-----

The TAP device of an instance belongs to the user who runs the VM,
which is the calling user unless ``--user`` names another one. A
flake registered by a user is only run by that user, ``--user`` can
therefore only name another user for a system wide flake. Such a
flake is set up by root, who names the user who is going to run it:

.. code:: bash

   $ sudo flake-ctl firecracker network add --app /usr/bin/claude --user tux

The address of an instance is part of the flake configuration. The
same instance of a system wide flake can therefore only be connected
for one user at a time, the command refuses to connect it for a
second one. Give every user an ``@NAME`` instance of their own
instead:

.. code:: bash

   $ sudo flake-ctl firecracker network add --app /usr/bin/claude --instance @tux --user tux
   $ sudo flake-ctl firecracker network add --app /usr/bin/claude --instance @wilber --user wilber

ADDRESSES
---------

The addresses are allocated in the network state file
``/var/lib/firecracker/network-state.yaml``, which records the
address, the TAP device, the user and the flake of every connected
instance of all users. The file belongs to root and is written
through **sudo**, a user cannot hand out or take over the address of
another user. A state file which does not belong to root is refused.
Calls which change the network setup can run at the same time, e.g
for different users. Each of them holds an exclusive lock on
``/var/lib/firecracker/network-state.lock`` while it runs, such that
the same address is never handed out twice. A call waits for the lock
if another call holds it. The lock file belongs to root as well and is
created through **sudo** by the first call which needs it.

The address handed out to an instance is the lowest free address of
the private network of the host setup, which is ``172.16.0.0/24``
unless the host uses that network already. An address which is
allocated to another TAP device is never handed out twice. An
instance which is already connected keeps the address allocated to
its TAP device, an instance which has an address in its flake
configuration keeps that one as long as it is not allocated to
another device. Addresses configured in flake registrations which
were connected before the addresses got allocated in the state file
are not handed out either, as far as those registrations can be read.

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

--user <USER>

  Name of the user who runs the VM and owns its TAP device. Defaults
  to the calling user. Another user can only be named for a system
  wide flake, see USERS

FILES
-----

* /usr/share/flakes
* $HOME/.config/flakes
* /etc/flakes/network.yaml
* $HOME/.config/flakes/firecracker/network.yaml
* /var/lib/firecracker/network-state.yaml
* /var/lib/firecracker/network-state.lock

EXAMPLE
-------

.. code:: bash

   $ flake-ctl firecracker network add --app $HOME/bin/claude

   $ flake-ctl firecracker network add --app $HOME/bin/claude --instance @id1

SEE ALSO
--------

flake-ctl-firecracker-network-init(8), flake-ctl-firecracker-network-remove(8), flake-ctl-firecracker-network-cleanup(8), flake-ctl-firecracker-register(8), firecracker-pilot(8)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
