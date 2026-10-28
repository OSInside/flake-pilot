BUBBLEWRAP-PILOT(8)
===================

NAME
----

**bubblewrap-pilot** - Launcher for flake applications

DESCRIPTION
-----------

A flake application is an application which gets called through
a runtime engine. bubblewrap-pilot runs the application in a
sandbox created by the **bwrap** program. The sandbox is a new
root system, created from a directory tree on the host which is
provided as an overlay. The application can modify its root
filesystem, the tree on the host stays untouched.

bubblewrap-pilot provides the application launcher binary and is not
expected to be called by users. Instead it is being used as the symlink
target at the time an application is registered via
**flake-ctl bubblewrap register**.

This means bubblewrap-pilot is the actual binary called with any
application registration. If the registered application is requested as
`/usr/bin/myapp` there will be a symlink pointing to:

.. code:: bash

   /usr/bin/myapp -> /usr/bin/bubblewrap-pilot

Consequently calling **myapp** will effectively call **bubblewrap-pilot**.
bubblewrap-pilot now reads the calling program basename, which is **myapp**
and looks up all the registration metadata stored in
`/usr/share/flakes`

Below `/usr/share/flakes` each application is registered
with the following layout:

.. code:: bash

   /usr/share/flakes/
       ├── myapp.d
       │   └── other.yaml
       └── myapp.yaml

   /etc/flakes/
       └── myapp.d
           └── local.yaml

All metadata information read by **bubblewrap-pilot** uses the YAML
markup. The main configuration `myapp.yaml` is read first
and can be optionally extended with further `*.yaml` files
below the `myapp.d` directory. All files in the
`myapp.d` directory will be read in alpha sort order.
Redundant information will always overwrite the former one.
Thus the last setting in the sequence wins.

For a system wide registration the `myapp.d` directory below
`/etc/flakes` is read last, also in alpha sort order. It allows to
locally adapt a registration which is provided as part of the system
without changing its files. Registrations of a user, see the
rootless mode in **flake-pilot**, do not read it.

A system wide registration can be called by any user. If there is no
system wide registration for the given application name, the
registration of the calling user in `~/.config/flakes` is used. Such
a flake is called by that user. In both cases the sandbox is created
for and belongs to the calling user, bwrap needs no privileges to
create it.

From a content perspective the following registration parameters
can be set for the sandbox engine:

.. code:: yaml

   sandbox:
     # Mandatory registration setup
     # Path of the directory tree on the host which becomes the
     # root filesystem of the sandbox. The tree is the read only
     # layer of an overlay and stays untouched. Everything the
     # application writes to its root filesystem is kept in a tmpfs
     # until the sandbox terminates, see the description of the
     # overlay below
     name: path/to/rootfs/on/host

     # Path of the program to call inside of the sandbox (target)
     target_app_path: path/to/program/in/sandbox

     # Path of the program to register on the host
     host_app_path: path/to/program/on/host

     # Optional registration setup
     # Sandbox runtime parameters
     runtime:
       # Optional pilot options in the format:
       # - %name or %name:value
       # Pilot options are not passed to the application call
       # but control the behavior of bubblewrap-pilot. An option
       # configured here is always effective and does not have
       # to be given at call time. An option of the same name
       # provided at call time takes precedence over the
       # configured one. As the '%' character is reserved in
       # YAML the option has to be quoted. For the list of
       # available options see the OPTIONS section
       # Example:
       pilot_options:
         - "%chdir:/data"

       # Caller arguments for the bwrap engine in the format:
       # - BWRAP_OPTION_NAME_AND_OPTIONAL_VALUE
       # An option is configured together with its values in
       # one entry, they are passed on to bwrap as separate
       # arguments. For details on bwrap options please consult
       # the bwrap documentation.
       # Example:
       bubblewrap:
         - --ro-bind /etc/resolv.conf /etc/resolv.conf
         - --bind %HOME /home/user
         - --dev /dev
         - --proc /proc
         - --tmpfs /tmp
         - --unshare-pid
         - --die-with-parent

