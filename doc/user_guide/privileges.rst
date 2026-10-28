.. _privileges:

====================
Privileges and Trust
====================

.. hint:: **Abstract**

   This chapter explains which features of Flake Pilot need root
   privileges and how they get them: through one program, the
   Flake Pilot helper, which is called through ``sudo``. It describes
   what the helper checks, what a ``sudo`` rule for it grants and
   introduces the reference ``sudoers`` policy which comes with the
   ``flake-pilot`` package.

.. note::

   **A sudo rule for the helper is not a root shell, but it is
   trust.**

   The helper provides a fixed set of commands and checks every
   argument and the authority of the calling user. A user with a
   ``sudo`` rule for it cannot run arbitrary commands as root. The
   rules do let the user change the network setup of the host for
   the firecracker VMs, and run every podman flake registered system
   wide, as root, with what its registration grants on the host.

How Flake Pilot Gets Its Privileges
===================================

Flake Pilot does not install any setuid binary. Every action which
needs more privileges than the calling user has is performed by the
helper:

.. code-block:: none

   /usr/libexec/flake-pilot/flake-pilot-helper

``flake-ctl``, ``podman-pilot`` and ``bubblewrap-pilot`` call it
through ``sudo``. It is the only program Flake Pilot calls through
``sudo``. A caller who is ``root`` already runs it directly. Every
other program, ``podman`` included, is called with the privileges of
the process that calls it.

The helper belongs to root and refuses to run as a setuid program,
it has to be called through ``sudo`` or by ``root``. It takes the
calling user from ``sudo`` and clears its environment before it does
anything else. Every command checks its arguments against a strict
pattern before they become part of a path, a device name or a
program argument, and it checks the authority of the caller against
data which belongs to root. ``man 8 flake-pilot-helper`` lists the
commands.

What the Helper Checks
======================

The network setup of the firecracker VMs
  The private network of the VMs has to be one of the networks Flake
  Pilot selects from and must not collide with a network of the host.
  The outgoing interface has to exist. Both are recorded in the
  network state, ``/var/lib/firecracker/network-state.yaml``, which
  belongs to root. The address of a VM, the route to it and its
  firewall rules are only ever created within the recorded network.
  The TAP device of a VM is created for the caller, only root can
  create it for another user. The flake configuration which
  identifies the VM has to belong to the caller or to the system wide
  registry. See :ref:`firecracker-networking`.

NFS exports
  A directory can only be exported by its owner, or by root for a
  user other than root. It is exported to exactly the VM the network
  state records for the flake instance. Every access of the VM is
  mapped to the owner of the directory. Only that owner and root can
  release the export. See :ref:`firecracker-volumes`.

The root filesystem of a bubblewrap sandbox
  Only used for a tree of the sandbox, or a layer a flake stacks on
  top of it, with mount points below it, e.g the root of the host.
  The helper moves into a new mount namespace whose mounts are
  private, nothing is mounted on the host. The directory of the
  caller below ``/var/tmp`` is opened without following a symbolic
  link, it has to belong to the caller and must not be writable for
  others. The tree and the layers are opened with the identity of
  the caller, a tree the caller cannot reach is refused. They are
  the read only layers of the overlay, the writable layer is a tmpfs
  of the caller which is gone along with the sandbox. The tmpfs and
  the overlay are mounted nosuid and nodev. The size of the tmpfs is
  limited by ``sandbox_tmpfs_size`` in ``/etc/flakes.yml``, a caller
  can only ask for less. The helper then drops its privileges to the
  ones of the caller for good, sets ``no_new_privs``, which keeps
  every program in the namespace from gaining privileges, e.g
  through a setuid program, and replaces itself by
  ``bubblewrap-pilot``, which takes the ``bwrap`` options from the
  registration and replaces itself by ``bwrap``. The tree, the
  layers and the options are taken from the registration of the
  flake. The registration of a flake registered system wide has to
  belong to root, the registration of a flake of the caller is read
  with the identity of the caller and provides nothing the caller
  could not choose anyway. The arguments of the caller are only
  passed to the application, which runs with the privileges of the
  caller only. See :ref:`sandbox-apps`.

System wide podman flakes
  The application has to be a link to ``podman-pilot`` which belongs
  to root, and its registration has to belong to root. The helper
  starts ``podman-pilot`` as root for the caller with a fresh
  environment. The container and its runtime options are taken from
  the registration. A ``%VAR`` reference in the runtime options, e.g
  ``%HOME``, resolves to the value of the user database for the
  caller, not to a value the caller set. The meta data of the
  instances is kept in a directory which belongs to root. See
  :ref:`container-apps`.

What Each Feature Needs
=======================

