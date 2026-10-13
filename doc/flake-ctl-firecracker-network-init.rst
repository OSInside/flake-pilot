FLAKE-CTL-FIRECRACKER-NETWORK-INIT(8)
=====================================

NAME
----

**flake-ctl firecracker network init** - Prepare the host for NAT networking

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl firecracker network init [--outgoing-interface <OUTGOING_INTERFACE>]

   OPTIONS:
       --outgoing-interface <OUTGOING_INTERFACE>
       --help

DESCRIPTION
-----------

Set up the host such that firecracker VM applications can reach the
outside world.

Firecracker connects a VM to the host through a TUN/TAP device. Such
a device is a host local endpoint and provides no connection beyond
the host by itself. Routing its traffic further requires the host to
act as a router and to translate the sender address of the VM traffic
to an address which is reachable from the outside. The command creates
this setup by:

1. Selecting the private network between the host and the VMs, see
   NETWORK below

2. Enabling IPv4 forwarding on the host, if it is switched off

   .. code:: bash

      sudo /usr/sbin/sysctl -w net.ipv4.ip_forward=1

   That flake-pilot switched it on is recorded in the network state
   file, **flake-ctl-firecracker-network-cleanup**\(8) switches it
   off again. If it was switched on before, it is left alone.

3. Creating the firewall setup, see FIREWALL below. The traffic of the
   VMs leaving through the outgoing interface is masqueraded. This is
   the given interface or, if none is given, the interface of the IPv4
   default route of the host

The traffic of the VMs to the outside world appears as if it would
originate from the outgoing interface. As the setup changes the
network configuration of the host, the commands are called through
**sudo**. The calling user therefore needs the permission to run them
as root.

The outgoing interface and the selected network are recorded in the
network configuration file. Connecting an application to the host
network with **flake-ctl-firecracker-network-add**(8) reads them from
there, takes the address of the application from the same network and
routes its traffic to the same interface.

Chains and rules which already exist are not created again. This
allows to call the command more than once without stacking up
duplicates of the same rule. The setup is not persistent, it has to be
created again after a reboot of the host.

**NOTE:** The setup assumes there is no other firewall software
active on the host and serves as an example setup. If the firewall
of the host is managed by another tool, please refer to the
documentation of that tool on how to create the NAT/postrouting
rules and do not use this command.

Setting up the host is only one part of the networking setup. The
network configuration of the VM itself is part of the flake
configuration of the application. See **flake-ctl-firecracker-register**(8)
and the ``Firecracker Networking`` section of the flake-pilot README
for the complete setup.

FIREWALL
--------

All rules of the setup live in chains of their own. Each chain is
entered by one jump rule, inserted at the top of the builtin chain of
the same purpose. The rules only match the traffic of the private
network and of the TAP devices of the VMs, all other traffic of the
host returns to the builtin chains unchanged. The setup looks as
follows, shown in the notation of ``iptables -S``:

.. code:: bash

   # table nat
   -A POSTROUTING -j FLAKE_PILOT_NAT
   -A FLAKE_PILOT_NAT -s <NETWORK> -o <OUTGOING_INTERFACE> -j MASQUERADE

   # table filter
   -A FORWARD -j FLAKE_PILOT_FORWARD
   -A FLAKE_PILOT_FORWARD -o tap-+ -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT
   -A FLAKE_PILOT_FORWARD -i tap-+ -s <NETWORK> -o <OUTGOING_INTERFACE> -j ACCEPT
   -A FLAKE_PILOT_FORWARD -i tap-+ -j DROP
   -A FLAKE_PILOT_FORWARD -o tap-+ -j DROP

   -A INPUT -i tap-+ -j FLAKE_PILOT_INPUT
   -A FLAKE_PILOT_INPUT -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT
   -A FLAKE_PILOT_INPUT -j DROP

The effect of the setup is:

* Only traffic of the private network of the VMs which leaves through
  the outgoing interface is masqueraded and forwarded. The answers to
  it pass back to the VMs.

* Traffic between two VMs, traffic from the outside world to a VM and
  traffic from a VM to any other interface of the host is dropped.

* Traffic from a VM to the host itself is dropped, except for answers
  to traffic of the host. **flake-ctl-firecracker-network-add**\(8)
  allows each VM the DHCP, DNS and NFS service of the host through its
  gateway, see there.

The chains are deleted as a whole by
**flake-ctl-firecracker-network-cleanup**\(8).

Former versions of flake-pilot created their rules in the builtin
chains. Their rules for single TAP devices are deleted by this
command. A MASQUERADE rule for all traffic of the outgoing interface
and a FORWARD rule accepting all answers, which former versions
created as well, cannot be told apart from rules created by other
means. If one of them is found, it is reported along with the command
to delete it.

NETWORK
-------

The VMs of a host live in one private network which only exists
between the TAP devices of the instances and the VMs behind them. The
preferred network is ``172.16.0.0/24``. If the host is connected to a
network which overlaps with it, the next free ``/24`` network of the
private address space is taken instead, in the order:

* ``172.16.0.0`` ... ``172.31.255.0``
* ``192.168.0.0`` ... ``192.168.255.0``
* ``10.0.0.0`` ... ``10.255.255.0``

The networks of the host are read from the addresses of its
interfaces and from its routing table, the TAP devices of the flakes
are left out. The first address of the selected network, e.g
``172.16.0.1``, becomes the gateway of the VMs and is configured on
every TAP device.

The selected network is recorded and a recorded network is kept as
long as the host does not use it. This keeps the applications which
are connected to it reachable and allows to create the host setup
again, e.g after a reboot, without moving the applications to another
network. Only a recorded network which collides with a network of the
host, e.g because the host was connected to it afterwards, is
replaced. In that case the applications have to be connected again
with **flake-ctl-firecracker-network-add**(8) to receive an address
of the new network.

OPTIONS
-------

--outgoing-interface <OUTGOING_INTERFACE>

  Name of the host interface the traffic of the VMs is routed to
  the outside world through, e.g ``eth0``. If not specified, the
  interface of the IPv4 default route is used, as shown by:

  .. code:: bash

     ip -4 route show default

  If the host has more than one default route, the one with the
  lowest metric is taken. Without a default route the interface
  has to be specified

FILES
-----

* /proc/sys/net/ipv4/ip_forward
* /etc/flakes/network.yaml
* $HOME/.config/flakes/firecracker/network.yaml
* /var/lib/firecracker/network-state.yaml

EXAMPLE
-------

.. code:: bash

   $ flake-ctl firecracker network init

   $ flake-ctl firecracker network init --outgoing-interface eth0

SEE ALSO
--------

flake-ctl-firecracker-network-add(8), flake-ctl-firecracker-network-remove(8), flake-ctl-firecracker-network-cleanup(8), flake-ctl-firecracker-register(8), firecracker-pilot(8)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
