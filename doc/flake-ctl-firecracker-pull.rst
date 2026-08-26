FLAKE-CTL-FIRECRACKER-PULL(8)
=============================

NAME
----

**flake-ctl firecracker pull** - Fetch firecracker image

SYNOPSIS
--------

.. code:: bash

    USAGE:
        flake-ctl firecracker pull [OPTIONS] --name <NAME> <--kis-image <KIS_IMAGE>|--rootfs <ROOTFS>|--kernel <KERNEL>>

    OPTIONS:
        --force
        --initrd <INITRD>
        --kernel <KERNEL>
        --kis-image <KIS_IMAGE>
        --name <NAME>
        --rootfs <ROOTFS>

DESCRIPTION
-----------

Pull the components of a firecracker image from the given location
into `/var/lib/firecracker/images/NAME` on the local machine. The
registry to pull into is detected from the caller. Called as any
user other than root the image is stored in the user specific,
rootless registry of that user below
`~/.config/flakes/firecracker/images/NAME`.
After completion the available firecracker images can be listed via:

.. code:: bash

   $ tree /var/lib/firecracker/images

and shows a file structure like in the following example

.. code:: bash

   /var/lib/firecracker/images
   └── myImage
        ├── initrd
        ├── kernel
        └── rootfs

An image pulled with `--kis-image` carries an additional
`source_checksum` file, see UPDATE CHECK below.

UPDATE CHECK
------------

An image pulled with the `--kis-image` option takes part in a
checksum based update check. The archive given to `--kis-image`
must be accompanied by a checksum file at the same location which
is named like the archive plus a `.sha256` suffix. For an archive
named `foo.tar.xz` the checksum file is expected at
`foo.tar.xz.sha256`. If no such file exists the pull is rejected
with an error.

The checksum is used to verify the downloaded archive and it is
stored along with the image as `source_checksum`. Pulling an image
under a name that already exists in the registry then behaves as
follows:

* The checksum file is fetched and compared against the
  `source_checksum` record of the registered image. If both match,
  the image is up to date. Nothing is downloaded, nothing in the
  registry is touched and the command succeeds.

* If the checksums differ, the latest version of the image is
  fetched and replaces the image in the registry.

An image registered by a `--rootfs`/`--kernel` pull provides no
such reference and is therefore not update checked. Pulling into
an existing name stays an error for those images unless `--force`
is given.

OPTIONS
-------

--force

  Force pulling the image even if it already exists This will wipe
  existing data for the provided identifier. The image is fetched
  from scratch, no update check against an existing image of the
  same name is done

--initrd <INITRD>

  Single initrd image to pull into local image store

--kernel <KERNEL>

  Single kernel image to pull into local image store

--kis-image <KIS_IMAGE>

  Firecracker image built by KIWI as kis image type to pull
  into local image store. This means the file behind KIS_IMAGE
  is expected to be a tarball containing the KIS
  components; rootfs-image, kernel and optional initrd.
  A checksum file named like KIS_IMAGE plus a '.sha256' suffix
  must be available at the same location, see UPDATE CHECK

--name <NAME>

  Image name used as local identifier

--rootfs <ROOTFS>

  Single rootfs image to pull into local image store

ENVIRONMENT
-----------

FLAKE_ALLOW_INSECURE_TRANSPORT

  The images pulled here provide the root filesystem and the kernel
  of a virtual machine. They are therefore only fetched via https.
  If the image can only be reached through a transport that provides
  no integrity and no authenticity of the server, e.g plain http,
  setting this variable allows to use it. Only do this if the
  connection to the image source can be trusted.

EXAMPLE
-------

.. code:: bash

   $ flake-ctl firecracker pull --name myImage --kis-image \
       https://download.opensuse.org/repositories/home:/marcus.schaefer:/delta_containers/images/firecracker-basesystem.x86_64.tar.xz

   $ flake-ctl firecracker pull --name firecore \
       --rootfs https://s3.amazonaws.com/spec.ccfc.min/ci-artifacts/disks/x86_64/ubuntu-18.04.ext4 \
       --kernel https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/kernels/vmlinux.bin

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