After reading of the app configuration information the sandbox is
created and the application is called inside of it. Prior to the
call of bwrap the pilot moves into a user and mount namespace of
its own, in which bwrap then creates the sandbox. The namespace
maps only the user and the group of the caller, as themselves, and
all mounts in it are private to it. In the namespace the pilot
mounts the rootfs of the flake as an overlay filesystem. The rootfs
is the lowest, read only layer of that overlay, its upper and work
directory live in a tmpfs. The tmpfs is mounted with the
permissions 700 on a directory below `/var/tmp` which is named
after the ID of the calling user. The pilot creates that directory
with the permissions 700. On the host it stays empty, it only
serves as the mount point. All mounts of the setup are created
nosuid and nodev. For the user with the ID 1000 the following setup
is used in the namespace:

.. code:: bash

   /var/tmp/bwrap_1000/   <- tmpfs
       ├── merged         <- overlay mount of the rootfs
       ├── upper          <- upper directory of the overlay
       └── work           <- work directory of the overlay

None of these mounts exists on the host. Nobody on the host can see
or access them, and they are gone along with everything written to
them when the sandbox and the pilot have terminated. Each sandbox
has a namespace of its own, two sandboxes of the same user
therefore do not share the setup although it is mounted on the same
directory. The instance name, see @NAME, does not change these
paths.

Mounting in a namespace of its own needs no privileges, the user of
the sandbox creates the mounts itself. Neither sudo nor
**flake-pilot-helper**\(8) is involved. It requires a host which
allows unprivileged user namespaces, which bwrap without the setuid
bit needs anyway, and a kernel which allows overlay mounts in a
user namespace, Linux 5.11 or newer.

This does not work for a rootfs or a layer, see **--overlay-src**
below, with mount points below it, e.g the root filesystem of the
host. The mounts of the host are locked in a user namespace, and
the kernel refuses an overlay which would reveal what such a mount
covers. The pilot therefore checks `/proc/self/mountinfo` for mount
points below the rootfs and the layers, a mount on the directory
itself does not count. If there is one, the setup is created by the
helper, see **flake-pilot-helper**\(8), called through **sudo** for
a caller who is not root. The flake is handed over to the helper as
a whole, the pilot only passes the application and the arguments of
the caller:

.. code:: bash

   sudo /usr/libexec/flake-pilot/flake-pilot-helper bubblewrap-pilot \
       --app APP [--size SIZE] [--term TERM] -- ARGUMENTS

The helper takes the rootfs and the layers from the registration of
the flake. The registration of a flake registered system wide
belongs to root. The flakes configuration `~/.config/flakes.yml` and
the registration of a flake of the caller are read with the identity
of the caller.

The helper moves into a new mount namespace and makes all of its
mounts private, nothing propagates to or from the host. It opens
the directory of the user below `/var/tmp` without following a
symbolic link, the directory has to belong to the caller. Its
parents have to belong to root and must not be writable for others
unless they are sticky, like `/var/tmp`. The rootfs and the layers
are opened with the identity of the caller, a tree the caller cannot
reach is refused. The helper mounts the same setup as described
above, the tmpfs belongs to the caller. Then it drops its privileges
for good, to the groups, the group and the user ID of the caller,
and sets the no_new_privs flag, which keeps every program in the
namespace from gaining privileges, e.g through a setuid program.
Finally it starts bubblewrap-pilot in the mount namespace of the
sandbox, as the caller. The pilot takes the bwrap options and the
application in the sandbox from the registration as well, and
replaces itself by bwrap. The caller can therefore not change what
runs in the sandbox, only the arguments of the application. For a
system wide flake this lets a sudo rule grant single flakes, see the
reference policy flake-pilot.sudoers shipped with the flake-pilot
package. The instance of the flake is recorded by the pilot which
called the helper. Nothing is mounted on the host in this case
either.

