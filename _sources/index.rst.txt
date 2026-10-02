.. Flake Pilot User Guide, master document

=======================
Flake Pilot User Guide
=======================

.. rubric:: Application Isolation - Secure Execution with a Native Feel

Flake Pilot registers, provisions and launches applications that are
not installed on your host but are provided inside a runtime
environment such as an OCI container, a Firecracker virtual machine or
a bubblewrap sandbox.
The registered application behaves like any other program on the
system: it is called by its name, it reads and writes the data you
point it to, and it returns its exit code to your shell. Everything
that is needed to run it, the image, the engine and the provisioning
of the instance, is handled behind that name.

An application registered this way is called a **flake**.

.. image:: ../images/architecture.png
   :align: center
   :alt: Flake Pilot architecture overview

About This Guide
================

This guide is written for administrators and developers who want to
provide isolated applications on a Linux host. It explains the
concepts behind flakes, shows how to register applications for the
``podman``, ``firecracker`` and ``bubblewrap`` engines, describes the
network setup for virtual machines and documents the layout of the
flake configuration.

The command line of each tool is documented in the manual pages
shipped with the packages, e.g ``man 8 flake-ctl`` or
``man 8 podman-pilot``. This guide references them where the details
matter.

How to Get Started
==================

Install the packages of the engines you want to use as described in
:ref:`installation`. Every engine comes with its own pilot and only
the ones you registered applications for are needed.

Next, prepare your user environment. Applications registered as a
normal user are placed in a directory of your choice, typically
``$HOME/bin``, which has to be part of your search path:

.. code-block:: bash

   mkdir -p ~/bin
   export PATH=$PATH:$HOME/bin

   flake-ctl init

``flake-ctl init`` creates the registry below ``$HOME/.config/flakes``
and the engine configuration that belongs to it. Called as ``root``
the registry is created in ``/usr/share/flakes`` and the registered
applications are available to everybody on the host.

With that in place a first application is one command away:

.. code-block:: bash

   flake-ctl podman register \
        --container docker.io/amazon/aws-cli --app $HOME/bin/aws --target /

   aws ec2 help

``flake-ctl list`` shows what is registered on your host. The complete
walk through, including the pitfalls of the user setup, is described
in :ref:`getting-started`.

Isolating AI Workloads With Firecracker
=======================================

AI tools are moving fast, they are rarely packaged for your
distribution, they want to read your source tree and they talk to the
network on their own. Running them in a Firecracker virtual machine
keeps them on a kernel of their own and lets you decide which part of
the host they can reach.

.. code-block:: bash

   flake-ctl firecracker pull --name claude \
       --kis-image https://ddrasqgvrmpt8.cloudfront.net/claude.x86_64-1.15.6-0.tar.xz

   flake-ctl firecracker register --vm claude \
       --app $HOME/bin/claude --target /bin/bash \
       --overlay-size 20GiB --force-vsock --resume

   claude

The application feels local, but the code it runs never touches the
host system. The write layer of the instance lives in an overlay of
its own and the data you want the tool to see is shared explicitly.
:ref:`vm-apps` shows the complete example, :ref:`firecracker-networking`
connects the instance to the outside world and
:ref:`firecracker-volumes` shares a host directory with it over NFS.

An Application Collection With Podman
=====================================

Cloud SDKs, vendor tools and language specific utilities are often not
packaged by your distribution, or only in a version that is too old.
Vendors do publish container images for them, and a podman flake turns
such an image into a program on your host:

.. code-block:: bash

   flake-ctl podman register \
       --container gcr.io/google.com/cloudsdktool/google-cloud-cli:stable \
       --app $HOME/bin/gcloud --target /usr/bin/gcloud

   gcloud version

From here on ``gcloud`` is just a command. Collect the tools you need
this way and your host stays clean of the dependencies they drag in,
while each of them can be updated, pinned to a version or dropped
again on its own. :ref:`container-apps` covers the registration
options, engine options and delta containers which keep the data to
pull small.

A Throw Away Host With Bubblewrap
=================================

Some tasks want to modify the host: an installation you are not sure
about, a build that scatters files outside of its build directory, a
script from the internet you would rather watch first. The
``bubblewrap`` engine mounts a directory tree, the host root system
included, as the read only layer of an overlay:

.. code-block:: bash

   flake-ctl bubblewrap register --rootfs / \
       --app $HOME/bin/protected-shell --target /bin/bash

   protected-shell

Inside of ``protected-shell`` the host looks and behaves as usual,
packages can be installed and files can be added or overwritten, but
all of it is written to the overlay and is gone when the application
ends. Paths you do want to keep are bound into the sandbox explicitly.
:ref:`sandbox-apps` describes the tree, the overlay and the options to
share data with the host.

.. toctree::
   :hidden:
   :maxdepth: 2
   :numbered:

   introduction
   installation
   getting_started
   container_apps
   vm_apps
   firecracker_networking
   firecracker_volumes
   sandbox_apps
   application_setup
   building_images
   troubleshooting

Resources
=========

* Source code and issue tracker:
  https://github.com/OSInside/flake-pilot

* Packages:
  https://build.opensuse.org/package/show/Virtualization:Appliances:Builder/flake-pilot

* Manual pages:
  https://github.com/OSInside/flake-pilot/tree/main/doc

Feedback is very much welcome.
