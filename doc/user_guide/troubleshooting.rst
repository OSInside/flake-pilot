.. _troubleshooting:

=================================
Troubleshooting and Known Issues
=================================

.. hint:: **Abstract**

   This chapter collects the switches which show what a pilot is
   doing and the issues which are known to get in the way.

Debugging a Flake
=================

All pilots report the details of their work if the following
environment variable is set:

.. code-block:: bash

   PILOT_DEBUG=1 myapp

The output shows which flake configuration was read, which engine
command was constructed from it and, for VM applications, which
kernel commandline and which TAP device name the instance is started
with. This is the first thing to look at whenever an application does
not behave as expected. For a sandbox flake the constructed ``bwrap``
call also shows the complete list of mounts the application is given,
which is usually what a missing file inside the sandbox comes down
to.

The log output is written without color by default. Colors are
enabled with the following environment variable:

.. code-block:: bash

   FLAKE_LOG_STYLE=always myapp

Accepted values are ``never`` (default), ``always`` and ``auto``.
An unrecognized value falls back to ``auto``. The variable applies
to the pilots and to flake-ctl. flake-pilot-helper does not see the
environment of its caller and always writes its messages without
color.

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
a plain number if the owning host user does not exist there.

The export maps every user of the VM, including ``root`` which the
init process of the guest starts the application as, to the user and
group ID of the exported path on the host (``all_squash`` with
``anonuid`` and ``anongid``). The IDs are the ones of the path itself,
the group is therefore not necessarily the primary group of its
owner. Look them up on the host with:

.. code-block:: bash

   stat -c '%u %g' /some/local/path

The same numbers are listed as ``anonuid`` and ``anongid`` in the
entry of the path in ``/etc/exports``.

Every access of the VM is performed with the permissions of that
owner on the host, and every file the application creates belongs to
the owner and to the group of the path. Reading and writing the
volume therefore works for any user of the VM, as long as the owner
on the host is allowed to do it. The following still fails with a
permission error:

* Changing the ownership of a file to another user or group, e.g with
  ``chown`` or ``chgrp``, even when done as ``root`` inside of the VM.
  Programs which restore the ownership of the files they write when
  running as ``root`` report this as well, e.g ``tar`` on extraction,
  ``cp -a`` or ``rsync -a``. The files are written nevertheless, they
  belong to the owner of the volume. Options which do not restore the
  ownership avoid the messages, e.g ``tar --no-same-owner`` or
  ``rsync -rlt``.

* Accessing a file below the exported path which the owner on the
  host cannot access either, e.g a file of another host user which is
  not readable for the owner.

Programs which compare the owner of a file with the user they run as
can refuse to work with the volume, as the application runs as
``root`` inside of the VM but the files belong to the numeric ID of
the owner. ``git`` for example reports the repository as owned by
someone else (``detected dubious ownership``). For ``git`` the
location can be marked as safe in the image of the VM:

.. code-block:: bash

   git config --global --add safe.directory /some/guest/path

The general solution is to run the application inside of the VM as a
user with the same numeric IDs as the exported path on the host.
Create a matching group and user in the image of the VM, e.g for the
ID ``1000``:

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

The init process of the guest always starts the application as
``root``. To run it as the new user, add a small wrapper script to
the image, e.g ``/usr/local/bin/claude-as-myuser``:

.. code-block:: bash

   #!/bin/sh
   exec runuser -u myuser -- env HOME=/home/myuser /usr/bin/claude "$@"

and register the application with the script as its target, e.g
``--target /usr/local/bin/claude-as-myuser``.

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
