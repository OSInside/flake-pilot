FLAKE-PILOT(8)
==============

DESCRIPTION
-----------

flake-pilot is a software to register, provision and launch
applications that are actually provided inside of a runtime
environment like an OCI container or a FireCracker VM. Along
with the project a collection of application launchers is
provided which are called `pilots`. For details on the different
pilots see:

- man podman-pilot
- man firecracker-pilot

The flake registration tool `flake-ctl` is the management utility
to list, register, remove, and-more... flake applications
on your host. For details about flake-ctl see:

- man flake-ctl

FILES
-----

- /etc/flakes.yml

  System wide configuration file. By default loaded or when
  the calling user is root. If the calling user is a normal user
  the file $HOME/.config/flakes.yml is loaded when present.

  .. code:: yaml

     generic:
       # Directory to store flake registrations
       flakes_dir: /usr/share/flakes

       # Metadata directory for the podman-pilot to store
       # container ID files from the container instances
       # started through the podman-pilot
       podman_ids_dir: /tmp/flakes

       # Metadata directory for the firecracker-pilot to store
       # virtual machine PID files from the firecracker(VM) instances
       # started through the firecracker-pilot
       firecracker_ids_dir: /tmp/flakes

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2023, SUSE Software Solutions Germany GmbH
