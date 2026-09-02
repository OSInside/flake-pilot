SCI(8)
======

NAME
----

**sci** - Execute provided command in virtualized environment

SYNOPSIS
--------

.. code:: bash

   USAGE:
       sci

   OPTIONS:
       *none*


DESCRIPTION
-----------

**NOTE**
    sci is not meant to be called as a user tool. At the end of execution
    sci will send a sysreq signal. sci is meant to be exclusively executed 
    inside a firecracker instance!


Simple Command Init (sci) is a tool which executes the provided
command in the run=... cmdline variable after preparation of an
execution environment for the purpose to run a command inside
of a firecracker instance.

Inside the fircracker.json configuration file kernel boot parameters
can be provided. Here various environment variables can be set.
Available variables are:


    + run= command
    + overlay_root= /dev/block_device
    + nfs= NAME_OR_IP:/export_path:/mount_path[,...]


If provided via the overlay_root=/dev/block_device kernel boot
parameter in the firecracker.json file,
sci also prepares the root filesystem as an overlay
using the given block device for writing.

For the overlay_root parameter to work the firecracker.json file
needs to have a proper section with
a record of the overlayfs on the root system.

If provided via the nfs= kernel boot parameter, sci mounts the
listed NFS volumes before the command is called. The parameter
takes a comma separated list of volumes, each of them given as
the name or address of the server, the path exported by that
server and the path it is mounted on in the instance:

.. code:: bash

   nfs=some.host:/host/path:/local/path,some.host:/other/path:/local/other

leads to the following mount calls:

.. code:: bash

   mount -t nfs some.host:/host/path /local/path
   mount -t nfs some.host:/other/path /local/other

A mount point which does not exist in the instance is created.
A volume which cannot be read from the given specification, or
which fails to mount, is skipped and the remaining volumes are
still mounted.

For the nfs parameter to work the instance needs a working
network setup, see **flake-ctl-firecracker-network-add**(8), and
has to provide the NFS client tools. Mounting a filesystem of
this type requires the ``mount.nfs`` helper program.

Every environment variable configurable and all options 
regarding filesystems are stored in the firecracker.json
file for the individual instance. Having this in mind, the desired values should
ideally be set in the belonging file.

.. _repository: https://github.com/OSInside/flake-pilot/tree/main/firecracker-pilot/template

For a working example refer to the firecracker.json template at the offical
repository_



ENVIROMENT VARIABLES
--------------------

+----------------------+-------------------+----------------------------------+
| Variable             | Value             | Description                      |       
+======================+===================+==================================+
|                      |                   |                                  |
|                      |                   |                                  |
| run                  | command           | sci will replace init and        |
|                      |                   | execute the provided command     |
|                      |                   | at startup                       |
|                      |                   |                                  |
+----------------------+-------------------+----------------------------------+
|                      |                   |                                  |
|overlay_root          | /dev/block_device | if the rootfs is read only       |
|                      |                   | an overlay is required to        |
|                      |                   | write to the filesystem.         |
|                      |                   | Each application will maintain   |
|                      |                   | their own specific overlay.      |
|                      |                   | Changes to rootfs will be        |
|                      |                   | stored in the overlay and applied|
|                      |                   | to the individual rootfs.        |
|                      |                   | Changes to the original rootfs   |
|                      |                   | will not be made.                |
|                      |                   |                                  |
+----------------------+-------------------+----------------------------------+
|                      |                   |                                  |
|nfs                   | NAME_OR_IP:       | NFS volume(s) to mount before    |
|                      | /export_path:     | the command is called. More than |
|                      | /mount_path       | one volume can be given as a     |
|                      |                   | comma separated list. A mount    |
|                      |                   | point which does not exist is    |
|                      |                   | created. The instance needs a    |
|                      |                   | working network setup and the    |
|                      |                   | NFS client tools for this.       |
|                      |                   |                                  |
+----------------------+-------------------+----------------------------------+

FILES
-----

* /usr/sbin/sci

NOTION
------

sci will execute these steps in order:

    + evaluation of environment variable 'run'
    + mounting of overlay if requested
    + switching root into overlay if configured
    + mounting of the nfs volumes if requested
    + execution of provided command
    + reboot of firecracker instance



AUTHOR
------

André Barthel

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
