FLAKE-CTL-FIRECRACKER-NETWORK-CLEANUP(8)
========================================

NAME
----

**flake-ctl firecracker network cleanup** - Delete the network setup of the host

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl firecracker network cleanup

   OPTIONS:
       --help

DESCRIPTION
-----------

Delete the network setup of the host which was created with
**flake-ctl-firecracker-network-init**\(8) and
**flake-ctl-firecracker-network-add**\(8). The command performs the
following steps:

1. Delete the TAP devices of all connected applications of all users,
   as recorded in the network state file. The address of a device,
   its link state and the host route to the application behind it
   are deleted along with it

2. Delete the firewall setup. The jump rules into its chains are
   deleted first, the chains along with their rules afterwards

   .. code:: bash

      sudo /usr/sbin/iptables -t nat -D POSTROUTING -j FLAKE_PILOT_NAT
      sudo /usr/sbin/iptables -D FORWARD -j FLAKE_PILOT_FORWARD
      sudo /usr/sbin/iptables -D INPUT -i tap-+ -j FLAKE_PILOT_INPUT
      sudo /usr/sbin/iptables -t nat -F FLAKE_PILOT_NAT
      sudo /usr/sbin/iptables -t nat -X FLAKE_PILOT_NAT
      ...

3. Delete the rules for single TAP devices former versions of
   flake-pilot created, and report the rules for all traffic former
   versions created, see **flake-ctl-firecracker-network-init**\(8)

4. Switch IPv4 forwarding off again, if it was switched on by
   **flake-ctl-firecracker-network-init**\(8)

   .. code:: bash

      sudo /usr/sbin/sysctl -w net.ipv4.ip_forward=0

   If IP forwarding was already switched on before, it is left alone

The address allocations in the network state file and the network
setup in the flake configurations are kept. The host is left in the
state it has after a reboot: calling
**flake-ctl-firecracker-network-init**\(8) and
**flake-ctl-firecracker-network-add**\(8) again connects the
applications with the addresses they had before. To delete the setup
of an application completely, use
**flake-ctl-firecracker-network-remove**\(8) instead.

As the setup changes the network configuration of the host, the
commands are called through **sudo**. The calling user therefore
needs the permission to run them as root.

A device, rule or chain which is not present is not deleted. This
allows to call the command more than once and also covers the case
that the setup on the host was already flushed, e.g by a reboot.

**NOTE:** The command does not check whether applications are
running. Deleting the network setup cuts the connection of all
running VMs to the outside world.

FILES
-----

* /proc/sys/net/ipv4/ip_forward
* /etc/flakes/network.yaml
* $HOME/.config/flakes/firecracker/network.yaml
* /var/lib/firecracker/network-state.yaml

EXAMPLE
-------

.. code:: bash

   $ flake-ctl firecracker network cleanup

SEE ALSO
--------

flake-ctl-firecracker-network-init(8), flake-ctl-firecracker-network-add(8), flake-ctl-firecracker-network-remove(8), firecracker-pilot(8)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
