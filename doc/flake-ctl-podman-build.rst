FLAKE-CTL-PODMAN-BUILD(8)
=========================

NAME
----

**flake-ctl podman build** - Build container image

SYNOPSIS
--------

.. code:: bash

    USAGE:
        flake-ctl podman build [OPTIONS] --kiwi-image <KIWI_IMAGE> --outfile <OUTFILE>

    OPTIONS:
        --9p-sharing
        --aarch64
        --box <NAME>
        --box-console <TTYNAME>
        --box-debug
        --box-memory <VMGB>
        --box-smp-cpus <NUMBER>
        --container
        --cpu <QEMU_CPU>
        --kiwi-image <KIWI_IMAGE>
        --kiwi-version <VERSION>
        --machine <QEMU_MACHINE>
        --no-accel
        --no-snapshot
        --no-update-check
        --outfile <OUTFILE>
        --shared-path <HOST_PATH[:BOX_PATH]>
        --ssh-key <NAME>
        --ssh-port <PORT>
        --sshfs-sharing
        --virtiofs-sharing
        --x86_64

DESCRIPTION
-----------

Build the KIWI image description given by `--kiwi-image` as container
image and store the resulting archive as `--outfile`. The image
description has to build the KIWI `docker` or `oci` image type. This
includes delta containers, see the flake-pilot user guide.

The build runs `kiwi system boxbuild` in an instance of the kiwi
container, which is started through podman. The only requirement for
the build host is therefore podman. The box, a small virtual machine
the actual image build runs in, is downloaded on first use and cached
in `~/.kiwi_boxes` of the calling user. This is equivalent to:

.. code:: bash

   podman run --privileged --pull=newer --rm -it \
       -v ~/.kiwi_boxes:/root/.kiwi_boxes \
       -v KIWI_IMAGE:/description \
       -v BUILD_DIR:/result \
       public.ecr.aws/b9k1j9y6/kiwi:latest \
       system boxbuild --box tumbleweed \
       kiwi --description /description --target-dir /result

The build directory is a temporary directory below `/var/tmp`, which
is deleted after the build. From the build result only the container
archive is kept. A build in the box VM provides the result bundle created by
`kiwi-ng result bundle` in there, a build with `--container` the
build result itself. A checksum file kiwi has created for the archive
is used to verify it. The archive is moved to `--outfile` and a
checksum file named like `--outfile` plus a `.sha256` suffix is
created next to it. The log of the build is kept next to it as well,
named like `--outfile` plus a `.log` suffix. The log is also kept if
the build fails. The archive can be loaded into the local container
registry as is, see **flake-ctl-podman-load**\(8):

.. code:: bash

   $ flake-ctl podman load --oci OUTFILE

The container to use is configured by the `kiwi_container` setting
in `/etc/flakes.yml` and defaults to
`public.ecr.aws/b9k1j9y6/kiwi:latest`, see **flake-pilot**\(8).

OPTIONS
-------

--kiwi-image <KIWI_IMAGE>

  Path to the KIWI image description to build. The description has
  to build the docker or oci image type

--outfile <OUTFILE>

  Path name of the container archive to create. The directory to create it
  in has to exist. An existing file of that name is replaced. A
  checksum file named like OUTFILE plus a '.sha256' suffix and the
  build log named like OUTFILE plus a '.log' suffix are created
  next to it

The following options are passed along to `kiwi system boxbuild`,
see the kiwi documentation for details:

--box <NAME>

  Name of the box to use for the build process. By default the
  tumbleweed box is used

--box-memory <VMGB>

  Number of GBs to reserve as main memory for the virtual machine.
  By default 8GB will be used

--box-console <TTYNAME>

  Name of console in the kernel settings for the virtual machine.
  By default set to hvc0

--box-smp-cpus <NUMBER>

  Number of CPUs to use in the SMP setup. By default 4 CPUs will
  be used

--box-debug

  In debug mode the started virtual machine will be kept open

--container

  Build in container instead of a VM. Options related to building
  in a VM will have no effect. Building a container image this way
  means to run buildah in a container, which is known to cause
  issues. The VM based build is recommended

--kiwi-version <VERSION>

  Specify a KIWI version to use for the build. The referenced KIWI
  will be fetched from pip and replaces the box installed KIWI
  version

--shared-path <HOST_PATH[:BOX_PATH]>

  Optional host path to share with the box. The same path as it is
  present on the host will also be available inside of the box
  during build time. With HOST_PATH:BOX_PATH the host path is
  available at BOX_PATH inside of the box instead. This allows an
  image description to reference data outside of its directory at
  a fixed location, e.g through a relative symlink which resolves
  to BOX_PATH from the location of the description in the box,
  `/description`. BOX_PATH has to be an absolute path and must not
  interfere with `/description`, `/result` and `/root/.kiwi_boxes`,
  which are used by the build. HOST_PATH must not contain ':'.
  The host path is mounted to BOX_PATH, or to the same location,
  in the kiwi container, from where kiwi shares it with the box

--no-update-check

  Skip check for available box update. The option has no effect if
  the selected box does not yet exist

--no-snapshot

  Run box with snapshot mode switched off. This causes the box disk
  file to be modified by the build process and allows to keep a
  persistent package cache as part of the box

--no-accel

  Run box without hardware acceleration. By default KVM acceleration
  is activated

--9p-sharing, --virtiofs-sharing, --sshfs-sharing

  Select the backend to use for sharing data between the host and
  the box. Only one of them can be selected

--ssh-key <NAME>

  Name of ssh key to authorize for connection. By default id_rsa is
  used

--ssh-port <PORT>

  Port number to use to forward the guest SSH port to the host.
  By default 10022 is used

--x86_64, --aarch64

  Select box for the given architecture. If no architecture is
  selected the host architecture is used for selecting the box.
  The selected box architecture also specifies the target
  architecture for the image build with that box

--machine <QEMU_MACHINE>

  Machine name used by QEMU. By default no specific value is used
  here and qemu selects its default machine type. For cross arch
  builds it is required to specify a machine name

--cpu <QEMU_CPU>

  CPU type used by QEMU. By default the host CPU type is used. On
  cross arch builds it is required to specify the CPU emulation the
  box should use

EXAMPLE
-------

.. code:: bash

   $ flake-ctl podman build \
       --kiwi-image appstore/podman/ai/claude \
       --outfile /var/tmp/claude.docker.tar

   $ flake-ctl podman load --oci /var/tmp/claude.docker.tar

The aws image description links its `release` directory to
`../.sle16/release`, which is `/.sle16/release` in the box:

.. code:: bash

   $ flake-ctl podman build \
       --kiwi-image appstore/podman/cloudsdk/aws \
       --outfile /var/tmp/aws.docker.tar \
       --shared-path appstore/podman/cloudsdk/.sle16:/.sle16

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2026, Marcus Schäfer
