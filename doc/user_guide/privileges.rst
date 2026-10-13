.. _privileges:

====================
Privileges and Trust
====================

.. hint:: **Abstract**

   This chapter explains which features of Flake Pilot need root
   privileges, how they get them through ``sudo`` and why the
   ``sudo`` rights involved make a user equivalent to root. It
   introduces the reference ``sudoers`` policy which comes with the
   ``flake-pilot`` package.

.. warning::

   **The sudo rights Flake Pilot needs are root rights.**

   Many features of Flake Pilot run general purpose programs like
   ``podman``, ``mount``, ``rsync``, ``cp`` or ``iptables`` as root
   through ``sudo``. These rights cannot be limited to the calls
   Flake Pilot makes. A user who has them can become root on the
   host, with or without Flake Pilot.

   Grant them only to users you would also trust with the root
   account of the host.

How Flake Pilot Gets Its Privileges
===================================

Flake Pilot does not install any setuid binary. Whenever a pilot or
``flake-ctl`` needs more privileges than the calling user has, it
calls the program in question through ``sudo``:

* as ``root``, e.g to mount an image, to provision a container or to
  create a network device, or

* as the calling user, which is how every ``podman`` call for a
  flake of the user registry is made.

Every program is passed to ``sudo`` with its absolute path, e.g
``/usr/bin/mount`` or ``/usr/sbin/iptables``, and never looked up in
the ``PATH`` of the caller. A rule in ``/etc/sudoers`` has to name
the programs the same way.

A user who calls Flake Pilot as ``root`` does not need ``sudo``.

Flake Pilot does not install a ``sudo`` policy. What the users of a
host are allowed to do is the decision of its administrator, and
this chapter provides the information to make it.

Why the Rules Cannot Be Narrowed
================================

``sudoers`` can restrict the arguments a program is called with. This
does not help here, for two reasons:

* The arguments differ on every call. They contain temporary
  directories, image files, container names, instance names and
  network addresses. A rule which matches all calls Flake Pilot can
  make has to match nearly every call.

* The calls stay powerful even with their arguments fixed in
  shape. Flake Pilot mounts an image file given by the flake
  configuration. A rule which allows ``mount IMAGE DIRECTORY`` also
  allows a user to mount an image of their own which contains a
  setuid root shell.

A ``sudo`` rule for one of these programs is therefore a rule to run
it with any arguments. The table below lists what that means for
each of them.

What Each Program Grants
========================

.. list-table::
   :header-rows: 1
   :widths: 20 80

   * - Program
     - What a user can do with it as root
   * - ``podman``
     - Start a container with ``--privileged`` and the host root
       mounted, which is a root shell on the host.
   * - ``mount``, ``umount``
     - Mount an image which contains setuid binaries, bind mount a
       file of the user over ``/etc/passwd`` or ``/etc/sudoers``,
       remove any mount of the host.
   * - ``rsync``
     - Copy any file over any other file, run commands through
       ``--rsh``.
   * - ``cp``, ``mv``, ``install``
     - Overwrite any file, e.g ``/etc/shadow``, ``/etc/sudoers`` or a
       program root runs.
   * - ``chmod``
     - Set the setuid bit on a shell, make any file writable.
   * - ``mkdir``
     - Create directories with any permissions in any location. Not
       a root shell on its own, but not limited to the directories
       of Flake Pilot either.
   * - ``systemctl``
     - Stop, start or mask any service, start a unit file written by
       the user, open an editor as root through ``systemctl edit``.
   * - ``iptables``
     - Change or flush any firewall rule, run a program through
       ``--modprobe``.
   * - ``ip``
     - Reconfigure any network interface and route, run a program
       through ``ip netns exec``.
   * - ``sysctl``
     - Change any kernel parameter, e.g ``kernel.core_pattern`` which
       names a program the kernel runs as root.
   * - ``exportfs``
     - Export any directory, ``/`` included, to any host with
       ``no_root_squash``.
   * - ``test``
     - Find out if a file exists, also in directories the user cannot
       read.
   * - ``true``
     - Nothing. Flake Pilot calls it to let ``sudo`` ask for the
       password before the actual work starts.

What Each Feature Needs
=======================