.. list-table::
   :header-rows: 1
   :widths: 35 35 30

   * - Feature
     - Privileges
     - Grants
   * - podman flake from the user registry, ``podman-pilot`` and
       ``flake-ctl podman``
     - none, rootless podman of the calling user
     - nothing
   * - podman flake from the system registry, ``podman-pilot``
     - helper, ``podman-pilot`` command
     - what the registration grants, see below
   * - ``flake-ctl podman`` for the system registry
     - must be called by root
     - n/a
   * - firecracker flake, ``firecracker-pilot``, from either registry
     - none
     - nothing
   * - ``flake-ctl firecracker pull``
     - none for the user registry, root for the system registry
     - nothing
   * - ``flake-ctl firecracker network init``, ``cleanup``
     - helper, ``network-init`` and ``network-cleanup`` commands
     - the network setup of the host
   * - ``flake-ctl firecracker network add``, ``remove``
     - helper, ``network-connect`` and ``network-disconnect`` commands
     - the VMs of the caller
   * - ``flake-ctl firecracker volume export``, ``release``
     - helper, ``volume-export`` and ``volume-release`` commands
     - the directories of the caller
   * - bubblewrap flake, ``bubblewrap-pilot``, from either registry
     - none
     - nothing
   * - bubblewrap flake whose tree or layer has mount points below
       it, e.g ``--rootfs /``
     - helper, ``bubblewrap-pilot`` command
     - the sandboxes of the caller
   * - ``flake-ctl init`` on a host without the meta data directory
     - helper, ``meta-dirs-init`` command
     - nothing

``flake-ctl firecracker volume add`` and ``remove`` only change the
flake configuration and need no privileges.

What This Means in Practice
===========================

User podman, firecracker and most bubblewrap flakes need no rule.
  Podman flakes from the user registry run in the rootless podman
  setup of the user, firecracker flakes run the VM as the calling
  user. Bubblewrap flakes mount the root filesystem of the sandbox
  in a user and mount namespace of the calling user. None of them
  calls the helper. The exception is a bubblewrap flake whose tree
  or one of its layers has mount points below it, e.g the root of
  the host. A user namespace cannot provide such a tree, the helper
  mounts it, see above.

A system wide podman flake is granted to every user with the rule.
  The flake runs as root in the podman setup of root, with the
  container and the runtime options of its registration. Whatever
  the flake can do on the host, e.g through the host directories its
  registration mounts into the container, every user with the rule
  can do through it. Register a podman flake system wide only if all
  of these users may use it this way.

The content of a system wide flake is trusted as well.
  A delta container may ship a ``/systemfiles`` script which collects
  the files to provision from the host. ``podman-pilot`` runs it on
  the host in a bash process of the user who calls the flake, with
  the privileges of that user only, the files it lists are copied
  into the container as root. The script runs with the privileges of
  every user who calls the flake and decides which host files end up
  in a container of root. Register only containers and images whose
  authors you trust with that.

The network setup is host wide.
  A user with the rule for it picks the private network of the VMs,
  the interface their traffic leaves the host through and the
  services of the host they may reach. Deleting the setup disconnects
  the VMs of all users. Consider creating the host setup as root and
  granting only the rules for the per flake commands.

The metadata directory is provided by root, not by the pilots.
  All pilots keep the state of their instances in a private
  directory of the calling user below a metadata directory shared
  by all users, ``/tmp/flakes`` by default. The owner of that
  directory can delete and replace the private directories of all
  other users. It therefore has to belong to root and to have the
  permissions ``1777``, like ``/tmp``. No pilot creates it. A pilot
  refuses to run if it does not exist or is set up differently, and
  tells how to fix it. The ``flake-pilot`` package lets
  ``systemd-tmpfiles`` create it at boot and when the package is
  installed, see ``/usr/lib/tmpfiles.d/flake-pilot.conf``. On a host
  without ``systemd``, or with a metadata directory other than
  ``/tmp/flakes`` configured in ``/etc/flakes.yml``, call
  ``flake-ctl init`` once after every boot instead. It creates the
  directory through the helper.

A user can block the flakes of another user.
  The private directory of a user is named after the ID of that
  user, e.g ``/tmp/flakes/1000``. Another user can create it first,
  or a symbolic link or file in its place. The pilots of the user it
  is named after then refuse to run and name the user who owns the
  entry along with the call to remove it, which only root can do.
  ``flake-ctl init`` called as root reports all such entries.

A password limits who uses the rule.
  ``sudo`` asks for the password of the calling user and caches it
  for a few minutes. ``NOPASSWD`` gives the user no further
  privilege, but it allows every process running under their
  account, e.g a malicious build script, to use the helper without
  any interaction. The reference policy does not use it.

``sudo`` logs every call.
  The calls of the helper show up in the system journal, e.g with
  ``journalctl _COMM=sudo``. The helper logs the user it acts for
  with every change of the host network setup.

.. _sudoers-reference:

The Reference Policy
====================

The ``flake-pilot`` package ships a reference policy as
documentation, e.g in
``/usr/share/doc/packages/flake-pilot/flake-pilot.sudoers``. In the
source tree it is located in ``doc/sudoers/flake-pilot.sudoers``.

The policy grants the members of the ``flake-pilot`` group the use
of the helper, one rule per feature, and states what each rule
grants. The first argument of the helper is the command to perform,
each rule is limited to the commands of its feature.

``sudo`` ignores files in ``/etc/sudoers.d`` whose name contains a
dot. Copied there under its original name, the policy therefore
stays inactive. To activate it:

1. Edit a copy of the file and delete the rules of the features you
   do not want to grant.

2. Create the group:

   .. code-block:: bash

      groupadd --system flake-pilot

3. Check the edited copy and install it without a dot in its name:

   .. code-block:: bash

      visudo --check --file ./flake-pilot.sudoers
      install -m 0440 -o root -g root ./flake-pilot.sudoers \
          /etc/sudoers.d/flake-pilot

4. Add the users to the group who may use the features listed
   above:

   .. code-block:: bash

      usermod --append --groups flake-pilot USER

The complete policy:

.. literalinclude:: ../sudoers/flake-pilot.sudoers
   :language: none
