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

IMAGE REQUIREMENTS
------------------

The pulled images are stored as they are, flake-ctl does not change
them. The root filesystem image is therefore required to provide
what the VM needs to run a flake:

* The `sci` program at `/usr/sbin/sci`, which is started as the init
  process of the VM, see **sci**\(8).

* The `/overlayroot` directory, used by `sci` to set up the overlay
  of the VM, as well as the `/proc`, `/sys`, `/dev` and `/dev/pts`
  directories.

Installing the `flake-pilot-firecracker-guestvm-tools` package into
the root filesystem when building the image provides `sci` and the
`/overlayroot` directory. The images of the flake-pilot appstore are
built this way.

UPDATE CHECK
------------

An image pulled with the `--kis-image` option takes part in a
checksum based update check. The archive given to `--kis-image`
must be accompanied by a checksum file at the same location which
is named like the archive plus a `.sha256` suffix. For an archive
named `foo.tar.xz` the checksum file is expected at
`foo.tar.xz.sha256`. If no such file exists the pull is rejected
with an error.

This applies to a local archive in the same way, the checksum file
is then expected next to it on the local machine.

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
  KIS_IMAGE is either a URL or the location of an archive on
  the local machine, given as path, e.g ``image.tar.xz`` or
  ``/path/to/image.tar.xz``, or as ``file://`` URI. A local
  archive is copied instead of downloaded.
  A checksum file named like KIS_IMAGE plus a '.sha256' suffix
  must be available at the same location, see UPDATE CHECK

--name <NAME>

  Image name used as local identifier. The name is the directory
  of the image in the registry. It has to start with a letter or
  digit and may only contain letters, digits, ``.``, ``_`` and
  ``-``, up to 255 characters

--rootfs <ROOTFS>

  Single rootfs image to pull into local image store. The image
  has to meet the IMAGE REQUIREMENTS

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
       https://ddrasqgvrmpt8.cloudfront.net/leap.x86_64-1.16.0-0.tar.xz

   $ flake-ctl firecracker pull --name myLocalImage --kis-image \
       $PWD/leap.x86_64-1.16.0-0.tar.xz

   $ flake-ctl firecracker pull --name myVM \
       --rootfs https://example.org/images/myvm-rootfs.ext4 \
       --kernel https://example.org/images/myvm-vmlinux \
       --initrd https://example.org/images/myvm-initrd

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
