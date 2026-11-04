.. _best-practices:

==============
Best Practices
==============

.. hint:: **Abstract**

   This chapter collects recommended setups for common use cases.
   Each section combines the registration and configuration steps of
   the previous chapters into one complete setup.

.. _best-practices-ai:

AI Workloads
============

AI coding tools read and write the data you work on, run commands on
their own and talk to the network. A Firecracker VM application keeps
such a tool on a kernel of its own, separated from the file system of
the host. The tool reaches only what you connect it to: the network
it needs to talk to its model provider, and one directory of the host
which holds the data you allow it to see.

The following setup registers an AI tool from one of the pre-built
images, connects it to the network, shares one directory of the host
with it and logs it in with an API key. All examples register the
application for the calling user and expect the setup described in
:ref:`getting-started`.

The Pre-Built Images
--------------------

Pre-built images are available for the following tools. Every image
comes with a user named ``ai``, home directory ``/home/ai``, which is
the user to run the tool as:

.. list-table::
   :header-rows: 1
   :widths: 15 45 40

   * - Image
     - Provides
     - API key login
   * - ``claude``
     - Claude Code
     - ``ANTHROPIC_API_KEY``
   * - ``copilot``
     - GitHub Copilot CLI and the GitHub CLI ``gh``
     - ``COPILOT_GITHUB_TOKEN``
   * - ``deepseek``
     - The ``pi`` coding agent, preconfigured for DeepSeek models
     - ``DEEPSEEK_API_KEY``
   * - ``pi``
     - The ``pi`` coding agent, no model provider preconfigured
     - The variable of the provider, e.g ``ANTHROPIC_API_KEY``,
       ``OPENAI_API_KEY``, ``GEMINI_API_KEY`` or
       ``OPENROUTER_API_KEY``

The images and their versions are listed at
https://ddrasqgvrmpt8.cloudfront.net/. An image archive is named
``NAME.ARCH-VERSION-RELEASE.tar.xz`` and is pulled from that location,
e.g ``https://ddrasqgvrmpt8.cloudfront.net/claude.x86_64-1.16.0-0.tar.xz``.
Check the listing for the current version of the image you want to
use. The examples below use the ``claude`` image; for another image
replace the image name, the application name and the archive name.

Pull the Image
--------------

.. code-block:: bash

   flake-ctl firecracker pull --name claude \
       --kis-image https://ddrasqgvrmpt8.cloudfront.net/claude.x86_64-1.16.0-0.tar.xz

The image is stored in the local firecracker registry under the name
``claude``. Pulling it again later fetches a new version only if the
checksum of the archive changed, see :ref:`vm-apps`.

Register the Application
------------------------

.. code-block:: bash

   flake-ctl firecracker register --vm claude \
       --app $HOME/bin/claude --target /bin/bash --user ai \
       --overlay-size 50GiB --force-vsock --resume

``--user ai``
   Runs the application as the ``ai`` user of the image instead of
   ``root``. The tool works in the home directory of that user and
   keeps its settings there. If the user does not exist in the VM
   the application is called as ``root``.

``--target /bin/bash``
   Calling ``claude`` on the host drops you into a shell of the
   ``ai`` user inside of the VM. From there you set up the login and
   start the tool, see below.

``--overlay-size 50GiB``
   The storage of the instance. Everything written inside of the VM,
   e.g the settings of the tool, packages it installs, caches and
   the data it creates outside of the shared directory, is stored
   there. The storage is created on the first start, kept after the
   instance is gone and reused on its next start. It is stored below
   ``~/.config/flakes/firecracker/storage`` on the host.

``--force-vsock`` and ``--resume``
   The instance keeps running after the call and further calls of
   ``claude`` are executed inside of it, which keeps the running
   sessions of the tool available. A resumed instance talks to the
   host through a vsock connection instead of the serial console.

Connect the Network
-------------------

The tool needs the network to reach its model provider:

.. code-block:: bash

   flake-ctl firecracker network init
   flake-ctl firecracker network add --app $HOME/bin/claude

