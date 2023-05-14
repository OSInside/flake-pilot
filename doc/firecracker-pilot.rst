FIRECRACKER-PILOT(8)
====================

NAME
----

**firecracker-pilot** - Launcher for flake applications

DESCRIPTION
-----------

A flake application is an application which gets called through
a runtime engine. firecracker-pilot supports virtual machine
images called through the firecracker VM engine.

firecracker-pilot provides the application launcher binary and is not expected
to be called by users. Instead it is being used as the symlink target
at the time an application is registered via **flake-ctl firecracker register**.

This means firecracker-pilot is the actual binary called with any application
registration. If the registered application is requested as
:file:`/usr/bin/myapp` there will be a symlink pointing to:

.. code:: bash

   /usr/bin/myapp -> /usr/bin/firecracker-pilot

Consequently calling **myapp** will effectively call **firecracker-pilot**.
firecracker-pilot now reads the calling program basename, which is **myapp**
and looks up all the registration metadata stored in
:file:`/usr/share/flakes`

Below :file:`/usr/share/flakes` each application is registered
with the following layout:

.. code:: bash

   /usr/share/flakes/
       ├── myapp.d
       │   └── other.yaml
       └── myapp.yaml

All metadata information read by **firecracker-pilot** uses the YAML
markup. The main configuration :file:`myapp.yaml` is read first
and can be optionally extended with further :file:`*.yaml` files
below the :file:`myapp.d` directory. All files in the
:file:`myapp.d` directory will be read in alpha sort order.
Redundant information will always overwrite the former one.
Thus the last setting in the sequence wins.

From a content perspective the following registration parameters
can be set for the firecracker engine:

.. code:: yaml

    vm:
      name: name
      target_app_path: path/to/program/in/VM
      host_app_path: path/to/program/on/host

      runtime:
        # Run the VM engine as a user other than the
        # default target user root. The user may be either
        # a user name or a numeric user-ID (UID) prefixed
        # with the ‘#’ character (e.g. #0 for UID 0). The call
        # of the VM engine is performed by sudo.
        # The behavior of sudo can be controlled via the
        # file /etc/sudoers
        runas: root

        firecracker:
          # Currently fixed settings through app registration
          boot_args:
            - "init=/usr/sbin/sci"
            - "console=ttyS0"
            - "root=/dev/vda"
            - "acpi=off"
            - "quiet"
          mem_size_mib: 4096
          vcpu_count: 2
          cache_type: Writeback

          # Size of the VM overlay
          # If specified a new ext2 overlay filesystem image of the
          # specified size will be created and attached to the VM
          overlay_size: 20g

          # Path to rootfs image done by app registration
          rootfs_image_path: /var/lib/firecracker/images/NAME/rootfs

          # Path to kernel image done by app registration
          kernel_image_path: /var/lib/firecracker/images/NAME/kernel

          # Optional path to initrd image done by app registration
          initrd_path: /var/lib/firecracker/images/NAME/initrd

After reading of the app configuration information the application
will be called using the configured engine. If no runtime
arguments exists, the following defaults will apply:

- The instance will be removed after the call

All caller arguments will be passed to the program call inside
of the instance except for arguments that starts with the '@'
sign. Caller arguments of this type are only used in the instance
ID file name but will not be passed to the program call inside of
the instance. This allows users to differentiate the same
program call between different instances when using
a resume based flake setup.

The execution of the program inside of the instance (the VM)
is managed by an extra program called `sci` and provided with
the flake-pilot project. `sci` is activated by using it as the
init process to the VM via `init=/usr/sbin/sci`. This setup is
done by the **firecracker-pilot** and users doesn't have to care.
However, users need to care that `sci` is installed in the used
rootfs image for firecracker. To support users with this task
we provide the **flake-pilot-firecracker-guestvm-tools** package
which provides among others the `sci` binary.

Creating a firecracker compatible VM image can be done in
different ways. One way is to use KIWI which supports building
firecracker compatible images. For further details checkout
the following example image which is hosted on the
**Open Build Service** which can be used as build platform
for your images:

- https://build.opensuse.org/package/show/home:marcus.schaefer:delta_containers/firecracker_base_leap_system

DEBUGGING
---------

firecracker-pilot provides more inner works details if the following
environment variable is set:

.. code:: bash

   export PILOT_DEBUG=1

FILES
-----

* /usr/share/flakes
* /var/lib/firecracker/images
* /var/lib/firecracker/storage
* /etc/flakes

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH