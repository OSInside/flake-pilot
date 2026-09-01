.. _application-setup:

=================
Application Setup
=================

.. hint:: **Abstract**

   This chapter describes how to inspect the registered applications
   and their instances and where the configuration of a flake lives.

Listing Applications
====================

After an application is registered, it can be listed via:

.. code-block:: bash

   flake-ctl list

The instances created from the registered applications can be listed
per engine via:

.. code-block:: bash

   flake-ctl podman show

   flake-ctl firecracker show

Like all ``flake-ctl`` commands these operate on the setup of the
calling user, see :ref:`getting-started`.

The Flake Registry
==================

Each application provides a configuration below ``/usr/share/flakes/``
for system wide registration or ``~/.config/flakes/`` for user
specific ones. The term ``flake`` is a short name for an application
running inside an isolated environment. For the ``aws`` flake of
:ref:`container-example-aws`, the config file structure looks like the
following:

.. code-block:: text

   ~/.config/flakes/
   ├── aws.d
   └── aws.yaml

``NAME.yaml``
   The flake configuration. It describes the image, the target
   program, the engine options and, for VM applications, the kernel
   commandline of the instance.

``NAME.d``
   A drop-in directory. The files placed here are read in alpha sort
   order and are attached to the master ``NAME.yaml`` file before it
   is parsed. A setting given again replaces the one from the file
   before, which allows to extend or overrule a registration without
   editing the file written by ``flake-ctl``.

Both are plain text and are meant to be adjusted. A change becomes
effective with the next call of the application, no re-registration
is needed.

Removing an Application
=======================

A registration is deleted with the ``remove`` command of the engine
it belongs to, e.g:

.. code-block:: bash

   flake-ctl podman remove --app $HOME/bin/aws

   flake-ctl firecracker remove --app $HOME/bin/fireshell

Reference Documentation
=======================

Please consult the manual pages for detailed information about the
contents of the flake setup:

* ``man 8 flake-ctl`` and the manual page of each subcommand
* ``man 8 podman-pilot`` for container flakes
* ``man 8 firecracker-pilot`` for VM flakes
* ``man 8 flake-pilot`` for the registry layout and the user mode

The sources of the manual pages are also available online:

https://github.com/OSInside/flake-pilot/tree/main/doc
