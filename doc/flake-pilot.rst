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
- man bubblewrap-pilot

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

       # Metadata directory for the bubblewrap-pilot to store
       # sandbox PID files from the sandbox instances
       # started through the bubblewrap-pilot
       bubblewrap_ids_dir: /tmp/flakes

       # The metadata directories are shared by all users, each
       # of them keeps a private directory below. They have to
       # belong to root and to have the permissions 1777, like
       # /tmp. The pilots do not create them and refuse to run
       # without them. /tmp/flakes is created at boot through
       # /usr/lib/tmpfiles.d/flake-pilot.conf, a different
       # directory by calling 'flake-ctl init' as root

       # Path to the podman storage configuration file
       # this information is used by the podman-pilot to
       # launch containers with a custom storage setup
       # As a user run "export CONTAINERS_STORAGE_CONF=/etc/flakes/storage.conf"
       # to allow podman commands to show information for this
       # flake storage location.
       podman_storage_conf: /etc/flakes/storage.conf

       # Size limit of the tmpfs which keeps what is written in
       # a bubblewrap sandbox while it runs, in the notation of
       # the tmpfs size option, e.g 2G or 25% of the memory. It
       # is the default size of the tmpfs and the maximum a flake
       # may request with the %tmpfs_size pilot option, applied
       # by bubblewrap-pilot or, for a sandbox it mounts, by the
       # helper. Only read from /etc/flakes.yml.
       # Without it the tmpfs may use up to half of the memory of
       # the host, see bubblewrap-pilot
       sandbox_tmpfs_size: 2G

       # Container which provides kiwi to build KIS images
       # through a kiwi system boxbuild, used by
       # 'flake-ctl firecracker build'. Only read from
       # /etc/flakes.yml. Without it the container
       # public.ecr.aws/b9k1j9y6/kiwi:latest is used
       kiwi_container: public.ecr.aws/b9k1j9y6/kiwi:latest

  The user specific configuration for running flake applications
  is created by calling the following command as that user:

  .. code:: sh

     flake-ctl init

  For details see:

  - man flake-ctl-init

SEE ALSO
--------

flake-ctl(8), flake-ctl-init(8), podman-pilot(8), firecracker-pilot(8), bubblewrap-pilot(8)

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2023, SUSE Software Solutions Germany GmbH
