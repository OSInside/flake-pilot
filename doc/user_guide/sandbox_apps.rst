.. _sandbox-apps:

===============================
Applications From a Sandbox
===============================

.. hint:: **Abstract**

   This chapter shows how to register applications which are provided
   by a directory tree on the host and run in a bubblewrap sandbox.
   All examples register the app for the calling user and expect the
   setup described in :ref:`getting-started`.

The ``bubblewrap`` engine runs the application in a new root system
created by the ``bwrap`` program. The root system is a plain directory
tree on the host, so the application sees the libraries and the
tooling of that tree instead of the ones installed on the host. The
tree is mounted as the read only layer of an overlay, thus the
application can write anywhere in its root filesystem without the tree
on the host being modified. The data written this way is kept in
memory, separately for every sandbox, and is gone when the application
terminates, see :ref:`sandbox-written-data`.

Unlike the other engines this one has no image registry. There is
nothing to pull and nothing to delete, the registration refers to the
tree by its path. Providing the tree is up to you.

Providing a Root Filesystem Tree
================================

Any directory which carries a usable root filesystem will do, e.g an
existing chroot environment or the unpacked contents of a container
image:

.. code-block:: bash

   mkdir -p ~/.local/share/rootfs/leap

   podman create --name leap-export registry.opensuse.org/opensuse/leap:15.6
   podman export leap-export | tar -x -C ~/.local/share/rootfs/leap
   podman rm leap-export

The tree is owned by you and is used as it is. It does not have to
exist at registration time, a registration against a path which is not
there yet is written with a warning and the application fails to run
until the tree is created.

The Host Root as the Tree
-------------------------

The root filesystem on the host, ``/``, is a directory tree like any
other and can be used as well. The application then runs with the
programs and the libraries it would find anyway, but none of its
changes reach them:

.. code-block:: bash

   flake-ctl bubblewrap register --rootfs / \
       --app $HOME/bin/protected-shell --target /bin/bash

   protected-shell

This registers an app named ``protected-shell`` which drops you into
the bash shell on the host itself. Everything looks familiar, ``ls``,
the home directory and the installed tooling are all there, and a
write anywhere in the tree succeeds but lands in the overlay of the
sandbox. Installing a package or deleting a file leaves the host as
it is. The change only lives in the overlay of this call, the next
call of ``protected-shell`` starts from the unmodified host again,
see :ref:`sandbox-written-data`.

Unlike a tree of its own, the host root has further mount points
below it, e.g ``/proc`` or the subvolumes of a ``btrfs`` root
filesystem. Such a tree cannot be provided by the launcher in a user
namespace, its overlay is mounted by the Flake Pilot helper instead.
``protected-shell`` therefore needs a ``sudo`` rule for the helper,
and a caller which is not ``root`` is asked for its ``sudo``
password, see :ref:`sandbox-written-data` and :ref:`privileges`.

This is useful to try out a command which is supposed to modify the
system, or to hand a directory to a program which should read it but
not touch anything else. The paths whose changes should be kept are
added explicitly:

.. code-block:: bash

   flake-ctl bubblewrap register --rootfs / \
       --app $HOME/bin/protected-shell --target /bin/bash \
       --opt "\--bind %HOME/work %HOME/work"

.. note::

   Keeping the changes on the host root in an overlay protects the
   files on the host, it does not make the call a security boundary.
   A path which is explicitly bound writable is modified for real.
   The sandbox shares
   the kernel and, unless further ``bwrap`` options restrict it, the
   network and the other namespaces of the caller. For a stricter
   separation use a tree of its own, or the ``firecracker`` engine,
   see :ref:`vm-apps`.

.. _sandbox-example-leapshell:

A Shell as a Sandbox App
========================

.. code-block:: bash

   flake-ctl bubblewrap register --rootfs ~/.local/share/rootfs/leap \
       --app $HOME/bin/leapshell --target /bin/bash

   leapshell

This registers an app named ``leapshell`` to the system. Once called,
``bwrap`` creates a sandbox whose root filesystem is the ``leap`` tree
and drops you into the bash shell of that tree. ``bwrap`` itself
creates the sandbox through user namespaces and needs no privileges.
The overlay of the tree is mounted prior to the call, in a user and
mount namespace the launcher creates for the sandbox, and needs no
privileges either. As long as the tree has no mount points below it,
no ``sudo`` rule is involved, see :ref:`privileges`.

