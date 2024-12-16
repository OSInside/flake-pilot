PODMAN-PILOT(8)
===============

NAME
----

**podman-pilot** - Launcher for flake applications

DESCRIPTION
-----------

A flake application is an application which gets called through
a runtime engine. podman-pilot supports OCI containers
called through the podman container engine.

podman-pilot provides the application launcher binary and is not expected
to be called by users. Instead it is being used as the symlink target
at the time an application is registered via **flake-ctl podman register**.

This means podman-pilot is the actual binary called with any application
registration. If the registered application is requested as
`/usr/bin/myapp` there will be a symlink pointing to:

.. code:: bash

   /usr/bin/myapp -> /usr/bin/podman-pilot

Consequently calling **myapp** will effectively call **podman-pilot**.
podman-pilot now reads the calling program basename, which is **myapp**
and looks up all the registration metadata stored in
`/usr/share/flakes`

Below `/usr/share/flakes` each application is registered
with the following layout:

.. code:: bash

   /usr/share/flakes/
       ├── myapp.d
       │   └── other.yaml
       └── myapp.yaml

All metadata information read by **podman-pilot** uses the YAML
markup. The main configuration `myapp.yaml` is read first
and can be optionally extended with further `*.yaml` files
below the `myapp.d` directory. All files in the
`myapp.d` directory will be read in alpha sort order.
Redundant information will always overwrite the former one.
Thus the last setting in the sequence wins.

From a content perspective the following registration parameters
can be set for the supported container engine:

.. code:: yaml

   container:
     # Mandatory registration setup
     # Name of the container in the local registry
     name: name

     # Path of the program to call inside of the container (target)
     target_app_path: path/to/program/in/container

     # Path of the program to register on the host
     host_app_path: path/to/program/on/host

     # Optional base container to use with a delta 'container: name'
     # If specified the given 'container: name' is expected to be
     # an overlay for the specified base_container. podman-pilot
     # combines the 'container: name' with the base_container into
     # one overlay and starts the result as a container instance
     #
     # Default: not_specified
     base_container: name

     # Optional additional container layers on top of the
     # specified base container
     layers:
       - name_A
       - name_B

     # Optional registration setup
     # Container runtime parameters
     runtime:
       # Run the container engine as a user other than the
       # default target user root. The user may be either
       # a user name or a numeric user-ID (UID) prefixed
       # with the ‘#’ character (e.g. #0 for UID 0). The call
       # of the container engine is performed by sudo.
       # The behavior of sudo can be controlled via the
       # file /etc/sudoers
       runas: root

       # Resume the container from previous execution.
       # If the container is still running, the app will be
       # executed inside of this container instance.
       #
       # Default: false
       resume: true|false

       # Attach to the container if still running, rather than
       # executing the app again. Only makes sense for interactive
       # sessions like a shell running as app in the container.
       #
       # Default: false
       attach: true|false

       # Caller arguments for the podman engine in the format:
       # - PODMAN_OPTION_NAME_AND_OPTIONAL_VALUE
       # For details on podman options please consult the
       # podman documentation.
       # Example:
       podman:
         - --storage-opt size=10G
         - --rm
         - -ti

After reading of the app configuration information the application
will be called using the configured engine. If no runtime
arguments exists, the following defaults will apply:

- The instance will be removed after the call
- The instance allows for interactive shell sessions

All caller arguments will be passed to the program call inside
of the instance except for arguments that starts with the '@'
or '%' sign. Caller arguments of this type are only used for
the podman-pilot startup itself. See the OPTIONS section
for the available runtime options.

OPTIONS
-------

@NAME

  This allows users to distribute the exact same program call to different
  instances when using a non resume based flake setup.

%silent

  This stops the progress spinner to be displayed

%ignore_sync_error

  When provisioning a container with systemfiles, the default action is
  to stop with an error when not all of the system files could be transfered
  from the host to the container instance. This option allows to continue
  even if there are files missing. This can lead to a non functional
  instance of course, you have been warned.

%interactive

  Force interactive call style for processes like a shell.
  Usually the pilot automatically detects if called in a
  terminal or not. This options allows to override the
  detection.

DEBUGGING
---------

podman-pilot provides more inner works details if the following
environment variable is set:

.. code:: bash

   export PILOT_DEBUG=1

FILES
-----

* /usr/share/flakes
* /etc/flakes

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
