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

The engine itself is not part of these packages. Install ``podman``
and/or ``firecracker`` in addition, depending on which of them you
want to use.

Building From Source
====================

Manual compilation and installation can be done as follows:

.. code-block:: bash

   make build && make install

This compiles the workspace with ``cargo build --release``, renders
the manual pages with ``rst2man`` and installs the binaries, the
templates, the bash completion and the manual pages below ``/usr``.

The build requires:

* A Rust toolchain with ``cargo``
* ``python3-docutils`` for the manual pages
* The development files of OpenSSL, used by the download code of
  ``flake-ctl``

.. note::

   ``make install`` honors ``DESTDIR`` and ``PREFIX``, which allows
   to install into a staging area, e.g
   ``make install DESTDIR=/tmp/root``.

Privileges
==========

Flake Pilot does not install any setuid binary. Actions which need
more privileges than the calling user has, like mounting an image or
creating a network device, are delegated to the ``sudo`` binary of
the system. Therefore a user who registers or runs a flake that
requires such an action needs the corresponding ``sudo`` permissions.

Applications which run rootless, e.g a plain container flake started
through rootless podman, do not need any of this.

Next Steps
==========

Continue with :ref:`getting-started` to prepare the registry of your
user and to register a first application.
