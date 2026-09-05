.. _troubleshooting:

=================================
Troubleshooting and Known Issues
=================================

.. hint:: **Abstract**

   This chapter collects the switches which show what a pilot is
   doing and the issues which are known to get in the way.

Debugging a Flake
=================

Both pilots report the details of their work if the following
environment variable is set:

.. code-block:: bash

   PILOT_DEBUG=1 myapp

The output shows which flake configuration was read, which engine
command was constructed from it and, for VM applications, which
kernel commandline and which TAP device name the instance is started
with. This is the first thing to look at whenever an application does
not behave as expected.

Known Issues
============

selinux
-------

The security profiles of selinux often prevents operations done by
the pilots. In case of trouble and to check if selinux might be the
cause, try to temporarily disable selinux as follows:

.. code-block:: bash

   sudo setenforce 0

For container based flakes, the selinux context of the container
image might also be the cause of problems. In this case, try to run
the container with the ``--security-opt label=disable`` option. This
can be done by passing the following option to the flake registration
command:

.. code-block:: bash

   --opt "\--security-opt label=disable"

firewalld
---------

The NFS export created by ``flake-ctl firecracker volume export``, see
:ref:`firecracker-volumes`, is of no use if the host firewall blocks
the guest from reaching the NFS server. If ``firewalld`` is in use on
the host, the mount inside of the VM fails and the application starts
without its volumes. The services required for NFS have to be allowed
explicitly:

.. code-block:: bash

   firewall-cmd --permanent --add-service=nfs
   firewall-cmd --permanent --add-service=mountd
   firewall-cmd --permanent --add-service=rpc-bind
   firewall-cmd --reload

The commands require root privileges and have to be run once per
host, the ``--permanent`` option keeps the setting across a reboot of
the host and a restart of ``firewalld``.

.. note::

   As with the network setup, see :ref:`firecracker-networking`, this
   serves as an example for one firewall implementation. Please check
   which tool is managing the firewall on your host and refer to its
   documentation on how to allow the NFS traffic of the private
   firecracker network.

Feedback
========

Feedback is very much welcome. Please report issues and improvements
at:

https://github.com/OSInside/flake-pilot/issues
