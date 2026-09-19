FLAKE-CTL-BUBBLEWRAP-REGISTER(8)
================================

NAME
----

**flake-ctl bubblewrap register** - Register sandbox application

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl bubblewrap register [OPTIONS] --rootfs <ROOTFS> --app <APP>

   OPTIONS:
       --app <APP>
       --force
       --opt <OPT>...
       --pilot-option <PILOT_OPTION>...
       --rootfs <ROOTFS>
       --run-as <RUN_AS>
       --target <TARGET>

DESCRIPTION
-----------

Register the given application to run inside of a sandbox created
from the specified root filesystem tree. The registration process
is two fold:

1. Create the application symlink pointing to `/usr/bin/bubblewrap-pilot`
2. Create the application default configuration below `/usr/share/flakes`.
   Each application registered is called a **flake**

On successful completion the registered *--app* name can be called
like a normal application on this host.

For further details about the flake configuration please refer to
the **bubblewrap-pilot** manual page.

Called as a user other than root the registration is done in the
flake registry of that user. For further details about the flake
configuration in user mode please refer to the **flake-pilot**
manual page.

NOTE
----

Unlike the other engines the sandbox engine has no image registry.
The root filesystem of the sandbox is a plain directory tree on the
host which is provided by the caller, e.g an unpacked container
image or an existing chroot environment. It is mounted as the read
only layer of an overlay and is therefore never modified by the
application. Consequently there is no image to pull and none to
delete, the registration only refers to the tree by its path.

The tree does not have to exist at registration time. If it does
not, a warning is shown, the application will fail to run until
the tree is created.

OPTIONS
-------

--app <APP>

  An absolute path to the application on the host. If not
  specified via the target option, the application will be
  called with that path inside of the sandbox

--force

  Force writing the registration even if a registration
  of the same name already exists. This is done by deleting
  an eventual existing registration prio creating the new
  registration. Please have in mind that a failed registration
  still causes an eventual existing former registration to be
  deleted in this case !

--opt <OPT>...

  Sandbox runtime option, and optional value, used to create the
  sandbox. This option can be specified multiple times. The
  options are added to the standard options of the sandbox which
  provide the pseudo filesystems, a writable /tmp, a private
  process ID namespace and the termination of the sandbox
  together with the pilot. As the root filesystem of the sandbox
  is an overlay of the given tree, this is the place to provide
  the paths on the host the application needs in addition. Data
  written to the root filesystem is not persistent, it is gone
  when the application terminates. See the example
  section for further details. An option which starts with a
  dash has to be escaped with a backslash to not be read as an
  option of the register command itself. For details on bwrap
  options please consult the **bwrap** manual page.

--pilot-option <PILOT_OPTION>...

  Pilot option, and optional value, in the format %name or
  %name:value. Pilot options are not passed to the application
  call but control the behavior of bubblewrap-pilot. An option
  registered here is always effective and does not have to be
  given at call time. Passing the option at call time takes
  precedence over the registered value. This option can be
  specified multiple times. For the list of available pilot
  options please refer to the **bubblewrap-pilot** manual page.

--rootfs <ROOTFS>

  An absolute path to the root filesystem tree on the host. The
  tree is mounted as the read only layer of an overlay which
  becomes the root filesystem of the sandbox the application
  runs in

--run-as <RUN_AS>

  Name of the user to run bubblewrap. If not specified the
  sandbox is created by the user calling the application.
  bwrap creates the sandbox through user namespaces and needs
  no privileges. Selecting another user is therefore only
  needed if the application itself requires it. The call of
  bwrap is performed by sudo in this case, the calling user
  needs the permission to do so

--target <TARGET>

  An absolute path to the application in the sandbox. Use this
  option if the application path on the host should be different
  to the application path inside of the sandbox

FILES
-----

* /usr/share/flakes
* $HOME/.config/flakes
* /etc/flakes/bubblewrap-flake.yaml
* /etc/flakes.yml

EXAMPLE
-------

.. code:: bash

   $ flake-ctl bubblewrap register --rootfs /var/lib/flakes/leap \
       --app /usr/bin/apt-get

   $ flake-ctl bubblewrap register --rootfs /var/lib/flakes/leap \
       --app /usr/bin/mybuild \
       --target /usr/bin/make \
       --opt '\--ro-bind /etc/resolv.conf /etc/resolv.conf' \
       --opt '\--bind %HOME/work /work'

   $ flake-ctl bubblewrap register --rootfs /var/lib/flakes/leap \
       --app /usr/bin/mybuild \
       --pilot-option '%chdir:/work'

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
