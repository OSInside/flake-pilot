FLAKE-CTL-PODMAN-SHOW(8)
========================

NAME
----

**flake-ctl podman show** - Show container instances

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl podman [--user] show [OPTIONS]

   OPTIONS:
       --format <FORMAT>
       --help

DESCRIPTION
-----------

Show the container instances created from flake applications.

For each container instance podman-pilot writes a meta data file
named after the flake application. The files are stored below
`/tmp/flakes` in a private directory of the user the instance
belongs to, e.g `/tmp/flakes/1000/myapp.cid`. The command reads
these files and combines them with the information from the
respective flake configuration file.

By default the information is rendered as a table with a headline
which is meant to be read by humans. For processing it in scripts
and other programs the machine readable formats ``json`` and
``csv`` are provided.

The following information is shown for each instance:

name
  Name of the flake application the instance was created from.
  An instance started with the pilot option **@NAME** is shown
  with that name as a suffix

user
  Name of the user the instance belongs to. Instances of other
  users are only shown if their meta data directory can be read

id
  Container ID as it was written by podman. In the table format
  the ID is shown abbreviated to its first 12 characters like
  **podman ps** does

status
  ``running`` if podman reports the container as running,
  ``stopped`` if it does not. The status is ``unknown`` if the
  container cannot be looked up. This is the case for an
  instance without a flake configuration and for a rootless
  instance of another user, because such a container lives in
  the podman storage of that user

image
  Name of the container the instance was created from

config
  Path of the flake configuration file. The system wide
  registration is preferred over the registration in the
  flakes directory of the user the instance belongs to

Information which cannot be read is shown as ``-`` in the table
format, as ``null`` in the json format and as an empty field in
the csv format.

Please note, a meta data file also exists for an instance which
is no longer running. The pilots delete them when they run the
flake application again. Therefore the command can also show
instances in the ``stopped`` status.

OPTIONS
-------

--format <FORMAT>

  Format of the output. Supported formats are:

  * ``table``: human readable table with a headline. Default
  * ``json``: machine readable list of instance records
  * ``csv``: machine readable comma separated values without
    a header line

--user

  Show the instances of the user specific, rootless setup.
  Requesting user mode as the root user has no effect

FILES
-----

* /tmp/flakes
* /usr/share/flakes
* $HOME/.config/flakes
* /etc/flakes.yml

EXAMPLE
-------

.. code:: bash

   $ flake-ctl podman show

   $ flake-ctl podman show --format json

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
