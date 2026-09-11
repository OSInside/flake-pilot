.. _building-images:

=================================
How To Build Your Own App Images
=================================

.. hint:: **Abstract**

   This chapter points to ways of building the container and VM
   images the flakes of this guide are based on.

Building images as container or VM images can be done in different
ways. One option is to use the **Open Build Service** with
`KIWI <https://github.com/OSInside/kiwi>`__, which is able to build
software packages and images and therefore allows maintaining the
complete application stack.

KIWI is also the tool of choice for the two image types which are
specific to Flake Pilot:

* **Delta containers**, which carry only the difference to a base
  container and are used with the ``--base`` option of
  ``flake-ctl podman register``, see :ref:`container-example-delta`.

* **KIS images**, the kernel/initrd/rootfs archives pulled with
  ``flake-ctl firecracker pull --kis-image``, see :ref:`vm-apps`.

Examples
========

For demonstration purposes and to showcase the mentioned use cases,
some example images were created and can serve as examples to build
your own images as you see fit. Please find the image descriptions
used in the context of this documentation here:

* https://build.opensuse.org/project/show/home:marcus.schaefer:delta_containers

* https://github.com/OSInside/flake-pilot/tree/main/appstore/firecracker

* https://github.com/OSInside/flake-pilot/tree/main/appstore/podman
  (https://gallery.ecr.aws/b9k1j9y6?page=1)
