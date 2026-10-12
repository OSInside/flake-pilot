# Threat model: flake-pilot

## What this project does

flake-pilot makes applications that live inside an isolated runtime callable like native
programs on a Linux host. Registering an application (a *flake*) creates two things:

- a symlink at the application path (e.g. `/usr/bin/some` or `~/bin/some`) that points to a
  launcher binary, called a *pilot*;
- a YAML registration describing the image, the target program and the runtime options.

When the application is called, the pilot does the following:

1. derives the registration name from `argv[0]`;
2. merges the YAML with its drop-in directories;
3. provisions an instance;
4. runs the target program inside it, passing through stdin/stdout and the exit code.

The workspace consists of:

- **`podman-pilot/`**: OCI container flakes through podman, including delta containers, layers
  and provisioning of host files into the container.
- **`bubblewrap-pilot/`**: sandbox flakes. The root is an overlay of a directory tree of the
  host, built with kernel overlayfs and then run in `bwrap`.
- **`firecracker-pilot/`**: microVM flakes. Each launch creates an overlay disk image, mounts
  the image to provision includes, boots Firecracker, and talks to the guest over vsock or the
  serial console.
- **`firecracker-pilot/guestvm-tools/sci/`**: `sci`, the static init (PID 1) inside the guest.
  It does the following:
  - sets up the overlay root and calls `switch_root`;
  - mounts the NFS volumes listed on the kernel command line;
  - runs the `run=` command, or listens on vsock for commands from the host and connects their
    PTY back to the host;
  - applies terminal resize requests.
- **`common/`** (crate `flakes`): code shared by all pilots:
  - registration lookup and merging (`registration.rs`, `config.rs`);
  - argument handling (`lookup.rs`);
  - the `sudo` wrappers (`user.rs`);
  - symlink-safe file operations (`openat.rs`);
  - private metadata directories and include sync (`io.rs`);
  - podman helpers (`podman.rs`).
- **`flake-ctl/`**: the management CLI (register/remove, image pull/load/export, Firecracker
  networking and NFS volume export).

**The main focus of this scan is the pilots, `sci`, and the `common/` code they use.**

### Privilege model (important for judging findings)

- **No setuid binaries.** The pilots run as the calling user. Anything that needs more
  privileges is delegated to the system `sudo`.

  The wrappers are in `common/src/user.rs` and `common/src/podman.rs`. Only explicitly listed
  environment variables are preserved across sudo. The project ships no sudoers policy; the
  admin decides who may run these commands.
- **System vs. user registrations.**
  - System registrations (`/usr/share/flakes`, `/etc/flakes.yml`, `/etc/flakes/<app>.d/`) are
    created by root.
  - User registrations (`~/.config/flakes*`) are created by the user. They are used only when
    no system registration of the same name exists (`common/src/registration.rs`).
  - `runas: root` selects rootful operation. Anything else means rootless operation as the
    caller.
- **State shared between users:**
  - `/tmp/flakes/` (mode 1777) holds per-uid private directories (`/tmp/flakes/<uid>`, 0700)
    with container IDs, VM IDs, sandbox PIDs and vsock sockets.
  - `/var/tmp/bwrap_<uid>/` holds the bubblewrap overlay directories.
  - Root podman storage is shared by all rootful flakes.
- **Where Firecracker keeps its files.** Overlay images live in the caller's home directory
  (`~/.config/flakes/firecracker/storage`) and are reused across launches. That overlay is the
  guest's writable root layer.

## Where untrusted input enters

Treat the following as attacker-controlled:

1. **The isolated workload.** Code running inside a container, sandbox or VM guest is
   untrusted. Typical examples are third-party tools and AI coding agents; the docs recommend
   Firecracker specifically for isolating AI agents. This covers:
   - everything the workload writes, including files in the persisted Firecracker overlay image
     and in bubblewrap upper directories, which host-side code later mounts, reads, extracts
     into or rsyncs into, often as root via sudo;
   - all bytes the guest sends back to the host over the vsock or serial channel, which are
     written to the caller's terminal;
   - the guest's influence on exit codes and on files the pilot reads afterwards.
2. **Other local users without sudo rights.** They can create files, directories and symlinks
   in `/tmp`, `/var/tmp` and `/tmp/flakes`, they know other users' uids, and they can race a
   victim's pilot run. A victim's pilot (rootful flake, or a user with sudo rights) must not be
   tricked into acting on paths, IDs, sockets or processes the attacker chose. Examples:
   - CID/VMID/PID file contents and the paths they live at;
   - pre-created `/tmp/flakes` or `/var/tmp/bwrap_<uid>` directories;
   - symlink swaps between check and use;
   - instance and socket name collisions.
