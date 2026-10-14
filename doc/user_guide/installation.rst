.. _installation:

============
Installation
============

.. hint:: **Abstract**

   This chapter describes how to install Flake Pilot from packages
   and how to build it from its sources.

Installing the Packages
=======================

Flake Pilot components are written in Rust and are available as
packages here:
https://build.opensuse.org/package/show/Virtualization:Appliances:Builder/flake-pilot

Install the following packages:

* ``flake-pilot``

  The registration tool ``flake-ctl``, the manual pages and the
  configuration templates. This package is always required.

* ``flake-pilot-podman``

  The ``podman-pilot`` launcher and the ``flake-ctl podman``
  subcommands.

* ``flake-pilot-firecracker``

  The ``firecracker-pilot`` launcher, the ``sci`` guest init and the
  ``flake-ctl firecracker`` subcommands.

* ``flake-pilot-bubblewrap``

  The ``bubblewrap-pilot`` launcher and the
  ``flake-ctl bubblewrap`` subcommands.

The engine itself is not part of these packages. Install ``podman``,
``firecracker`` and/or ``bubblewrap`` in addition, depending on which
of them you want to use.

Building From Source
====================

Manual compilation and installation can be done as follows:

.. code-block:: bash

   make build && make install

This compiles the workspace with ``cargo build --release``, renders
the manual pages with ``rst2man`` and installs the binaries, the
configuration and templates below ``/etc``, the bash completion, the
manual pages, the ``systemd-tmpfiles`` configuration, the dracut
network startup module for firecracker guests and the state
directories below ``/var/lib``.

The build requires:

* A Rust toolchain with ``cargo``
* ``python3-docutils`` for the manual pages
* The development files of OpenSSL, used by the download code of
  ``flake-ctl``

.. note::

   ``make install`` honors ``DESTDIR`` and ``PREFIX``, which allows
   to install into a staging area, e.g
   ``make install DESTDIR=/tmp/root``.

The following ``make`` variables control optional parts of the build:

* ``SCI_STATIC=yes``

  Passed to ``make build``, additionally builds a statically linked
  ``sci``, which ``make install`` then prefers over the dynamically
  linked one. This requires the static ``glibc`` library.

* ``TMPFILES_STATEDIRS=yes``

  Passed to ``make install``, installs ``systemd-tmpfiles``
  configurations which create the podman and firecracker state
  directories below ``/var/lib`` at boot instead of installing the
  directories.

Privileges
==========

Flake Pilot does not install any setuid binary. Actions which need
more privileges than the calling user has, like mounting the root
filesystem of a sandbox or creating a network device, are performed
by the Flake Pilot helper, which is called through the ``sudo``
binary of the system. The packages do not install a ``sudo`` policy.

.. note::

   A ``sudo`` rule for the helper does not let a user run arbitrary
   commands as root, but it lets the user run every podman flake
   registered system wide as root. Read :ref:`privileges` before you
   grant it, and start from the reference policy described there.

Next Steps
==========

Continue with :ref:`getting-started` to prepare the registry of your
user and to register a first application.
