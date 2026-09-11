.. _introduction:

============
Introduction
============

.. hint:: **Abstract**

   This chapter explains what Flake Pilot does, which components it
   is made of and which problems it is designed to solve.

What Is Flake Pilot
===================

Flake Pilot is software to register, provision, and launch
applications that are actually provided inside a runtime environment
like an OCI container or a Firecracker VM.

The user of such an application does not have to know this. The
application is called by its path like any other program, it receives
the arguments given on the command line and it uses the data of the
calling user. What happens in between, pulling an image, creating an
instance, layering a delta on top of a base, connecting a virtual
machine to the network, is the job of Flake Pilot.

The term **flake** is the short name for an application which runs
inside an isolated environment. Registering an application as a flake
means to create two things:

1. A symbolic link at the path of the application which points to the
   launcher binary of the engine that provides it

2. A configuration file in the flake registry which describes the
   image, the target program inside of it and the options to run it
   with

Components
==========

.. image:: ../images/architecture.png
   :align: center
   :alt: Flake Pilot architecture overview

There are two main components:

The Launchers
-------------

The launcher binary. Each application that was registered as a flake
is redirected to a launcher binary. As of today, support for the
``podman`` and ``firecracker`` engines is implemented, leading to the
respective ``podman-pilot`` and ``firecracker-pilot`` launcher
binaries.

A launcher, also called a pilot, reads the flake configuration that
belongs to the name it was called with, provisions the instance and
runs the target program inside of it. Standard input, standard output
and the exit code are passed through, which is what makes the call
feel native.

The Flake Registration Tool
---------------------------

``flake-ctl`` is the management utility to list, register, remove,
and more... flake applications on your host. It provides one
subcommand per engine, e.g ``flake-ctl podman`` and
``flake-ctl firecracker``, plus the commands which are common to all
engines like ``flake-ctl list``.

Use Cases
=========

* Running AI workloads in isolated environments.

* Delta containers used together with a base container such that only
  small delta containers are pulled to the registry, used with a base
  that exists only once.

* Include arbitrary data without harming host integrity, e.g custom
  binaries, proprietary software not following package guidelines and
  standards.

* Layering of several containers, e.g deltas on top of a base.
  Building a solution stack, e.g base + python + python-app.

* Provisioning app dependencies from the host instead of providing
  them in the container, e.g a delta container providing the app
  using a base container but taking the certificates or other
  sensitive information from the host; a three-way dependency model.

* Isolating applications that require different library versions than
  those the host provides, e.g old legacy applications.

* and maybe more...

Choosing an Engine
==================

Both engines provide isolation, but on a different level and at a
different price:

``podman``
   Container based isolation. The application shares the kernel of
   the host, starts fast and has direct access to the resources you
   hand to it. This is the right choice for most applications, see
   :ref:`container-apps`.

``firecracker``
   Virtual machine based isolation. The application runs on its own
   kernel inside a microVM, which separates it from the host much
   more strictly at the cost of a boot and of an explicit network
   setup, see :ref:`vm-apps`.

.. note::

   For container based flakes the isolation level can also be raised
   without leaving the ``podman`` engine, by running the container
   with the ``krun`` runtime. See :ref:`krun-runtime`.
