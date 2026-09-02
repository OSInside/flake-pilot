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
`/usr/bin/myapp` there will be a symlink pointing to:

.. code:: bash

   /usr/bin/myapp -> /usr/bin/firecracker-pilot

Consequently calling **myapp** will effectively call **firecracker-pilot**.
firecracker-pilot now reads the calling program basename, which is **myapp**
and looks up all the registration metadata stored in
`/usr/share/flakes`. If there is no system wide registration
for the given application name, the registration directory of
the calling user, `~/.config/flakes`, is used as created by
**flake-ctl firecracker register** called as that user

Below `/usr/share/flakes` each application is registered
with the following layout:

.. code:: bash

   /usr/share/flakes/
       ├── myapp.d
       │   └── other.yaml
       └── myapp.yaml

All metadata information read by **firecracker-pilot** uses the YAML
markup. The main configuration `myapp.yaml` is read first
and can be optionally extended with further `*.yaml` files
below the `myapp.d` directory. All files in the
`myapp.d` directory will be read in alpha sort order.
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

        # Resume the VM from previous execution.
        # If the VM is still running, the app will be
        # executed inside of this VM instance.
        #
        # Default: false
        resume: true|false

        # Optional pilot options in the format:
        # - %name or %name:value
        # Pilot options are not passed to the application call
        # but control the behavior of firecracker-pilot. An
        # option configured here is always effective and does
        # not have to be given at call time. An option of the
        # same name provided at call time takes precedence over
        # the configured one. As the '%' character is reserved
        # in YAML the option has to be quoted. For the list of
        # available options see the OPTIONS section
        # Example:
        pilot_options:
          - "%port:2000"

        firecracker:
          # Currently fixed settings through app registration
          boot_args:
            - "init=/usr/sbin/sci"
            - "console=ttyS0"
            - "root=/dev/vda"
            - "acpi=off"
            - "quiet"
            # Optional NFS volume(s) to mount in the VM before the
            # app is called, given as a comma separated list of
            # NAME_OR_IP:/export_path:/mount_path elements. A mount
            # point which does not exist is created. This requires
            # a network setup for the VM and the NFS client tools
            # in the VM image. See sci(8) for details
            # Example:
            - "nfs=some.host:/host/path:/local/path"
          mem_size_mib: 4096
          vcpu_count: 2
          cache_type: Writeback

          # Size of the VM overlay
          # If specified a new ext2 overlay filesystem image of the
          # specified size will be created and attached to the VM
          overlay_size: 20GiB

          # Path to rootfs image done by app registration
          rootfs_image_path: /var/lib/firecracker/images/NAME/rootfs

          # Path to kernel image done by app registration
          kernel_image_path: /var/lib/firecracker/images/NAME/kernel

          # Optional path to initrd image done by app registration
          initrd_path: /var/lib/firecracker/images/NAME/initrd

          # Optional instance specific settings, keyed by the
          # @NAME selector used to call the app. The '@' character
          # is reserved in YAML, thus the key has to be quoted. For
          # convenience the plain name without the '@' prefix is
          # accepted as a key as well
          instance:
            "@id":
              # Boot arguments effective for this instance only.
              # They are merged into the boot_args above: an option
              # which is also set here takes the place of the global
              # setting of the same option, options which are not
              # set globally are appended
              boot_args:
                - "ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off"

After reading of the app configuration information the application
will be called using the configured engine. If no runtime
arguments exists, the following defaults will apply:

- The instance will be removed after the call

All caller arguments will be passed to the program call inside
of the instance except for arguments that starts with the '@'
or '%' sign. Caller arguments of this type are only used for
the firecracker-pilot startup itself. See the OPTIONS section
for the available runtime options.

All options listed in the OPTIONS section, except for @NAME,
can also be set permanently for an application through the
**pilot_options** setting of the flake configuration. Such an
option is always effective and does not have to be given at
call time. Passing the option at call time takes precedence
over the configured value.

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

OPTIONS
-------

@NAME

  This allows users to distribute the exact same program call to different
  instances when using a non resume based flake setup.

%port:number

  This allows to specify a static port assignment for the communication
  between guest and host in a resume based flake setup. By default
  firecracker-pilot calculates a port number itself.

%progress

  Show a progress spinner as long as the pilot provisions

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
* ~/.config/flakes
* ~/.config/flakes/firecracker/images
* ~/.config/flakes/firecracker/storage

AUTHOR
------

Marcus Schäfer

COPYRIGHT
---------

(c) 2022, Elektrobit Automotive GmbH
(c) 2023, Marcus Schäfer