``init`` prepares the host and is needed once per host, ``add``
connects the application. Both commands change the network
configuration of the host through the Flake Pilot helper, which
requires a ``sudo`` rule, see :ref:`privileges`. After a reboot of the
host both have to be called again, the application keeps its address.

A connected VM reaches everything the host reaches through its
outgoing interface. On a cloud instance, keep it away from the
metadata service of the cloud, and if the tool needs no access to
your local network, from the private networks as well:

.. code-block:: bash

   flake-ctl firecracker network init \
       --egress-deny link-local --egress-deny private

:ref:`firecracker-networking` explains the setup and the policy.

Share One Directory With the Host
---------------------------------

Data you want the tool to see is provided through one dedicated
directory of the host, not through your home directory:

.. code-block:: bash

   mkdir -p $HOME/ai-share

   flake-ctl firecracker volume export \
       --path $HOME/ai-share --app $HOME/bin/claude

   flake-ctl firecracker volume add --app $HOME/bin/claude \
       --volume $HOME/ai-share:/home/ai/share

The directory is exported through NFS to the VM of the application
and mounted at ``/home/ai/share`` when the instance starts. The
network has to be connected before, the volume is reached through it.
Put the data the tool should work on into that directory, e.g a clone
of the project you want it to work on, and nothing else.

The tool reads and writes the directory with the rights of its owner
on the host. It can change, delete or add any file in it, check what
you take over from it, e.g a script or a build file, before you run
it on the host. The directory has to be owned by you and must not be
reached through a symbolic link. :ref:`firecracker-volumes` explains
the export and the volume setup.

Log in With an API Key
----------------------

Call the application and log the tool in with the API key of your
provider from the shell inside of the VM:

.. code-block:: bash

   claude

``claude``
   .. code-block:: bash

      export ANTHROPIC_API_KEY=YOUR_API_KEY
      claude

   Claude Code asks once to approve the key on its first start.

``copilot``
   .. code-block:: bash

      export COPILOT_GITHUB_TOKEN=YOUR_TOKEN
      copilot

   The token is a fine-grained personal access token of your GitHub
   account with the **Copilot Requests** permission. Classic personal
   access tokens are not supported by the Copilot CLI. The GitHub CLI
   ``gh`` reads its token from ``GH_TOKEN``.

``deepseek``
   .. code-block:: bash

      export DEEPSEEK_API_KEY=YOUR_API_KEY
      pi

   Select one of the DeepSeek models with ``/model``.

``pi``
   .. code-block:: bash

      export ANTHROPIC_API_KEY=YOUR_API_KEY
      pi

   Use the variable of your provider, e.g ``OPENAI_API_KEY``,
   ``GEMINI_API_KEY`` or ``OPENROUTER_API_KEY``, and select a model
   of that provider with ``/model``.

An exported key is known to the shell it is exported in. To keep it
for the next start of the instance, add the ``export`` line to
``/home/ai/.bashrc`` inside of the VM. The file is stored in the
storage of the instance, never put a key into the shared directory.

An API key is the simplest way to log in, but not the only one. The
tools also support the login to a subscription of the provider or
the access to the models through a cloud platform, e.g Google Vertex
AI or Amazon Bedrock. Most of these methods require an extra utility
in the VM which handles the login to that platform, e.g ``gcloud``
for Google Vertex AI. The ``claude`` image comes with ``gcloud``,
Claude Code uses Google Vertex AI as follows:

.. code-block:: bash

   gcloud auth application-default login
   export CLAUDE_CODE_USE_VERTEX=1
   export CLOUD_ML_REGION=YOUR_REGION
   export ANTHROPIC_VERTEX_PROJECT_ID=YOUR_PROJECT_ID
   claude

A utility which is not part of the image can be installed, see
below. The documentation of the tool lists the methods it supports.

Working With AI Instances
-------------------------