Nothing on the host is visible inside except for the pseudo
filesystems, which is the point of the separate root. The paths the
application actually needs are handed to it explicitly.

Adding Paths from the Host
==========================

``--opt`` passes an option to ``bwrap``, most of the time a mount
specification. An option which starts with a dash has to be escaped
with a backslash so that it is not read as an option of the register
command itself:

.. code-block:: bash

   flake-ctl bubblewrap register --rootfs ~/.local/share/rootfs/leap \
       --app $HOME/bin/mybuild --target /usr/bin/make \
       --opt "\--ro-bind /etc/resolv.conf /etc/resolv.conf" \
       --opt "\--bind %HOME/work /work" \
       --pilot-option "%chdir:/work"

   mybuild

This registers an app named ``mybuild`` which calls ``make`` inside of
the sandbox. The name resolution on the host is shared read only, the
``work`` directory of the calling user is shared writable as ``/work``
and the program is called in that directory.

A value starting with ``%`` is replaced by the environment variable of
the same name at call time, ``%HOME`` above. A placeholder which does
not resolve stays as the plain variable name.

The options given this way are **added** to the standard options of
the sandbox:

.. code-block:: bash

   --dev /dev --proc /proc --tmpfs /tmp --unshare-pid --unshare-ipc
   --unshare-uts --new-session --die-with-parent --clearenv

They provide the pseudo filesystems, a writable ``/tmp``, private
process ID, IPC and host name namespaces and the termination of the
sandbox together with the pilot. The application runs in a session of
its own and can therefore not inject input into your terminal. As a
consequence an interactive shell in the sandbox has no job control.
The environment of the application is cleared, only ``PATH``,
``HOME``, ``USER``, ``LOGNAME``, ``TERM``, ``LANG``, ``LC_ALL`` and
``TZ`` are set again with your values. A variable you set with a
``--setenv`` option of your own keeps that value, any other variable
the application needs has to be given that way. All of the options
are written to the flake configuration and can be changed there like
any other setting, see :ref:`application-setup`.

The option which mounts the root filesystem of the sandbox is not
part of that list. It is always added by the launcher, ahead of every
other option, because it refers to the overlay it creates for the
sandbox. The root of the sandbox is available to the other options
as ``%OVERLAYROOT``.

An ``--overlay-src`` option of your own adds a directory to that
root instead of mounting it at a path of its own. Such an option is
not passed to ``bwrap``, the directory becomes a layer of the overlay
and is stacked on top of the rootfs:

.. code-block:: bash

   flake-ctl bubblewrap register --rootfs ~/.local/share/rootfs/leap \
       --app $HOME/bin/leapshell --target /bin/bash \
       --opt "\--overlay-src $HOME/leap-extra"

Everything below ``leap-extra`` then shows up in the sandbox as if it
were part of the tree, files of the same path win over the ones of
the rootfs. Like the rootfs the directory is a read only layer, it is
not modified by the application.

.. note::

   This differs from the other engines, where ``--opt`` replaces the
   defaults of the template. Sandbox options are mostly mounts which
   add up to a working setup rather than alternatives to it.

.. _sandbox-written-data:

Where Written Data Is Kept
==========================

Everything the application writes to its root filesystem ends up in a
tmpfs, nothing of it is written to disk. The tmpfs and the overlay are
not mounted on the host. Prior to the call of ``bwrap`` the launcher
moves into a user and mount namespace of its own, which maps only
your user and group, and mounts them there. ``bwrap`` creates the
sandbox in that namespace. The tmpfs is mounted on a directory below
``/var/tmp`` which is named after the ID of the calling user, e.g
``/var/tmp/bwrap_1000``. Only that user has access to the directory,
and on the host it stays empty. Nobody on the host can see or access
the mounts, and every sandbox has a namespace of its own, two
sandboxes of the same user do not share them although they are
mounted on the same directory.

When the application terminates, the mounts are gone along with
everything written to them. The next call starts from the unmodified
tree again, whether it is called with an ``@NAME`` selector or not.

There are two exceptions. Data written below ``/tmp`` with the
default options is kept in a tmpfs created by ``bwrap`` and is gone
when the application terminates as well. Data written to a path
which is bound writable from the host changes that path on the host
and is kept.

The namespace needs a host which allows unprivileged user namespaces,
which ``bwrap`` without the setuid bit needs anyway, and a kernel
which allows overlay mounts in a user namespace, Linux 5.11 or newer.

