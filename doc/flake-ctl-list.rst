FLAKE-CTL-LIST(8)
=================

NAME
----

**flake-ctl list** - List registered flake applications

SYNOPSIS
--------

.. code:: bash

   USAGE:
       flake-ctl list [OPTIONS]

   OPTIONS:
       --format <FORMAT>
       --help


DESCRIPTION
-----------

List registered flake applications.

The registry to read is detected from the caller. Called as the
root user the system wide registry is listed. Called as any other
user the flake registry of that user is listed.

By default the list is rendered as a table with a headline
which is meant to be read by humans. For processing the list
in scripts and other programs the machine readable formats
``json`` and ``csv`` are provided.

The following information is listed for each flake:

name
  Name of the flake application

engine
  Engine used to run the flake application, either
  ``podman`` or ``firecracker``

target_app_path
  Path of the application as it is called on the engine

host_app_path
  Path of the application as it is called on the host

config
  Path of the flake configuration file.

Information which cannot be read from the flake configuration
is shown as ``-`` in the table format, as ``null`` in the
json format and as an empty field in the csv format.

OPTIONS
-------

--format <FORMAT>

  Format of the output. Supported formats are:

  * ``table``: human readable table with a headline. Default
  * ``json``: machine readable list of flake records
  * ``csv``: machine readable comma separated values without
    a header line

FILES
-----

* /usr/share/flakes
* $HOME/.config/flakes

EXAMPLE
-------

.. code:: bash

   $ flake-ctl list

   $ flake-ctl list --format json

   $ flake-ctl list --format csv

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
