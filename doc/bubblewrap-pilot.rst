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

All metadata information read by **bubblewrap-pilot** uses the YAML
markup. The main configuration `myapp.yaml` is read first
and can be optionally extended with further `*.yaml` files
below the `myapp.d` directory. All files in the
`myapp.d` directory will be read in alpha sort order.
Redundant information will always overwrite the former one.
Thus the last setting in the sequence wins.

From a content perspective the following registration parameters
can be set for the sandbox engine:

.. code:: yaml

   sandbox:
     # Mandatory registration setup
     # Path of the directory tree on the host which becomes the
     # root filesystem of the sandbox. The tree is the read only
     # layer of an overlay, everything the application writes to
     # its root filesystem is kept in memory and is gone when the
     # application terminates
     name: path/to/rootfs/on/host

     # Path of the program to call inside of the sandbox (target)
     target_app_path: path/to/program/in/sandbox

     # Path of the program to register on the host
     host_app_path: path/to/program/on/host

     # Optional registration setup
     # Sandbox runtime parameters
     runtime:
       # Create the sandbox for a user other than the calling
       # one. The call of bwrap is performed by sudo in this
       # case. The behavior of sudo can be controlled via the
       # file /etc/sudoers. The value 'any' as well as no
       # value at all refers to the calling user. bwrap needs
       # no privileges to create the sandbox
       #
       # Default: any
       runas: any

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
overlay filesystem on the host. The rootfs is the lower, read only
layer of that overlay, its upper and work directory live in a
tmpfs which is created for the instance. The directories of the
setup are created below `/var/tmp`, in a directory which is named
after the ID of the calling user. Thus two users running the same
flake do not use the same paths. For an instance named `myapp@one`
of the user with the ID 1000 the following setup is used:

.. code:: bash

   /var/tmp/bwrap_1000/
       ├── myapp@one_merged   <- overlay mount of the rootfs
       ├── myapp@one_overlay  <- tmpfs with the upper/work dirs
       ├── myapp@one_rw       <- upper dir of the sandbox root
       └── myapp@one_work     <- work dir of the sandbox root

Creating and deleting these mounts is not allowed for a standard
user. A caller which is not root passes the calls to sudo, the
behavior of sudo can be controlled via the file /etc/sudoers.

The merged directory of the overlay becomes the root filesystem
of the sandbox. It is provided to bwrap as the read only source of
another overlay which makes the root writable inside of the
sandbox. The corresponding options are always the first ones of
the sandbox:

.. code:: bash

   --overlay-src %OVERLAYROOT
   --overlay /var/tmp/bwrap_1000/myapp@one_rw \
             /var/tmp/bwrap_1000/myapp@one_work /

The variable **%OVERLAYROOT** resolves to the merged directory of
the instance, `/var/tmp/bwrap_1000/myapp@one_merged` in the above
example. It can also be referenced in the bwrap runtime arguments
of the flake to access the root filesystem of the sandbox as it
exists on the host.

A flake can provide further directories for the root of the
sandbox by configuring **--overlay-src** options of its own. As
bwrap reads the sources of an overlay before the mount they belong
to, these options are moved in front of the **--overlay** option
above, in the order they are configured. They are therefore
stacked on top of the rootfs, the last one of them wins for a path
which exists in more than one source. All other options, the ones
from the flake configuration as well as the default ones, are
added after the **--overlay** option. For the configuration:

.. code:: yaml

   bubblewrap:
     - --overlay-src /data/tools
     - --unshare-pid

the sandbox is created with:

.. code:: bash

   --overlay-src /var/tmp/bwrap_1000/myapp@one_merged
   --overlay-src /data/tools
   --overlay /var/tmp/bwrap_1000/myapp@one_rw \
             /var/tmp/bwrap_1000/myapp@one_work /
   --unshare-pid

When the application has terminated the overlay mount and the
tmpfs below it are deleted.

If no bwrap runtime arguments exists, the following defaults will
apply:

.. code:: bash

   --dev /dev --proc /proc --tmpfs /tmp --unshare-pid --die-with-parent

This provides the standard pseudo filesystems, a writable `/tmp`,
a private process ID namespace and the termination of the sandbox
together with the pilot. Configuring runtime arguments replaces
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
