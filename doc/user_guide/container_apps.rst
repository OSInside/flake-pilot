.. _container-apps:

==============================
Applications From a Container
==============================

.. hint:: **Abstract**

   This chapter shows how to register applications which are
   provided by an OCI container. All examples register the app for
   the calling user and expect the setup described in
   :ref:`getting-started`.

A container flake is registered with ``flake-ctl podman register``.
The two options which are always needed are:

``--container``
   The name of the container as it is known in the local registry,
   the value of the **REPOSITORY** column of ``podman images``. An
   image which is not there yet is pulled by the registration.

``--app``
   The absolute path of the application on the host. This is the
   name the flake is called by, the path the symbolic link to the
   launcher is created at.

``--target`` names the program to call inside of the container. If it
is not given, the application is called with the same path it has on
the host. Set it to ``/`` to use the entry point the image was built
with.

.. _container-example-aws:

Amazon's SDK Utility as a Container App
=======================================

.. code-block:: bash

   flake-ctl podman register \
        --container docker.io/amazon/aws-cli --app $HOME/bin/aws --target /

   aws ec2 help

This creates ``$HOME/bin/aws`` on your host, which actually launches
the ``amazon/aws-cli`` container. The default entry point of the
container was configured by Amazon to launch their cloud API
application. Thus, the target program to call inside the container
doesn't need to be explicitly configured in the registration and is
therefore just set to ``/``. The call of ``aws ec2 help`` launches an
instance of the container via rootless podman and shows the help text
for the ``ec2`` subcommand.

.. _container-example-delta:

An Editor as a Delta Container
==============================

A delta container carries only the difference to a base container.
The base exists once on the host and is shared by all deltas built
against it, which keeps the data to pull small:

.. code-block:: bash

   flake-ctl podman register \
       --app $HOME/bin/joe \
       --container registry.opensuse.org/home/marcus.schaefer/delta_containers/containers_tw/joe \
       --base registry.opensuse.org/home/marcus.schaefer/delta_containers/containers_tw/basesystem \
       --target /usr/bin/joe

   joe

This creates ``$HOME/bin/joe`` which is a simple but nice editor. The
launch of the container requires a provision step in which the base
container gets mounted and the delta container is layered on top of
it. This action unfortunately requires root privileges and is
forwarded to the system's ``sudo`` binary.

.. note::

   Delta containers have to be built against the base they are used
   with. Such containers can be created with KIWI,
   see :ref:`building-images`. Additional layers between the base and
   the delta are registered with the ``--layer`` option, which can be
   given more than once and is evaluated in the order of the
   arguments.

.. _container-example-claude:

Claude AI as a Container App
============================

.. code-block:: bash

   mkdir -p ~/ai

   flake-ctl podman register \
       --app $HOME/bin/claude \
       --target /bin/bash \
       --container public.ecr.aws/b9k1j9y6/ai/claude:latest \
       --resume \
       --opt "\--net host" \
       --opt "\-ti" \
       --opt "\--workdir %HOME/ai" \
       --opt "\--volume %HOME/ai:%HOME/ai" \
       --opt "\-e HOME=%HOME"

   claude

This pulls the claude container from the ai space of a public ECR
which we use to offer nightly builds of the most popular AI tools.
The registered flake just starts an isolated shell if you call
``claude``. Further calls of ``claude`` will run in the same container
instance due to the ``--resume`` option. The ``ai`` directory is the
only path shared from the host with the container.

Engine Options
==============

``--opt`` passes an option to the container engine. The option can be
given more than once and is written to the flake configuration:

* The leading backslash escapes the dash of the option so that it is
  not taken as an option of ``flake-ctl`` itself.

* A word starting with ``%`` is a placeholder for the environment
  variable of that name and is resolved when the app is called, not
  when it is registered. ``%HOME`` therefore becomes the home
  directory of the user who calls the flake. A placeholder which
  cannot be resolved is turned into the plain variable name.

* If no options are specified at all, the container starts with
  terminal emulation, ``-ti``, and is removed after the call,
  ``--rm``, unless ``--resume`` or ``--attach`` is set. As soon as
  one option is given, none of these defaults apply anymore.

.. _krun-runtime:

Deeper Isolation With krun
==========================

.. note::

   For deeper isolation based on a VM you can either use the
   firecracker pilot from flake-pilot or the krun runtime with podman
   which gives isolation based on KVM and should be preferred for AI
   workloads. Please make sure to install the ``crun`` package which
   also provides krun.

The krun OCI handler however, does not support the exec command. This
limitation exists because libkrun runs workloads inside isolated
microVMs, and there is no built-in mechanism or agent inside the
lightweight virtual machine to spawn and inject new secondary
processes. Because of this a krun based app registration cannot use
the **resume** feature and looks as follows:

.. code-block:: bash

   flake-ctl podman register \
       --app $HOME/bin/claude \
       --target /bin/bash \
       --container public.ecr.aws/b9k1j9y6/ai/claude:latest \
       --opt "\--net host" \
       --opt "\--runtime=krun" \
       --opt "\-ti" \
       --opt "\--workdir %HOME/ai" \
       --opt "\--volume %HOME/ai:%HOME/ai" \
       --opt "\-e HOME=%HOME"

To switch the podman runtime in a user or system wide scope create
the file ``/etc/containers/containers.conf`` for a system wide setup
or ``$HOME/.config/containers/containers.conf`` for a user specific
setup and place the following content:

.. code-block:: ini

   [engine]
   runtime = "krun"

Calling a Registered Container App
==================================

Arguments given to the app are passed on to the program inside of the
container, with two exceptions which are read by the launcher itself:

``@NAME``
   A selector which allows to distribute the exact same program call
   to different instances, e.g ``claude @id1``. This is useful for
   flakes which are not registered with ``--resume``.

``%OPTION``
   A runtime option of the pilot, e.g ``%interactive`` to force the
   interactive call style or ``%remove`` to delete an instance which
   was kept by ``--resume`` or ``--attach``. See ``man 8
   podman-pilot`` for the complete list.

Pilot Options as Part of the Registration
=========================================

A pilot option which is always needed does not have to be typed at
every call. ``--pilot-option`` registers it as part of the flake:

.. code-block:: bash

   flake-ctl podman register \
       --app $HOME/bin/claude \
       --target /bin/bash \
       --container public.ecr.aws/b9k1j9y6/ai/claude:latest \
       --opt "\--volume %HOME/ai:%HOME/ai" \
       --pilot-option "%ignore_missing_volume_path"

The option can be given more than once and is stored in the
``pilot_options`` list of the flake configuration:

.. code-block:: yaml

   container:
     runtime:
       pilot_options:
         - "%ignore_missing_volume_path"

Registered options are read on every call. An option of the same
name given at call time takes precedence over the registered one.
