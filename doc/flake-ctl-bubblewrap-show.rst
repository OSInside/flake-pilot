FLAKE-CTL-BUBBLEWRAP-SHOW(8)
============================

NAME
----

**flake-ctl bubblewrap show** - Show sandbox instances

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl bubblewrap show [OPTIONS]

   OPTIONS:
       --format <FORMAT>
       --help

DESCRIPTION
-----------

Show the sandbox instances created from flake applications.

The setup to read the flake configurations from is detected based on
the caller. Called as any user other than root the user specific,
rootless setup of that user is used.

For each sandbox instance bubblewrap-pilot writes a meta data file
named after the flake application. The files are stored below
`/tmp/flakes` in a private directory of the user the instance
belongs to, e.g `/tmp/flakes/1000/myapp.bwrapid`. The command reads
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
  Process ID of the sandbox. An instance which was created but
  is not started yet is recorded with a process ID of zero

status
  ``running`` if the process of the sandbox still exists,
  ``stopped`` if it does not

image
  Path of the root filesystem tree on the host the sandbox was
  created from

config
  Path of the flake configuration file. The system wide
  registration is preferred over the registration in the
  flakes directory of the user the instance belongs to

Information which cannot be read is shown as ``-`` in the table
format, as ``null`` in the json format and as an empty field in
the csv format.

A sandbox exists as long as the application running in it. The
meta data file is deleted when the application terminates. A file
which was left behind, e.g because the pilot was killed, is
garbage collected the next time the flake application runs.
Therefore the command can also show instances in the ``stopped``
status.

OPTIONS
-------

--format <FORMAT>

  Format of the output. Supported formats are:

  * ``table``: human readable table with a headline. Default
  * ``json``: machine readable list of instance records
  * ``csv``: machine readable comma separated values without
    a header line

FILES
-----

* /tmp/flakes
* /usr/share/flakes
* $HOME/.config/flakes
* /etc/flakes.yml

EXAMPLE
-------

.. code:: bash

   $ flake-ctl bubblewrap show

   $ flake-ctl bubblewrap show --format json

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
