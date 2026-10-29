FLAKE-CTL-FIRECRACKER-REGISTER(8)
=================================

NAME
----

**flake-ctl firecracker register** - Register VM application

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl firecracker register [OPTIONS] --vm <VM> --app <APP>


   OPTIONS:
       --app <APP>
       --force
       --pilot-option <PILOT_OPTION>...
       --resume
       --force-vsock
       --overlay-size <OVERLAY_SIZE>
       --target <TARGET>
       --user <USER>
       --vm <VM>

DESCRIPTION
-----------

Register the given application to run inside of the specified firecracker
virtual machine. The registration process is two fold:

1. Create the application symlink pointing to `/usr/bin/firecracker-pilot`
2. Create the application default configuration below `/usr/share/flakes`.
   Each application registered is called a **flake**. A system wide
   flake also gets an empty drop-in directory `/etc/flakes/APP.d`
   for local adaptions of its registration

The registry to register in is detected from the caller. Called as any
user other than root the registration is done in the user specific,
rootless registry of that user. The flake configuration is created
below `~/.config/flakes` and the VM is looked up in
`~/.config/flakes/firecracker/images`

On successful completion the registered *--app* name can be called
like a normal application on this host.

The registration creates no network setup. The ``ip=`` option is
deleted from the kernel commandline of the VM. Without it the VM is
not connected, the pilot creates no TAP device setup for it and
passes no ``network-interfaces`` section to firecracker. The
``rd.neednet=0`` option is set such that the initrd does not wait
for a network which never comes up and would block the boot of the
VM. The setup can be created later on with
**flake-ctl-firecracker-network-add**\(8)

For further details about the flake configuration please refer to
the **firecracker-pilot** manual page.

OPTIONS
-------

--app <APP>

  An absolute path to the application on the host. If not specified via
  the target option, the application will be called with that path inside
  of the VM

--force

  Force writing the registration even if a registration
  of the same name already exists. This is done by deleting
  an eventual existing registration prio creating the new
  registration. Please have in mind that a failed registration
  still causes an eventual existing former registration to be
  deleted in this case !

--overlay-size <OVERLAY_SIZE>

  Size of overlay write space in bytes. Optional suffixes are:
  KiB/MiB/GiB/TiB (1024) or KB/MB/GB/TB (1000)

--pilot-option <PILOT_OPTION>...

  Pilot option, and optional value, in the format %name or
  %name:value. Pilot options are not passed to the application
  call but control the behavior of firecracker-pilot. An option
  registered here is always effective and does not have to be
  given at call time. Passing the option at call time takes
  precedence over the registered value. This option can be
  specified multiple times. For the list of available pilot
  options please refer to the **firecracker-pilot** manual page.

--resume

  Resume the VM from previous execution. If the VM is still running,
  the app will be executed inside of this VM instance

--force-vsock

  Force using a vsock to communicate between guest and
  host if resume is set to false. In resume mode the vsock
  setup is always required.

--target <TARGET>

  An absolute path to the application in the VM. Use this option if the application path
  on the host should be different to the application path inside of the VM

--user <USER>

  Name of the user to call the application as inside of the VM.
  The name is stored as ``user`` in the runtime section of the
  flake configuration. The application runs with the user and
  group ID and with the groups of this user as they are setup in
  the VM. If the user does not exist in the VM, the application
  is called as root and a warning is printed in debug mode.
  The name may consist of up to 32 characters out of
  [A-Za-z0-9._-] and has to start with a letter or '_'

--vm <VM>

  A virtual machine name. The name must match with a name in the local firecracker
  registry


FILES
-----

* /usr/share/flakes
* /etc/flakes/APP.d
* ~/.config/flakes
* /etc/flakes.yml

EXAMPLE
-------

.. code:: bash

   $ flake-ctl firecracker register --vm NAME \
       --overlay-size 20GB \
       --app /usr/bin/apt-get

   $ flake-ctl firecracker register --vm NAME \
       --app /usr/bin/apt-get \
       --resume \
       --pilot-option '%port:2000'

   $ flake-ctl firecracker register --vm NAME \
       --app /usr/bin/bash \
       --user john

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
