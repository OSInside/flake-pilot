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
memory and is gone when the application terminates.

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
it is and is gone when the shell ends.

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
creates the sandbox through user namespaces and needs no privileges,
the overlay of the tree is mounted on the host prior to the call and
does need them. A caller which is not ``root`` is asked for its
``sudo`` password at that point.

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

   --dev /dev --proc /proc --tmpfs /tmp --unshare-pid --die-with-parent

They provide the pseudo filesystems, a writable ``/tmp``, a private
process ID namespace and the termination of the sandbox together with
the pilot. All of them are written to the flake configuration and can
be changed there like any other setting, see :ref:`application-setup`.

The options which mount the root filesystem of the sandbox are not
part of that list. They are always added by the launcher, ahead of
every other option, because they refer to the overlay it creates for
the called instance. The root of the sandbox as it exists on the host
is available to the other options as ``%OVERLAYROOT``.

An ``--overlay-src`` option of your own adds a directory to that
root instead of mounting it at a path of its own. Such an option is
moved in front of the mount of the root, where ``bwrap`` expects the
sources of an overlay, and is stacked on top of the rootfs:

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

``--run-as``
   Create the sandbox as another user, through ``sudo``. Only needed
   if the application itself requires it, ``bwrap`` does not.

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