A tree, or a directory added with ``--overlay-src``, which has mount
points below it cannot be used this way, e.g the host root. The
mounts of the host are locked in a user namespace, and the kernel
refuses an overlay which would reveal what they cover. The launcher
checks for such mount points, a mount on the directory itself does
not count. If it finds one, it calls the ``sandbox-run`` command of
the Flake Pilot helper through ``sudo`` instead:

* The helper moves into a mount namespace of its own whose mounts
  are private, nothing is mounted on the host in this case either.

* It opens the tree and the layers with your identity, a tree you
  cannot reach yourself is refused, and mounts the same setup as
  described above on your directory below ``/var/tmp``.

* It then drops its privileges to yours for good, makes sure that no
  program in the namespace can gain privileges, e.g through a setuid
  program, and replaces itself by ``bwrap``.

``bwrap`` is started with a fresh environment which only contains
``PATH``, your ``HOME``, ``USER`` and ``LOGNAME`` and ``TERM``. With
the default ``--clearenv`` option the application gets the same
variables as in any other sandbox, see the standard options above.
Without it the application only sees that fresh environment. A
sandbox created this way is shown by ``flake-ctl bubblewrap show``
with the process of ``sudo``. It needs a ``sudo`` rule for the
helper, see :ref:`privileges`.

The administrator sets the size of the tmpfs with the
``sandbox_tmpfs_size`` setting in ``/etc/flakes.yml``, e.g:

.. code-block:: yaml

   generic:
     sandbox_tmpfs_size: 2G

Without the setting the kernel default applies, half of the memory
of the host. Every tmpfs gets the configured size unless the flake
asks for a smaller one with the ``%tmpfs_size`` pilot option. A
number with a ``k``, ``m``, ``g`` or ``t`` suffix or a percentage of
the memory of the host using the ``%`` suffix is accepted, a size
above the limit is refused:

.. code-block:: bash

   leapshell %tmpfs_size:512M

For a sandbox the launcher mounts itself the limit is applied by the
launcher. As you create the tmpfs in a namespace of your own, it is a
default of the system there rather than a privilege boundary. The
memory of the tmpfs is accounted to your processes and is released
when the sandbox terminates. For a sandbox mounted by the helper the
limit is enforced by the helper.

Former versions kept the written data across calls, on disk in the
directories ``leapshell_rw`` and ``leapshell_work`` or in tmpfs
mounts on the host, ``leapshell_overlay`` and ``leapshell_merged``,
below ``/var/tmp/bwrap_1000``. They are not used anymore. A tmpfs
mount left on the host by a former version stays until the host is
rebooted or ``root`` removes it, the remaining directories can be
deleted by you. The ``%discard`` pilot option of former versions is
gone as well. A pilot option which is unknown is ignored, a flake
registered with it keeps working.

Registration Options in Short
=============================

``--rootfs``
   The absolute path of the directory tree on the host which becomes
   the root filesystem of the sandbox.

``--app`` and ``--target``
   The path of the application on the host and the program to call
   inside of the sandbox, like for container flakes.

``--opt``
   An option of ``bwrap`` and its values, added to the standard
   options listed above. Can be given more than once. See
   ``man 1 bwrap`` for what is available.

``--pilot-option``
   A runtime option of the pilot, e.g ``%chdir:/work``. The option is
   stored in the ``pilot_options`` list of the flake configuration and
   is effective on every call. The same option given at call time
   takes precedence. The option can be specified multiple times.

See ``man 8 flake-ctl-bubblewrap-register`` for the complete list.

Calling a Registered Sandbox App
================================

Arguments given to the app are passed on to the program inside of the
sandbox, with two exceptions which are read by the launcher itself:

``@NAME``
   A selector which allows to distribute the exact same program call
   to different instances, e.g ``leapshell @one``. Each instance gets
   a sandbox of its own. Calling the same instance twice at the same
   time is refused.

``%OPTION``
   A runtime option of the pilot, e.g ``%chdir:/work`` to call the
   program in a directory of the sandbox instead of its root
   directory. See ``man 8 bubblewrap-pilot`` for the complete list.

A sandbox exists as long as the application running in it. The
instances of the current setup are listed via:

.. code-block:: bash

   flake-ctl bubblewrap show

Removing a registration deletes the symlink and the flake
configuration. The root filesystem tree belongs to you and is never
touched by it:

.. code-block:: bash

   flake-ctl bubblewrap remove --app $HOME/bin/leapshell
