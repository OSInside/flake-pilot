.. _getting-started:

===============
Getting Started
===============

.. hint:: **Abstract**

   This chapter prepares the host for the examples of this guide and
   registers a first application.

System Wide and Rootless Registration
=====================================

Flake Pilot knows two registries, and ``flake-ctl`` detects from the
caller which of them to operate on. There is no option to select the
mode:

* Called as the **root** user the command manages the system wide
  setup. Flakes are registered below ``/usr/share/flakes`` and the
  images are stored in the system wide registries of the engines.
  Applications registered this way are available to everybody on the
  host.

* Called as **any other user** the command manages the setup of that
  user. Flakes are registered below ``$HOME/.config/flakes`` and the
  images are stored in the registries of that user, rootless mode.

The examples in this guide register the apps for the calling user.

Preparing the Environment
=========================

All apps will be registered in the users home directory. Therefore
it's handy to add that path to the environment:

.. code-block:: bash

   mkdir -p ~/bin
   export PATH=$PATH:$HOME/bin

Add the ``export`` line to the shell profile to make it permanent.

Creating the User Setup
=======================

The setup for the rootless mode is created once via:

.. code-block:: bash

   flake-ctl init

The command creates the flake registry of the user below
``$HOME/.config/flakes``, the user specific configuration file
``$HOME/.config/flakes.yml`` and a podman storage configuration which
points podman to a storage location of that user. Files which already
exist are kept, they are only recreated if the ``--force`` option is
given.

To let plain ``podman`` commands operate on the flake storage of the
user as well, export the storage configuration as it is shown at the
end of the setup:

.. code-block:: bash

   export CONTAINERS_STORAGE_CONF=$HOME/.config/flakes/podman/storage.conf

.. note::

   ``flake-ctl init`` creates the user specific setup only. The
   system wide setup is provided by the ``flake-pilot`` package,
   calling the command as the root user is an error.

Your First Flake
================

The following registers Amazon's SDK utility as a container app
named ``aws``:

.. code-block:: bash

   flake-ctl podman register \
        --container docker.io/amazon/aws-cli --app $HOME/bin/aws --target /

   aws ec2 help

This creates ``$HOME/bin/aws`` on your host, which actually launches
the ``amazon/aws-cli`` container. The call of ``aws ec2 help``
launches an instance of the container via rootless podman and shows
the help text for the ``ec2`` subcommand.

The registration is visible in the registry of the user:

.. code-block:: bash

   flake-ctl list

Where to Continue
=================

* :ref:`container-apps` for more container based registrations,
  including delta containers and layered setups

* :ref:`vm-apps` for applications provided by a Firecracker virtual
  machine

* :ref:`application-setup` for the layout of the registry and the
  contents of a flake configuration
