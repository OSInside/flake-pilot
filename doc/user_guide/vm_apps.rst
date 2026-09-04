.. _vm-apps:

====================================
Applications From a Virtual Machine
====================================

.. hint:: **Abstract**

   This chapter shows how to register applications which are
   provided by a Firecracker virtual machine. All examples register
   the app for the calling user and expect the setup described in
   :ref:`getting-started`.

Registering a VM application takes two steps: the image is pulled
into the local firecracker registry and the application is registered
against the name of that image.

Pulling an Image
================

A firecracker image consists of a kernel, an initrd and a root
filesystem. ``flake-ctl firecracker pull`` fetches these components
into ``/var/lib/firecracker/images/NAME`` for a system wide setup, or
into ``~/.config/flakes/firecracker/images/NAME`` for the setup of a
user.

The recommended form is a KIS image archive, a single archive which
carries all components:

.. code-block:: bash

   flake-ctl firecracker pull --name leap \
       --kis-image https://github.com/OSInside/flake-pilot/raw/refs/heads/main/appstore/firecracker/leap.x86_64-1.15.6-0.tar.xz

An image pulled this way takes part in a checksum based update check.
The archive has to be accompanied by a ``.sha256`` file at the same
location. Pulling the same name again compares the checksums, fetches
a new version only if they differ and leaves the registry untouched
if the image is up to date.

.. _vm-example-fireshell:

A Shell as a Firecracker VM App
===============================

.. code-block:: bash

   flake-ctl firecracker pull --name leap \
       --kis-image https://github.com/OSInside/flake-pilot/raw/refs/heads/main/appstore/firecracker/leap.x86_64-1.15.6-0.tar.xz

   flake-ctl firecracker register --vm leap \
       --app $HOME/bin/fireshell --target /bin/bash --overlay-size 20GiB

   fireshell

This registers an app named ``fireshell`` to the system. Once called,
a Firecracker VM, based on the pulled ``leap`` image, is started and
drops you into a bash shell. In addition, some write space of 20GB is
added to the instance.

The registration creates no network setup. The VM boots without an
``ip=`` option on its kernel commandline, no TAP device is created
for it and no ``network-interfaces`` section is passed to
firecracker. The setup can be added later on, see
:ref:`firecracker-networking`.

.. note::

   Data transfer from the virtual machine to the host is done through
   the serial console. Alternatively a vsock based communication can
   be used. To do this specify the option ``--force-vsock`` when
   registering the application.

.. _vm-example-claude:

Claude AI as a Firecracker VM App
=================================

.. code-block:: bash

   flake-ctl firecracker pull --name claude \
       --kis-image https://github.com/OSInside/flake-pilot/raw/refs/heads/main/appstore/firecracker/claude.x86_64-1.15.6-0.tar.xz

   flake-ctl firecracker register --vm claude \
       --app $HOME/bin/claude --target /bin/bash \
       --overlay-size 20GiB --force-vsock --resume

   flake-ctl firecracker network init --outgoing-interface eth0
   flake-ctl firecracker network add --app $HOME/bin/claude

   claude

This registers an app named ``claude`` to the system. Once called, a
Firecracker VM, based on the pulled ``claude`` image, is started and
executes the ``bash`` shell. The communication is vsock based and the
VM instance is kept alive after the execution of the target program,
which allows for further calls to the same instance.

The two ``network`` commands connect the application to the outside
world, they are explained in :ref:`firecracker-networking`.

In the shell, you can setup access to claude AI for example through
Google Vertex AI as follows:

.. code-block:: bash

   export ANTHROPIC_VERTEX_PROJECT_ID=YOUR_PROJECT_ID
   gcloud auth application-default login --project $ANTHROPIC_VERTEX_PROJECT_ID

   claude

Registration Options in Short
=============================

``--vm``
   The name of the image in the local firecracker registry, the name
   used with ``flake-ctl firecracker pull --name``.

``--app`` and ``--target``
   The path of the application on the host and the program to call
   inside of the VM, like for container flakes.

``--overlay-size``
   Size of the write space added to the instance. Optional suffixes
   are KiB/MiB/GiB/TiB (1024) or KB/MB/GB/TB (1000).

``--resume``
   Keep the VM alive after the call. Further calls of the app are
   executed inside the running instance.

``--force-vsock``
   Use a vsock instead of the serial console to talk to the guest. In
   resume mode a vsock is always required.

``--pilot-option``
   A runtime option of the pilot, e.g ``%port:2000`` to bind the
   guest to host communication of a resume flake to a port of your
   choice. The option is stored in the ``pilot_options`` list of the
   flake configuration and is effective on every call. The same
   option given at call time takes precedence. The option can be
   specified multiple times.

See ``man 8 flake-ctl-firecracker-register`` for the complete list.