.. list-table::
   :header-rows: 1
   :widths: 30 40 15 15

   * - Feature
     - Programs
     - Runs as
     - Grants
   * - podman flake from the user registry, ``podman-pilot`` and
       ``flake-ctl podman``
     - ``podman``
     - calling user
     - nothing
   * - podman flake from the system registry, ``podman-pilot`` and
       ``flake-ctl podman``
     - ``podman``, ``true``, ``mkdir``, ``chmod``; for delta
       containers and layers also ``test``, ``cp``, ``rsync``, ``sh``
     - root
     - **root**
   * - firecracker flake, ``firecracker-pilot``, from either registry
     - none
     - calling user
     - nothing
   * - ``flake-ctl firecracker pull``
     - ``mount``, ``umount``, ``mkdir``, ``cp``, ``mv``
     - root
     - **root**
   * - ``flake-ctl firecracker network init``, ``add``, ``remove``
     - ``sysctl``, ``iptables``, ``ip``
     - root
     - **root**
   * - ``flake-ctl firecracker volume export``, ``release``
     - ``install``, ``systemctl``, ``exportfs``
     - root
     - **root**
   * - bubblewrap flake, ``bubblewrap-pilot``, from either registry
     - ``mount``, ``umount``
     - root
     - **root**

``flake-ctl firecracker volume add`` and ``remove`` only change the
flake configuration and need no privileges.

What This Means in Practice
===========================

Podman flakes from the user registry run without root.
  ``podman`` is still called through ``sudo``, as the calling user,
  and ``sudo`` requires a rule for that, see the reference policy.
  Delta containers and layers are not provisioned for these flakes,
  as provisioning requires root.

Firecracker flakes run without root.
  ``firecracker-pilot`` starts the VM as the calling user and calls
  nothing through ``sudo``. Root is only needed for the host setup
  of ``flake-ctl firecracker``, see below.

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
  ``flake-ctl init`` as root once after every boot instead.

A system wide podman flake is not usable by everybody.
  The registry in ``/usr/share/flakes`` makes a flake visible to all
  users, but running a podman flake from it requires root podman.
  Every user who runs it has to be trusted with root.

Every bubblewrap flake requires root.
  This applies to flakes in the registry of the user as well. Running
  them in a sandbox isolates the application, but the user who starts
  them needs root equivalent ``sudo`` rights.

Host setup is a task for the administrator.
  The network and NFS setup of ``flake-ctl firecracker`` changes the
  host, not a single flake. Instead of granting the rules for it,
  consider running these commands as root.

The content of a flake is trusted with root as well.
  A delta container may ship a ``/systemfiles`` script which
  ``podman-pilot`` runs as root to collect the files to provision
  from the host. Register only containers and images whose authors
  you trust with root.

A password does not limit the rights, but it limits who uses them.
  ``sudo`` asks for the password of the calling user and caches it
  for a few minutes. ``NOPASSWD`` gives the user no further
  privilege, but it allows every process running under their
  account, e.g a malicious build script, to become root without any
  interaction. The reference policy does not use it.

``sudo`` logs every call.
  The calls Flake Pilot makes, and every other use of the same
  rights, show up in the system journal, e.g with
  ``journalctl _COMM=sudo``.

.. _sudoers-reference:

The Reference Policy
====================

The ``flake-pilot`` package ships a reference policy as
documentation, e.g in
``/usr/share/doc/packages/flake-pilot/flake-pilot.sudoers``. In the
source tree it is located in ``doc/sudoers/flake-pilot.sudoers``.

The policy grants the rights described above to the members of the
``flake-pilot`` group, one rule per feature, and states the trust
each rule grants. It does not try to restrict arguments, for the
reasons explained above.

``sudo`` ignores files in ``/etc/sudoers.d`` whose name contains a
dot. Copied there under its original name, the policy therefore
stays inactive. To activate it:

1. Edit a copy of the file and delete the rules of the features you
   do not use.

2. Create the group:

   .. code-block:: bash

      groupadd --system flake-pilot

3. Check the edited copy and install it without a dot in its name:

   .. code-block:: bash

      visudo --check --file ./flake-pilot.sudoers
      install -m 0440 -o root -g root ./flake-pilot.sudoers \
          /etc/sudoers.d/flake-pilot

4. Add the users you trust with root to the group:

   .. code-block:: bash

      usermod --append --groups flake-pilot USER

5. For podman flakes from the user registry, add a rule per user
   which allows them to run ``podman`` as themselves, see the
   template in the policy. A rule for the whole group would allow
   each member to run ``podman`` as any other member.

The complete policy:

.. literalinclude:: ../sudoers/flake-pilot.sudoers
   :language: none