3. **Guest-internal unprivileged processes (for `sci`).** `sci` runs as root inside the guest.
   It accepts commands and resize requests on vsock listeners
   (`sci/src/main.rs`, `VsockListener::bind_with_cid_port`) and does not check the peer CID. If
   a workload runs as an unprivileged guest user, it must not be able to:
   - submit commands to `sci`;
   - hijack another session's PTY or resize channel;
   - crash `sci` (PID 1, so the VM goes down).
4. **Image content.** For flakes *without* provisioning, container images, VM root filesystems
   and sandbox trees are workload content (see 1). The pilot must not let image content cause
   host-side effects beyond running the workload in isolation. Examples:
   - symlinks in an image that make root-run `mkdir`, `cp`, `tar` or `rsync` write outside the
     mount;
   - crafted ext4 overlay images mounted by root.

**Trusted** in this model:

- root and anything root owns: system registrations and their drop-in directories,
  `/etc/flakes.yml`, `/etc/flakes/*`, sudoers, the engines (podman, bwrap, firecracker), the
  kernel, and the guest kernel and initrd chosen by root;
- the **calling user's own** registrations, environment, `argv` and cwd, as far as they act
  within that user's own privileges;
- delta containers and layers used with podman provisioning. Their `systemfiles` script runs on
  the host as root by design (see "Anything to leave alone").

## What a real vulnerability looks like here

In scope (roughly in order of interest):

- **Escape from the isolation through pilot or `sci` behavior.** Host-side handling of
  workload-controlled data that leads to file reads/writes, mounts or code execution on the host
  beyond what the registration explicitly shares. Examples:
  - mounting the guest-writable Firecracker overlay and extracting or rsyncing includes into it
    as root (`firecracker-pilot/src/firecracker.rs` `run_creation`/`mount_vm`,
    `common/src/io.rs` `sync_includes`);
  - bubblewrap overlay setup (`bubblewrap-pilot/src/overlay.rs`);
  - provisioning paths that follow symlinks inside an image.
- **Cross-user attacks through shared state.** Symlink, TOCTOU and squatting bugs in
  `/tmp/flakes` and `/var/tmp/bwrap_<uid>` handling that let an unprivileged user make a victim's
  sudo-backed `mkdir`/`chmod`/`cp`/`mount`/`umount`/`podman` act on attacker-chosen paths or
  instances, or read or tamper with the victim's instance data. Relevant code:
  - `common/src/io.rs` `private_dir`;
  - `common/src/user.rs` (`no_symlink_in_path`, `mkdir`, `chmod`, `cp` done as another user);
  - `common/src/openat.rs`;
  - CID/VMID/PID file handling and garbage collection in each pilot.
- **`sci` command channel.** Command and argument injection, or confusion between the host's
  command string and its arguments (the host joins arguments with spaces; `sci` splits them with
  `shell_words`). Also:
  - acceptance of commands or resize requests from peers other than the host;
  - session/port confusion between concurrent sessions in resume mode;
  - PTY and descriptor handling bugs.
- **Data from the guest or workload that affects the host process.** Unbounded reads, panics or
  hangs in host-side stream handling, and terminal control sequences from the guest that can do
  more in the caller's terminal than display output.
- **Environment and option handling at the sudo boundary.** Environment or option injection into
  root-run commands from inputs that are not trusted under the model above, e.g. values read
  from metadata files written by another user, or from image content. Examples:
  - option-looking values (leading `-`) reaching `podman`/`mount`/`tar` argv;
  - `,`/`:` in overlay mount option strings;
  - unvalidated container IDs read from CID files.
- **Information disclosure across users**: sandbox writes, overlays, sockets or metadata that
  are readable by other users, or credentials from substituted `%VAR` options leaking into
  errors or shared root podman metadata.

## Components that matter most / least

Highest priority:

- `firecracker-pilot/src/` (especially `firecracker.rs`: VM creation, overlay image creation and
  mounting, include provisioning, vsock/serial channel, terminal handling, metadata files,
  NFS/network boot arguments).
- `firecracker-pilot/guestvm-tools/sci/src/main.rs` (vsock listeners, command parsing,
  PTY/raw-channel redirection, child reaping, overlay root setup and `switch_root`, NFS mounts,
  resize handling, dracut initrd code).
- `podman-pilot/src/podman.rs` (instance lifecycle, CID files, resume/attach, layer and include
  provisioning, `%VAR` expansion of runtime options, garbage collection).
- `bubblewrap-pilot/src/bubblewrap.rs`, `overlay.rs` (bwrap argument construction, overlay
  directories and root mounts, PID files).
- `common/src/` (`user.rs`, `openat.rs`, `io.rs`, `podman.rs`, `registration.rs`, `config.rs`,
  `lookup.rs`, `command.rs`, `network.rs`): every pilot relies on it.

Lower priority (in scope, but not the focus of this scan):

