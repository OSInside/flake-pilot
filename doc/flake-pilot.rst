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

       # Path to the podman storage configuration file
       # this information is used by the podman-pilot to
       # launch containers with a custom storage setup
       # As a user run "export CONTAINERS_STORAGE_CONF=/etc/flakes/storage.conf"
       # to allow podman commands to show information for this
       # flake storage location.
       podman_storage_conf: /etc/flakes/storage.conf

  To setup a user specific configuration for running flake applications
  perform the following steps:

  1. Create the directory $HOME/.config if not already present

     .. code:: sh

        mkdir -p $HOME/.config

  2. Create a copy of the system wide configuration file /etc/flakes.yml
     to $HOME/.config/flakes.yml and adjust the configuration as desired.
     For example:

      .. code:: yaml

         generic:
           flakes_dir: /home/USERNAME/.config/flakes
           podman_storage_conf: /home/USERNAME/.config/flakes/storage.conf
           podman_ids_dir: /tmp/flakes
           firecracker_ids_dir: /tmp/flakes

  3. Create a copy of the system wide flake storage configuration file
     /etc/flakes/storage.conf to $HOME/.config/flakes/storage.conf
     and adjust the storage configuration as desired. For example:

      .. code:: ini

         [storage]
         driver = "overlay"
         graphroot = "/home/USERNAME/.config/flakes/storage"
         runroot= "/home/USERNAME/.config/flakes/storage/runroot"
         rootless_storage_path = "/home/USERNAME/.config/flakes/storage"

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2023, SUSE Software Solutions Germany GmbH
