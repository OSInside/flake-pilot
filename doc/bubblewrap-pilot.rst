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
     # across calls until it is discarded or the host is rebooted,
     # see the description of the overlay below
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
call of bwrap the pilot mounts the rootfs of the flake as an
overlay filesystem on the host. The rootfs is the lowest, read only
layer of that overlay, its upper and work directory live in a
tmpfs which is created for the instance. The directories of the
setup are created below `/var/tmp`, in a directory which is named
after the ID of the calling user. Thus two users running the same
flake do not use the same paths. The directories are created with
the permissions 700, nobody but the user of the sandbox can access
them. For an instance named `myapp@one` of the user with the ID
1000 the following setup is used:

.. code:: bash

   /var/tmp/bwrap_1000/
       ├── myapp@one_merged   <- overlay mount of the rootfs
       └── myapp@one_overlay  <- tmpfs with the upper/work dirs

Creating and deleting these mounts is not allowed for a standard
user. The pilot creates the directories, the mounts are created and
deleted by the helper, see **flake-pilot-helper**\(8), called through
**sudo** for a caller who is not root. The helper derives all paths
from the caller and the instance name, does not follow a symbolic
link and mounts the tmpfs and the overlay nosuid and nodev. The
sudo rule for the helper is described in the reference policy
flake-pilot.sudoers shipped with the flake-pilot package.

The merged directory of the overlay becomes the root filesystem of
the sandbox. It is bound writable as the root of the sandbox, which
is always the first option of the sandbox:

.. code:: bash

   --bind %OVERLAYROOT /

The variable **%OVERLAYROOT** resolves to the merged directory of
the instance, `/var/tmp/bwrap_1000/myapp@one_merged` in the above
example. It can also be referenced in the bwrap runtime arguments
of the flake to access the root filesystem of the sandbox as it
exists on the host.

Everything the application writes to its root filesystem ends up
in the tmpfs of the instance, nothing of it is written to disk.
Exceptions are paths covered by mounts of their own inside of the
sandbox: data written below `/tmp` with the default options, see
below, is kept in a tmpfs created by bwrap, and data written to a
path which is bound writable from the host modifies that path on
the host.

A flake can provide further directories for the root of the
sandbox by configuring **--overlay-src** options. They are not
passed to bwrap, the directories become layers of the overlay on the
host instead. They are stacked on top of the rootfs in the order they
are configured, the last one of them wins for a path which exists in
more than one layer. Like the rootfs they are read only. All other
options, the ones from the flake configuration as well as the
default ones, are added after the **--bind** option. For the
configuration:

.. code:: yaml

   bubblewrap:
     - --overlay-src /data/tools
     - --unshare-pid

the overlay on the host stacks `/data/tools` on top of the rootfs
and the sandbox is created with:

.. code:: bash

   --bind /var/tmp/bwrap_1000/myapp@one_merged /
   --unshare-pid

When the application has terminated the overlay mount is deleted.
The tmpfs below it is kept, the data written by the application is
therefore still there with the next call of the same instance, the
same flake called with the same @NAME selectors. Different instances
use a tmpfs of their own. The tmpfs uses memory of the host as long
as it exists. It is deleted with a reboot of the host, or when the
application is called with the **%discard** option, see OPTIONS. To
start an instance from the unmodified rootfs every time, register
the flake with **%discard** as pilot option.

The size of the tmpfs is limited by the **sandbox_tmpfs_size**
setting in `/etc/flakes.yml`, which only root can write. It is the
size of every tmpfs and the largest size a flake can request with
the **%tmpfs_size** option, see OPTIONS. Without the setting the
kernel default applies, half of the memory of the host.

Former versions kept the written data on disk, in the directories
`myapp@one_rw` and `myapp@one_work` next to the ones above. They are
not used anymore and are deleted along with the tmpfs when the
instance is called with **%discard**.

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
the configured value. Configuring runtime arguments replaces
these defaults, they have to be listed along with the custom
options if they should stay in effect. A registration created with
**flake-ctl bubblewrap register** lists them in the configuration
file and adds the options of the registration to them.

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
the sandbox. The files are stored below `/tmp/flakes` in a private
directory of the user the instance belongs to, e.g
`/tmp/flakes/1000/myapp@one.bwrapid` and are deleted when the
application terminates. They can be listed with
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

%discard

  Discard the writable layer of the instance when the application
  has terminated: the tmpfs which keeps what was written to the root
  filesystem of the sandbox in this and in former calls is deleted.
  The next call starts from the unmodified rootfs. The directories
  former versions kept the written data in on disk are deleted as
  well. Configured as pilot option of the flake, every call starts
  from the unmodified rootfs

%tmpfs_size:SIZE

  Limit the tmpfs which keeps the writable layer of the instance to
  the given size: a number of bytes with an optional k, m, g or t
  suffix, e.g 512M, or a percentage of the memory of the host, e.g
  10%. The size must not exceed the **sandbox_tmpfs_size** limit of
  `/etc/flakes.yml`, the limit is the default. A tmpfs kept from a
  former call is resized. This fails if it holds more data than the
  new size allows, the instance has to be called with **%discard**
  then

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
* /etc/flakes.yml

SEE ALSO
--------

flake-ctl-bubblewrap-register(8), flake-ctl-bubblewrap-remove(8), flake-ctl-bubblewrap-show(8), bwrap(1)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
