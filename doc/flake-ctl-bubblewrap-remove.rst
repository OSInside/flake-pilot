FLAKE-CTL-BUBBLEWRAP-REMOVE(8)
==============================

NAME
----

**flake-ctl bubblewrap remove** - Remove application registration

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl bubblewrap remove [OPTIONS] --app <APP>

   OPTIONS:
       --app <APP>
       --force

DESCRIPTION
-----------

Remove the registration of the given sandbox application. This
deletes the application symlink on the host as well as the flake
configuration file and the configuration directory of the
application.

The root filesystem tree the application was registered with is
provided by the caller and is never deleted by this command. A
tree which is no longer used can be deleted like any other
directory on the host.

A registration which is still in use is kept. This is the case if
one of its instances is still running, the instance would stay
behind without the configuration it was created from. The running
instances can be listed with **flake-ctl bubblewrap show**.

Called as a user other than root the registration is removed from
the flake registry of that user.

OPTIONS
-------

--app <APP>

  Application absolute path to be removed from host

--force

  Force removing the registration, does not raise an
  error if no registration exists. Do not apply
  the check for a flake registered app and remove
  when present. Use with care !

FILES
-----

* /usr/share/flakes
* $HOME/.config/flakes
* /etc/flakes.yml

EXAMPLE
-------

.. code:: bash

   $ flake-ctl bubblewrap remove --app /usr/bin/apt-get

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