The pilot started by the helper gets a fresh environment: PATH is
set to `/usr/sbin:/usr/bin:/sbin:/bin`, HOME, USER and LOGNAME to the
values of the user database for the caller, and TERM to the one of
the caller if it is valid. A variable the bwrap options of the flake
reference as %NAME, in the layers as well as in the other options,
resolves to the value of that environment. With **--clearenv** the
variables LANG, LC_ALL and TZ of the caller are therefore not set in
the sandbox of such a flake.

The merged directory of the overlay becomes the root filesystem of
the sandbox. It is bound writable as the root of the sandbox, which
is always the first option of the sandbox:

.. code:: bash

   --bind %OVERLAYROOT /

The variable **%OVERLAYROOT** resolves to the merged directory,
`/var/tmp/bwrap_1000/merged` in the above example. The path is only
valid in the namespace of the sandbox, which is where bwrap
evaluates its options. It can also be referenced in the bwrap
runtime arguments of the flake to access the root filesystem of the
sandbox.

Everything the application writes to its root filesystem ends up
in the tmpfs of the sandbox, nothing of it is written to disk.
Exceptions are paths covered by mounts of their own inside of the
sandbox: data written below `/tmp` with the default options, see
below, is kept in a tmpfs created by bwrap, and data written to a
path which is bound writable from the host modifies that path on
the host.

A flake can provide further directories for the root of the
sandbox by configuring **--overlay-src** options. They are not
passed to bwrap, the directories become layers of the overlay
instead. They are stacked on top of the rootfs in the order they
are configured, the last one of them wins for a path which exists in
more than one layer. Like the rootfs they are read only. All other
options, the ones from the flake configuration as well as the
default ones, are added after the **--bind** option. For the
configuration:

.. code:: yaml

   bubblewrap:
     - --overlay-src /data/tools
     - --unshare-pid

the overlay stacks `/data/tools` on top of the rootfs and the
sandbox is created with:

.. code:: bash

   --bind /var/tmp/bwrap_1000/merged /
   --unshare-pid

If layers are configured, the rootfs is provided to the overlay
through a read only overlay of its own, mounted on the directory
`lower` of the tmpfs and stacked on the empty directory `empty`.
Layers are mostly part of the rootfs, and an overlay refuses lower
directories of which one contains another.

When the sandbox has terminated its mounts are gone, the data the
application has written to its root filesystem is not kept. Every
call starts from the unmodified rootfs. Data which should outlast
the call has to be written to a path which is bound writable from
the host.

The size of the tmpfs is limited by the **sandbox_tmpfs_size**
setting in `/etc/flakes.yml`, which only root can write. It is the
size of the tmpfs of every sandbox and the largest size a flake can
request with the **%tmpfs_size** option, see OPTIONS. Without the
setting the kernel default applies, half of the memory of the host.
For a sandbox the pilot mounts itself the limit is applied by
bubblewrap-pilot. As the user creates the tmpfs in a namespace of
its own, the limit is a default of the system there, not a
privilege boundary. The memory of the tmpfs is accounted to the
processes of the user and is released when the sandbox terminates.
For a sandbox mounted by the helper the limit is enforced by the
helper.

Former versions kept the written data on disk, in the directories
`myapp@one_rw` and `myapp@one_work`, or in tmpfs mounts on the
host, `myapp@one_overlay` and `myapp@one_merged`, below
`/var/tmp/bwrap_1000`. They are not used anymore. A tmpfs mount a
former version has left on the host stays until the host is
rebooted or root unmounts it, the remaining directories can be
deleted by the user. The **%discard** option of former versions is
gone as well. A pilot option which is unknown is ignored, a flake
registered with it keeps working.

If no bwrap runtime arguments exists, the following defaults will
apply:

.. code:: bash

   --dev /dev --proc /proc --tmpfs /tmp --unshare-pid --unshare-ipc
   --unshare-uts --new-session --die-with-parent --clearenv
   --setenv NAME VALUE ...