- `flake-ctl/src/`, in particular image pull/extraction (`firecracker.rs`, `oci.rs`,
  `fetch.rs`), NFS export handling and network setup (`network.rs`). Report clear issues, but
  spend most effort on the pilots and `sci`.

Out of scope:

- `appstore/` (image build descriptions), `package/` (RPM spec and tmpfiles), `doc/`,
  `helper/`, `completion/`, `utils` and test code.

## How to exercise it

- **Where things are.** built with `make build` (`cargo build --release`
  for the whole workspace).
  - `make install` places `podman-pilot`, `bubblewrap-pilot`, `firecracker-pilot` and `flake-ctl`
    in `/usr/bin`, and the registration templates and `storage.conf` in `/etc/flakes/`.
  - `sci` is built but not installed; it is at `/src/target/release/sci`.
  - The global config `/etc/flakes.yml` is not installed by `make install`; copy
    `flakes.yml` there.
- **Tests and rebuilds.** All crate dependencies were fetched during the image build (there are
  no dev-dependencies), so rebuilding and testing work offline:
  - `cd /src && cargo build --offline --workspace`
  - `cargo test --offline --workspace`
  The test suite runs as an unprivileged user and needs no podman, bubblewrap,
  Firecracker or KVM. `make test` runs all tests.
- **`sci`.** Its logic (command-string parsing, resize request parsing, session bookkeeping) can
  be exercised through its unit tests or small harnesses. Do not run `sci` as PID 1 or let it
  mount filesystems in the scanner image.
- **Real pilot testing in OSS scanner container.** As flake-pilot itself is an isolation tool
  that runs applications through container, overlay or virtualization technology, the way OSS
  scanner approaches it creates expected conflicts such as nested overlays, no access to
  /dev/kvm and maybe more.

## How we rate severity

- **Critical**:
  - an untrusted workload (container, sandbox or VM guest) gets code execution or arbitrary file
    write on the host as root or as another user, through pilot or `sci` behavior, with a default
    or documented configuration;
  - a local user **without any sudo rights** gets root on the host through a pilot.
- **High**:
  - escape from the isolation that gives the workload read or write access to host files outside
    what the registration explicitly shares;
  - a local unprivileged user making a victim's sudo-backed pilot action create, delete, mount
    over, chown/chmod, overwrite or disclose arbitrary files, or execute in the victim's
    instance;
  - memory-safety bugs in `unsafe` code (terminal/PTY/`libc` handling) reachable from guest or
    workload data.
- **Medium**:
  - an unprivileged guest process gaining guest root through `sci`, or taking over another
    session's PTY;
  - cross-user information disclosure (another user's instance data, sandbox writes, sockets);
  - one user stopping, removing or hijacking another user's instance;
  - terminal escape sequences from a workload that do more than display output (e.g. inject
    input);
  - cross-user denial of service (blocking another user's flakes via shared directories or
    forged metadata).
- **Low**:
  - panics or hangs on malformed data that only affect the caller's own invocation;
  - option injection that needs a sudoers policy narrower than what the pilot requires;
  - hardening gaps without a demonstrated boundary crossing.
- Not a vulnerability (informational at most): anything that needs control of root-owned
  configuration, the system registry, sudoers, the engine binaries or the guest kernel and
  initrd chosen by root; anything a user can do to themselves with their own privileges.

Please state the attacker (workload, guest user, other local user, caller), what they control,
the sudo rights the scenario assumes, and the engine used.

## Anything to leave alone

- **sudo-based design.** Do not report that a user who is allowed the generic sudo commands the
  pilots delegate (`podman`, `mount`, `tar`, `rsync`, ...) can become root. Under the
  current design such users are root-equivalent. Report a concrete way for someone **without**
  those rights to make use of them instead.
- **The caller's own configuration.** Do not report that a user's own registration, environment
  or arguments can make a pilot do things within that user's own privileges or sudo rights. This
  includes `runas` and `%VAR` expansion. It becomes a finding only when it lets another user, the
  workload or image content cross a boundary.
- **Podman provisioning trust.** Do not report that delta containers and layers used with podman
  provisioning run their `systemfiles` script as root on the host. This trust requirement is by
  design (see the comment in `podman-pilot/src/podman.rs`).
- **Documented bubblewrap behavior.** Do not report that the bubblewrap default options share
  the network and other namespaces of the caller. The documentation states that the bubblewrap
  overlay on the host root is not a security boundary.
- **Engine vulnerabilities.** Do not report vulnerabilities in podman, bubblewrap, Firecracker,
  the kernel or NFS themselves, unless flake-pilot uses them in a way that creates the problem.
- **Code outside this scan's focus.** Do not report issues only in out-of-scope directories,
  tests, or documentation wording.
- **Offline environment.** Do not pull images or contact registries or download hosts; the scan
  runs offline.
