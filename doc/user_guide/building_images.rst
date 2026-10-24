.. _building-images:

=================================
How To Build Your Own App Images
=================================

.. hint:: **Abstract**

   This chapter explains how the container and VM images the flakes
   of this guide are based on are built, and how to build your own.

The container and VM images used in this guide are built with
`KIWI <https://github.com/OSInside/kiwi>`__. Every image is described
by a KIWI image description, and the descriptions for all images used
in this guide are part of the appstore in the Flake Pilot repository:

* https://github.com/OSInside/flake-pilot/tree/main/appstore/podman
  for container images, published at
  https://gallery.ecr.aws/b9k1j9y6?page=1

* https://github.com/OSInside/flake-pilot/tree/main/appstore/firecracker
  for VM images, published at https://ddrasqgvrmpt8.cloudfront.net

They can be used as they are, or as a starting point for your own
images.

KIWI is also the tool of choice for the two image types which are
specific to Flake Pilot:

* **Delta containers**, which carry only the difference to a base
  container and are used with the ``--base`` option of
  ``flake-ctl podman register``, see :ref:`building-delta-containers`
  and :ref:`container-example-delta`.

* **KIS images**, the kernel/initrd/rootfs archives pulled with
  ``flake-ctl firecracker pull --kis-image``, see :ref:`vm-apps`.

.. _building-images-boxbuild:

Building With Podman Only
=========================

KIWI does not need to be installed on the build host. Instead the
build runs KIWI from a container, and inside the container
``kiwi system boxbuild`` is called. The boxbuild command starts a
small virtual machine, the box, and runs the actual image build
inside of it. Because the build happens in the box and not on the
host, the only requirement for the build host is podman.

Each image description in the appstore comes with a build script
named after the image. For example, the ``claude`` container image
is built by ``claude.sh``:

.. code-block:: bash

   #!/bin/bash

   mkdir -p image $HOME/.kiwi_boxes

   podman run \
       --privileged \
       --pull=newer \
       -v $HOME/.kiwi_boxes:/root/.kiwi_boxes \
       -v $PWD:/claude.kiwi \
       -v $PWD/image:/claude.oci \
       --rm \
       -it public.ecr.aws/b9k1j9y6/kiwi:latest \
       system boxbuild \
       --box tumbleweed \
       --box-smp-cpus 2 \
       --box-memory 2048 \
       kiwi \
       --description /claude.kiwi \
       --target-dir /claude.oci

The script mounts the image description from the current directory
into the KIWI container and the ``image`` directory as the target for
the build result. The box is downloaded to ``$HOME/.kiwi_boxes`` once
and reused by later builds. The options after ``system boxbuild``
select the box and its resources, and everything after the ``kiwi``
keyword is passed to the KIWI build that runs inside the box.

Building Container Images
-------------------------

The container images for podman are in ``appstore/podman``, grouped
in subdirectories such as ``ai`` and ``cloudsdk``. To build one of
them, change into its directory and call its build script:

.. code-block:: bash

   cd appstore/podman/ai/claude
   ./claude.sh

The result is an OCI archive in the ``image`` directory, which can be
loaded into the local container storage and then registered as a
flake as described in :ref:`container-apps`:

.. code-block:: bash

   podman load -i image/*.tar.xz

Building VM Images
------------------

The VM images for firecracker are in ``appstore/firecracker`` and
follow the same concept. Each image has a build script named after
the image, for example ``claude.sh``:

.. code-block:: bash

   cd appstore/firecracker/claude
   ./claude.sh

The image descriptions use the KIWI ``kis`` image type, so the result
in the ``image`` directory is a KIS archive containing the rootfs, the
kernel and the initrd. To use it with
``flake-ctl firecracker pull --kis-image``, publish the archive on a
web server together with a checksum file of the same name plus a
``.sha256`` suffix, see :ref:`vm-apps`. To use it on the build host
itself, create the checksum file next to the archive and pass the
path of the archive:

.. code-block:: bash

   cd image
   sha256sum claude.x86_64-1.16.0-0.tar.xz > claude.x86_64-1.16.0-0.tar.xz.sha256
   flake-ctl firecracker pull --name claude \
       --kis-image $PWD/claude.x86_64-1.16.0-0.tar.xz

.. _building-delta-containers:

Delta Containers
================

A regular container image contains the complete root tree of the
application, including all the parts it shares with other
containers. A delta container contains only the files that differ
from a given base container. The base is pulled to the host once and
shared by all delta containers built against it, so every additional
application only adds its own data.

A delta container is not functional on its own. To run it, the base
container is mounted and the delta container is layered on top of it.
Flake Pilot does this in the provision step of podman flakes that
were registered with the ``--base`` option, see
:ref:`container-example-delta`. Further layers between the base and
the delta can be added with the ``--layer`` option.

In KIWI, a delta container is a container image built from a base
container with the ``delta_root`` attribute enabled:

.. code-block:: xml

   <type image="oci"
         derived_from="docker-archive:/path/to/basesystem.tar"
         delta_root="true">
       <containerconfig name="joe"/>
   </type>

The ``derived_from`` attribute sets the base container, here given as
a docker archive type. This is the base the delta container must be used
with later. The ``derived_from`` value is passed along to ``skopeo copy``
as such any skopeo supported source-image format/location can be used
to reference the base container.
With ``delta_root="true"`` KIWI puts only the differences to that base into
the resulting image. A ``config-overlay.sh`` script, if present in
the image description, runs at the end of the build and is the last
point at which the delta root tree can be changed.

Example delta containers built against a common basesystem container,
including the ``joe`` editor used in :ref:`container-example-delta`,
can be found here:

* https://build.opensuse.org/project/show/home:marcus.schaefer:delta_containers
