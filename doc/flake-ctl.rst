FLAKE-CTL(8)
============

NAME
----

**flake-ctl** - Load and Register flake applications

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl <SUBCOMMAND>

   OPTIONS:
       -h, --help       Print help information
       -V, --version    Print version information

   SUBCOMMANDS:
       help         Print this message or the help of the given subcommand(s)
       init         Create the setup to run flake applications
       list         List registered container applications
       podman       Load and register OCI applications
       firecracker  Load and register VM applications

DESCRIPTION
-----------

flake-ctl is the control program to register and manage flake applications
which actually runs inside of an instance created by a runtime engine.
Currently supported runtime engines are:

* podman
* firecracker

An application registered via flake-ctl can be called on the host like a
native application just by calling the name used in the
registration process.

USER MODE
---------

flake-ctl detects from the caller which setup to operate on. There
is no option to select the mode:

* Called as the root user the command manages the system wide setup.
  Flakes are registered below /usr/share/flakes and the images are
  stored in the system wide registries of the engines

* Called as any other user the command manages the setup of that
  user. Flakes are registered below $HOME/.config/flakes and the
  images are stored in the registries of that user, rootless mode.
  The user specific setup has to exist and is created once by
  calling **flake-ctl init**

SEE ALSO
--------

podman-pilot(8), flake-ctl-init(8), flake-ctl-list(8), flake-ctl-podman-load(8), flake-ctl-podman-register(8), flake-ctl-podman-remove(8), flake-ctl-podman-show(8), firecracker-pilot(8), flake-ctl-firecracker-load(8), flake-ctl-firecracker-register(8), flake-ctl-firecracker-remove(8), flake-ctl-firecracker-show(8), flake-ctl-firecracker-network-init(8), flake-ctl-firecracker-network-add(8), flake-ctl-firecracker-network-remove(8), flake-ctl-firecracker-volume-export(8), flake-ctl-firecracker-volume-release(8), flake-ctl-firecracker-volume-add(8), flake-ctl-firecracker-volume-remove(8)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
