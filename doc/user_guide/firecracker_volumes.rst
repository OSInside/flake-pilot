.. _firecracker-volumes:

====================
Firecracker Volumes
====================

.. hint:: **Abstract**

   This chapter explains how a local host path is made available
   inside of a Firecracker VM application, which commands create and
   delete that setup and how the result looks in the flake
   configuration.

A Firecracker VM has no way to share a directory with the host
directly, unlike a container it does not use the host kernel and
therefore cannot bind mount a host path into the guest. ``flake-pilot``
bridges this gap with NFS: a local path is exported by the host and
mounted by the guest over the private network described in
:ref:`firecracker-networking`. The setup is created and deleted with
the ``flake-ctl firecracker volume`` commands, no manual NFS
configuration is needed.

The setup works within the following requirements:

* The application has to be connected to the host network with
  ``flake-ctl firecracker network add``, see
  :ref:`firecracker-networking`. The volume setup reads the gateway
  address from that network setup.

* The guest needs a working network setup at boot and has to provide
  the NFS client tools, in particular the ``mount.nfs`` helper
  program.

Managing volumes is a two step process: a local path is exported
through NFS once, independently of any application. It is then added
to one or more flake instances which mount it at a path of their
choice.

Export a Local Path
====================

.. code-block:: bash

   flake-ctl firecracker volume export --path /some/local/path

Writes a ``flake-pilot`` managed entry for the given path to
``/etc/exports``, restricted to the private firecracker network
``172.16.0.0/24``. If the ``nfs-server`` systemd service is not
running yet it is started, otherwise the export table is reloaded so
the new export becomes effective right away.

The command expects an existing directory. As it changes a system
wide NFS configuration, the required privileged operations are
executed through ``sudo`` when needed. Exporting the same path again
keeps the existing entry unchanged.

Release a Local Path
=====================

.. code-block:: bash

   flake-ctl firecracker volume release --path /some/local/path

Removes the ``flake-pilot`` managed NFS export entry for the given
path from ``/etc/exports`` and restarts the ``nfs-server`` service so
the updated export table becomes effective. Only entries previously
managed by ``flake-pilot`` are touched, an export created by other
means is left in place.

.. note::

   Releasing a path does not check whether it is still in use by a
   flake. Remove it from every flake it was added to first, see
   below, before releasing the export.

Add a Volume to an Application
===============================

.. code-block:: bash

   flake-ctl firecracker volume add \
       --app $HOME/bin/claude --volume /some/local/path:/some/guest/path

Looks up the flake configuration of the application from its path and
writes an ``nfs=`` boot option to it in the form:

.. code-block:: text

   nfs=172.16.0.1:/some/local/path:/some/guest/path

The server part of the option is not configurable, it is the gateway
address of the private network the application is connected to,
``172.16.0.1``, read from the ``rd.route=`` boot option written by
``flake-ctl firecracker network add``. This is why the network setup
has to exist before a volume can be added.

The ``--volume`` option can be given more than once to add several
volumes in one call:

.. code-block:: bash

   flake-ctl firecracker volume add --app $HOME/bin/claude \
       --volume /some/local/path:/some/guest/path \
       --volume /other/local/path:/other/guest/path

As every instance needs its own volume setup if it should differ from
the application, the command can be called with ``--instance`` as
well:

.. code-block:: bash

   flake-ctl firecracker volume add --app $HOME/bin/claude \
       --volume /some/local/path:/some/guest/path --instance @id1

The volume(s) of an instance take the place of the volumes configured
for the application itself, they are not merged. Calling the
application without the ``@id1`` selector still mounts the volumes
configured globally, if any.

A volume is identified by its guest path: adding a volume with a
guest path that is already configured replaces the entry, e.g to
point it at a different local path or, after the path was exported
again, a different server.

.. note::

   The command only changes the flake configuration of the
   application, no privileged operations are required. The local path
   itself still has to be exported, see above, or the mount inside of
   the guest fails.

Remove a Volume From an Application
=====================================

.. code-block:: bash

   flake-ctl firecracker volume remove \
       --app $HOME/bin/claude --volume /some/local/path:/some/guest/path

Reverts what ``volume add`` has performed: the matching entry is
deleted from the ``nfs=`` boot option, and the option itself is
deleted once no volume is left. ``--volume`` can be repeated the same
way as with ``add``, and ``--instance`` removes the volume(s) of that
instance only. A volume is matched by its host and guest path, no
matter which server it is currently provided from.

Removing a volume does not release its NFS export nor does it change
the network setup, both may still be needed by other flakes or
instances. Release the export explicitly once it is no longer used by
any application.

The Result in the Flake Configuration
=======================================

The flake configuration for the registered ``claude`` app from
:ref:`vm-example-claude` can be found at:

.. code-block:: bash

   vi ~/.config/flakes/claude.yaml

Adding a volume to the application and another one to its ``@id1``
instance leads to the following settings, on top of the network setup
from :ref:`firecracker-networking`:

.. code-block:: yaml

   vm:
     runtime:
       firecracker:
         boot_args:
           - ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off
           - rd.route=172.16.0.1/24::eth0
           - nameserver=8.8.8.8
           - nfs=172.16.0.1:/some/local/path:/some/guest/path
         instance:
           "@id1":
             boot_args:
               - ip=172.16.0.3::172.16.0.1:255.255.255.0::eth0:off
               - nfs=172.16.0.1:/other/local/path:/other/guest/path

More than one volume of the same section is folded into a single
``nfs=`` option as a comma separated list, which is the format the
init process of the guest, ``sci``, reads the volumes from. It mounts
each of them with ``mount -t nfs`` before the application is started,
creating the guest path if it does not exist yet. A volume which
cannot be parsed, or which fails to mount, is skipped, the remaining
volumes are still mounted. For details refer to ``man 8 sci``.

.. note::

   As with the network ``instance`` section, the ``@`` character is
   reserved in YAML and the key has to be quoted. The plain name
   without the ``@`` prefix, e.g ``id1``, is accepted as a key as
   well.