This provides the standard pseudo filesystems, a writable `/tmp`,
private process ID, IPC and host name namespaces and the
termination of the sandbox together with the pilot. The application
runs in a session of its own, without the terminal of the caller as
its controlling terminal. It can therefore not inject input into
that terminal, but an interactive shell in the sandbox has no job
control. The environment of the application is cleared, only the
variables PATH, HOME, USER, LOGNAME, TERM, LANG, LC_ALL and TZ are
set again with the values of the caller, as far as they are set for
the caller. This applies to every sandbox whose options contain
**--clearenv**, a variable the options set with **--setenv** keeps
the configured value. Without **--clearenv** the application of a
sandbox created by the helper only sees the fresh environment the
helper starts bwrap with, see above. Configuring runtime arguments
replaces these defaults, they have to be listed along with the
custom options if they should stay in effect. A registration created
with **flake-ctl bubblewrap register** lists them in the
configuration file and adds the options of the registration to them.

Unless a working directory is configured, the application is
called in the root directory of the sandbox. This is because the
working directory of the caller usually does not exist in the new
root system.

The bwrap runtime arguments allows to set environment variable
placeholders starting with '%' and followed by the name of the
environment variable. For example %HOME will be replaced with the
value of $HOME of the calling user. If the given placeholder
cannot be translated into an existing environment variable it
will be turned into the variable name, $HOME in the above
example

All caller arguments will be passed to the program called inside
of the sandbox except for arguments that start with the '@'
or '%' sign. Caller arguments of this type are only used for
the bubblewrap-pilot startup itself. See the OPTIONS section
for the available runtime options.

The sandbox exists as long as the application running in it. There
is one sandbox per registered flake command or, if the application
is called with @NAME arguments, per command instance. Calling the
same instance twice at the same time is refused. For each instance
the pilot writes a meta data file which contains the process ID of
the sandbox, for a sandbox created by the helper the one of the
sudo process the helper runs in. The files are stored below
`/tmp/flakes` in a private directory of the user the instance
belongs to, e.g `/tmp/flakes/1000/myapp@one.bwrapid` and are
deleted when the application terminates. They can be listed with
**flake-ctl bubblewrap show**.

The exit code of the application in the sandbox becomes the exit
code of the pilot. An application terminated by a signal is
reported as a failure.

All options listed below, except for @NAME, can also be set
permanently for an application through the **pilot_options**
setting of the flake configuration. Such an option is always
effective and does not have to be given at call time. Passing
the option at call time takes precedence over the configured
value.

OPTIONS
-------

@NAME

  This allows users to distribute the exact same program call to
  different instances. Each instance gets its own sandbox and its
  own meta data file. The name may consist of alphanumeric
  characters and the characters '=', '_' and '-'

%chdir:DIRECTORY

  Call the application in the given directory of the sandbox
  instead of its root directory. A working directory configured
  as a bwrap option of the flake, **--chdir**, takes precedence
  over this option

%tmpfs_size:SIZE

  Limit the tmpfs which keeps the writable layer of the sandbox to
  the given size: a number of bytes with an optional k, m, g or t
  suffix, e.g 512M, or a percentage of the memory of the host, e.g
  10%. The size must not exceed the **sandbox_tmpfs_size** limit of
  `/etc/flakes.yml`, the limit is the default

DEBUGGING
---------

bubblewrap-pilot provides more inner works details if the following
environment variable is set:

.. code:: bash

   export PILOT_DEBUG=1

FILES
-----

* /usr/share/flakes
* $HOME/.config/flakes
* /tmp/flakes
* /var/tmp/bwrap_USERID
* /usr/libexec/flake-pilot/flake-pilot-helper
* /etc/flakes.yml

SEE ALSO
--------

flake-ctl-bubblewrap-register(8), flake-ctl-bubblewrap-remove(8), flake-ctl-bubblewrap-show(8), flake-pilot-helper(8), bwrap(1)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