Because of ``--resume`` the instance keeps running after the first
call. Every further call of ``claude`` on the host, e.g from another
terminal, opens one more shell of the ``ai`` user in the same
instance. All of these sessions share the file system, the processes
and the login of the instance: run the tool in one session, and look
at the files it changes, run the tests of your project or install
software from another one.

An agent might need software which is not part of the pre-built
image, e.g a compiler or the build tools of your project. The images
are based on openSUSE Leap, software is installed with ``zypper`` as
``root``. The ``ai`` user is allowed to run any command as ``root``
through ``sudo`` without a password, the rule is part of the images
in ``/etc/sudoers.d/ai``:

.. code-block:: bash

   sudo zypper install gcc make

The software is stored in the storage of the instance and kept for
its next start.

With the rule the tool can install what it needs by itself, and it
can run any other command as ``root`` inside of the VM without asking
you as well. ``root`` of the VM reaches nothing of the host beyond
what the ``ai`` user reaches: the shared directory is accessed with
the rights of its owner on the host either way, and the network is
the same. If you want to keep ``root`` to yourself, set a ``root``
password of your own and remove the rule afterwards:

.. code-block:: bash

   sudo passwd root
   sudo rm /etc/sudoers.d/ai

From then on you become ``root`` with ``su -`` and your password, the
change is kept in the storage of the instance as well.

Calling the application with an ``@NAME`` argument starts a separate
instance, which runs in a firecracker process of its own:

.. code-block:: bash

   claude @project-a

The argument selects the instance and is not passed on to the shell.
A name consists of letters, digits, ``_``, ``-`` and ``=``. Every
instance has a storage of its own, e.g ``claude@project-a.ext4``
below ``~/.config/flakes/firecracker/storage``, and is resumed on its
own: further calls of ``claude @project-a`` open more sessions in
that instance, calls without the argument in the instance of the
application. Use separate instances for AI workloads which must not
see the data of each other, e.g one instance per project or per
customer.

An instance needs a network setup of its own, a TAP device cannot be
shared across instances. To keep the data of the instances apart,
give each of them a shared directory of its own as well, which takes
the place of the directory of the application:

.. code-block:: bash

   flake-ctl firecracker network add \
       --app $HOME/bin/claude --instance @project-a

   mkdir -p $HOME/ai-share-project-a

   flake-ctl firecracker volume export \
       --path $HOME/ai-share-project-a \
       --app $HOME/bin/claude --instance @project-a

   flake-ctl firecracker volume add \
       --app $HOME/bin/claude --instance @project-a \
       --volume $HOME/ai-share-project-a:/home/ai/share

Like the application, the instance has to be connected again after a
reboot of the host. A new instance starts from the plain image: log
in, and if needed install software or remove the ``sudo`` rule, in
that instance again. Every running instance is a VM of its own and takes
its share of the memory and the processors of the host.

The Instance Is Disposable
--------------------------

The instance is not a valuable system. It is the runtime of an AI
workload, the valuable part is the data the workload produces. Keep
that data in the shared directory, which is stored on the host, or
e.g commit and push it to a repository, and treat the instance as
something you can throw away. This means you can kill an instance at
any time, e.g when a tool does something you did not ask for, to
make sure no AI keeps running wild.

``flake-ctl firecracker show`` lists the instances along with the
process ID of their firecracker process in the ``id`` column and
their ``status``:

.. code-block:: bash

   flake-ctl firecracker show

Killing that process stops the VM at once, along with the tool and
all sessions of the instance:

.. code-block:: bash

   kill PROCESS_ID

The next call of the application starts the instance again from its
storage. The storage of an instance commits its journal every second.
If the instance gets killed, only the data written to the storage in
the last second before can be lost, and the journal keeps the file
system of the storage consistent. A file in the shared directory
which the tool was writing at that moment may be incomplete.

What Remains Reachable
----------------------

With this setup the tool sees the file system of the VM, the shared
directory and the network the host reaches through its outgoing
interface. It does not see the file system of the host otherwise.
:ref:`isolation-limits` summarizes what a connected VM can reach.
