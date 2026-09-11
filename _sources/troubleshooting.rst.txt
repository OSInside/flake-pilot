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

User and Group ID of an NFS Volume
-----------------------------------

NFS transports the ownership of a file as numeric user and group ID,
it does not transport the names behind them. Host and guest each
resolve those numbers through their own user database, and the
minimal image of a Firecracker VM usually knows nothing about the
accounts of the host. A volume exported with
``flake-ctl firecracker volume export``, see
:ref:`firecracker-volumes`, therefore shows up inside the VM owned by
a plain number if the owning host user does not exist there, and
every access done under a different ID than the one the files belong
to is answered with a permission error.

The application itself is started as ``root`` by the init process of
the guest, and the export is written with the ``no_root_squash``
option, so ``root`` inside the VM is not mapped to ``nobody`` and can
read and write the volume regardless of its ownership. The mismatch
becomes visible as soon as the application runs under a user account
of its own inside the VM, or when the files it creates should stay
accessible to the owning user on the host, as they are created with
the ID the process runs under.

The fix is to give the VM a user with the same numeric IDs as the
owner of the exported path on the host. Look the IDs up on the host
first:

.. code-block:: bash

   stat -c '%u %g' /some/local/path

For a path owned by the calling user this is the same as:

.. code-block:: bash

   id -u; id -g

With the IDs known, create a matching group and user in the VM, e.g
for the ID ``1000``:

.. code-block:: bash

   groupadd -g 1000 myuser
   useradd -u 1000 -g 1000 -m myuser

The ``-u`` and ``-g`` options are what matters, they pin the account
to the IDs used on the host, the name is free to choose as it never
leaves the guest. If the group already exists under the wanted ID,
the ``groupadd`` call can be left out and ``useradd -g`` points to
the existing group. Adding the user to the image description of the
VM, see :ref:`building-images`, keeps the setup in place across a
rebuild of the image and applies to every application based on it.

.. note::

   Only the numbers have to match, not the user names. A host user
   ``tux`` with the ID ``1000`` and a guest user ``myuser`` with the
   ID ``1000`` are the same owner as far as NFS is concerned, whereas
   two accounts of the same name with different IDs are not.

Feedback
========

Feedback is very much welcome. Please report issues and improvements
at:

https://github.com/OSInside/flake-pilot/issues
