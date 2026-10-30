//
// Copyright (c) 2022 Elektrobit Automotive GmbH
// Copyright (c) 2023 Marcus Schäfer
//
// This file is part of flake-pilot
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
//
use std::{thread, time};
use flakes::io::IO;
use std::process::Command;
use flakes::command::{handle_output, CommandExtTrait};
use flakes::error::{FlakeError, OperationError};
use flakes::user::{User, chmod};
use flakes::lookup::Lookup;
use flakes::network::get_tap_name as get_tap_device_name;
use flakes::network::{
    read_network_config, get_jump_rules, get_masquerade_rule, FirewallRule,
    NetworkConfig
};
use spinoff::{Spinner, spinners};
use ubyte::ByteUnit;
use std::path::Path;
use std::process::{ExitCode, Stdio, id};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::env;
use std::fs;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use crate::config::{config, is_usermode, RuntimeSection, EngineSection};
use tempfile::NamedTempFile;
use std::io::{self, Write, SeekFrom, Seek};
use std::fs::File;
use std::os::unix::net::{UnixStream, UnixListener};
use std::io::prelude::*;
use std::net::Shutdown;
use serde::{Serialize, Deserialize};
use serde_json::{self};
use flakes::config::get_firecracker_ids_dir;
use std::os::fd::AsRawFd;
use uzers::{get_current_uid, get_current_username, get_user_by_uid};
use uzers::os::unix::UserExt;

use crate::defaults;

// FireCrackerConfig represents firecracker json config
#[derive(Debug, Serialize, Deserialize)]
pub struct FireCrackerConfig {
    #[serde(rename = "boot-source")]
    pub boot_source: FireCrackerBootSource,
    pub drives: Vec<FireCrackerDrive>,
    // A VM without a network setup has no interfaces. In this case
    // the section is not written to the config passed to firecracker
    #[serde(rename = "network-interfaces")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_interfaces: Vec<FireCrackerNetworkInterface>,
    #[serde(rename = "machine-config")]
    pub machine_config: FireCrackerMachine,
    pub vsock: FireCrackerVsock
}
#[derive(Debug, Serialize, Deserialize)]
pub struct FireCrackerBootSource {
    pub kernel_image_path: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub initrd_path: String,
    pub boot_args: String
}
#[derive(Debug, Serialize, Deserialize)]
pub struct FireCrackerDrive {
    pub drive_id: String,
    pub path_on_host: String,
    pub is_root_device: bool,
    pub is_read_only: bool,
    pub cache_type: String
}
#[derive(Debug, Serialize, Deserialize)]
pub struct FireCrackerNetworkInterface {
    pub iface_id: String,
    pub guest_mac: String,
    pub host_dev_name: String
}
#[derive(Debug, Serialize, Deserialize)]
pub struct FireCrackerMachine {
    pub vcpu_count: i64,
    pub mem_size_mib: i64
}
#[derive(Debug, Serialize, Deserialize)]
pub struct FireCrackerVsock {
    pub guest_cid: u32,
    pub uds_path: String
}

pub fn create(program_name: &String) -> Result<(String, String), FlakeError> {
    /*!
    Create VM for later execution of program_name.
    The VM name and all other settings to run the program
    inside of the VM are taken from the config file(s)

    FIRECRACKER_FLAKE_DIR/
       ├── program_name.d
       │   └── other.yaml
       └── program_name.yaml

    All commandline options will be passed to the program_name
    called in the VM through the sci guestvm tool. An example
    program config file looks like the following:

    vm:
      name: name
      target_app_path: path/to/program/in/VM
      host_app_path: path/to/program/on/host

      runtime:
        # Resume the VM from previous execution.
        # If the VM is still running, the app will be
        # executed inside of this VM instance.
        #
        # Default: false
        resume: true|false

        # Force using a vsock to communicate between guest and
        # host if resume is set to false. In resume mode the vsock
        # setup is always required.
        #
        # Default: false
        force_vsock: true|false

        # Name of the user to call the application as inside
        # of the VM. If the user does not exist in the VM, the
        # application is called as root
        #
        # Default: root
        user: name

        # Pilot options which are always effective for this
        # application. The same options can be given at call
        # time and then take precedence over the setting here
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
          mem_size_mib: 4096
          vcpu_count: 2
          cache_type: Writeback

          # Size of the VM overlay
          # If specified a new ext4 overlay filesystem image of the
          # specified size will be created and attached to the VM
          overlay_size: 20GB

          # Path to rootfs image done by app registration
          rootfs_image_path: /var/lib/firecracker/images/NAME/rootfs

          # Path to kernel image done by app registration
          kernel_image_path: /var/lib/firecracker/images/NAME/kernel

          # Optional path to initrd image done by app registration
          initrd_path: /var/lib/firecracker/images/NAME/initrd

          # Optional instance specific settings, keyed by the
          # @NAME instance selector. A boot_args option set here
          # replaces the global setting of the same option for
          # this instance. Options not set globally are appended
          instance:
            "@id":
              boot_args:
                - "ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off"

    Calling this method returns a vector including a placeholder
    for the later VM process ID and and the name of
    the VM ID file.
    !*/
    if ! Lookup::which(defaults::FIRECRACKER) {
        return Err(FlakeError::IOError {
            kind: "FileNotFound".to_string(),
            message: format!("{} not found in $PATH, installed ?",
                defaults::FIRECRACKER
            )
        })
    }
    // Read and check optional @NAME pilot arguments. The instance
    // name is used to construct file, socket and network device
    // names and is therefore only allowed to contain characters
    // which are safe to be used in such names
    for instance_name in env::args().skip(1).filter(
        |arg| arg.starts_with('@')
    ) {
        if ! Lookup::is_safe_instance_name(&instance_name) {
            return Err(FlakeError::IOError {
                kind: "Invalid instance name".to_string(),
                message: format!(
                    "The instance name {instance_name} contains characters \
                    which are not allowed"
                )
            })
        }
    }

    // Make sure meta dirs exists
    init_meta_dirs()?;

    // setup VM ID file name
    let vm_id_file_path = get_meta_file_name(
        program_name, &get_ids_dir()?, "vmid"
    );

    // get flake config sections
    let RuntimeSection {
        resume, firecracker: engine_section, ..
    } = config().runtime();

    // Check for an existing instance. The pilot does not delete
    // the meta data of an instance, this is done when the flake
    // gets removed via 'flake-ctl firecracker remove'. The meta
    // data of an instance which is no longer running is reused
    // and replaced when the VM gets created
    if Path::new(&vm_id_file_path).exists() {
        let vmid = fs::read_to_string(&vm_id_file_path)?;
        if vm_running(&vmid)? {
            if resume {
                // VM exists
                // report ID value and its ID file name
                return Ok((vmid, vm_id_file_path));
            }
            // we are about to create a VM for which
            // a VM is already running
            return Err(FlakeError::AlreadyRunning)
        }
    }

    // Setup VM...
    let pilot_options = Lookup::get_pilot_run_options(
        config().pilot_options()
    );
    let mut spinner = None;
    if pilot_options.contains_key("%progress") {
        spinner = Some(
            Spinner::new_with_stream(
                spinners::Line, "Launching flake...",
                None, spinoff::Streams::Stderr
            )
        );
    }

    match run_creation(&vm_id_file_path, program_name, engine_section) {
        Ok(result) => {
            if let Some(spinner) = spinner {
                spinner.stop_and_persist("✓", "Launching flake");
            }
            Ok(result)
        },
        Err(error) => {
            if let Some(spinner) = spinner {
                spinner.stop_and_persist("✗", "Flake launch has failed");
            }
            Err(error)
        },
    }
}

fn run_creation(
    vm_id_file_path: &str,
    program_name: &String,
    engine_section: EngineSection
) -> Result<(String, String), FlakeError> {
    // Create initial vm_id_file with process ID set to 0
    IO::create_meta_file(vm_id_file_path)?.write_all("0".as_bytes())?;
    let result = ("0".to_owned(), vm_id_file_path.to_owned());

    // Setup root overlay if configured
    let vm_overlay_file = get_meta_file_name(
        program_name, &get_overlay_dir()?,
        defaults::FIRECRACKER_STORAGE_EXTENSION
    );
    if let Some(overlay_size) = engine_section.overlay_size {
        let overlay_size = overlay_size.parse::<ByteUnit>().expect(
            "could not parse overlay size"
        ).as_u64();
        if !Path::new(&vm_overlay_file).exists() {
            let mut vm_overlay_file_fd = File::create(&vm_overlay_file)?;
            vm_overlay_file_fd.seek(SeekFrom::Start(overlay_size - 1))?;
            vm_overlay_file_fd.write_all(&[0])?;

            // Create filesystem
            let mut mkfs = Command::new("/usr/sbin/mkfs.ext4");
            mkfs.arg("-F")
                .arg("-O")
                .arg("has_journal")
                .arg(&vm_overlay_file);
            if Lookup::is_debug() {
                debug!("{:?} {:?}", mkfs.get_program(), mkfs.get_args());
            }
            match mkfs.perform() {
                Ok(_) => { },
                Err(error) => {
                    error!("Failed to create overlay: {error:?}");
                    fs::remove_file(&vm_overlay_file)?;
                }
            }
        }
    }
    Ok(result)
}

pub fn start(
    program_name: &String, (vm_id, vm_id_file): (String, String)
) -> Result<ExitCode, FlakeError> {
    /*!
    Start VM with the given VM ID

    firecracker-pilot exits with the return code of the app called
    in the VM after this function. If the app does not report its
    exit status, the return code from firecracker is used
    !*/
    let RuntimeSection {
        resume, force_vsock, ..
    } = config().runtime();

    let mut is_blocking: bool = true;
    let exit_status;

    if vm_running(&vm_id)? {
        // 1. Execute app in running VM
        exit_status = execute_command_at_instance(program_name)?;
    } else {
        let firecracker_config = NamedTempFile::new()?;
        create_firecracker_config(
            program_name, &firecracker_config
        )?;
        if resume || force_vsock {
            // 2. Startup VM as background job and execute app through vsock
            is_blocking = false;
            call_instance(
                program_name, &firecracker_config, &vm_id_file, is_blocking
            )?;
            exit_status = execute_command_at_instance(program_name)?;
        } else {
            // 3. Startup VM and execute app
            exit_status = call_instance(
                program_name, &firecracker_config, &vm_id_file, is_blocking
            )?;
        }
    }
    Ok(get_exit_code(exit_status))
}

pub fn get_exit_code(exit_status: Option<i32>) -> ExitCode {
    /*!
    Provide the exit code of firecracker-pilot for the exit status
    the app in the VM has reported

    An app which has not reported its exit status, e.g because the
    sci in the VM does not support it yet, keeps the return code
    from firecracker which is a success at this point
    !*/
    match exit_status {
        // Like a shell, only the lower 8 bits of the status are used
        Some(code) => ExitCode::from(code as u8),
        None => ExitCode::SUCCESS
    }
}

pub fn call_instance(
    program_name: &String, config_file: &NamedTempFile,
    vm_id_file: &str, is_blocking: bool
) -> Result<Option<i32>, FlakeError> {
    /*!
    Run firecracker with specified configuration

    In blocking mode the app runs on the console of the instance
    and the exit status it reports is returned
    !*/
    // Check if there is an old UDS socket file still present.
    // It must be deleted to allow the new firecracker process
    // to create and connect to it
    let vsock_uds_path = get_vsock_uds_path(program_name)?;
    if Path::new(&vsock_uds_path).exists() {
        if Lookup::is_debug() {
            debug!("Deleting {vsock_uds_path} prior new firecracker call");
        }
        delete_file(&vsock_uds_path);
    }
    // The app on the console of the instance reports its exit status
    // to this listener. It must exist before the instance boots
    let exit_status_receiver = if is_blocking {
        Some(exit_status_listener(
            &format!("{vsock_uds_path}_{}", defaults::EXIT_STATUS_PORT)
        )?)
    } else {
        None
    };
    // Its standard error is received on a channel of its own if
    // the caller keeps it apart from the standard output
    let stderr_relay = if is_blocking {
        stderr_listener(&vsock_uds_path, defaults::TERM_CONSOLE_PORT)?
    } else {
        None
    };
    let mut firecracker = Command::new(defaults::FIRECRACKER);
    firecracker
        .arg("--no-api")
        .arg("--id")
        .arg(id().to_string())
        .arg("--config-file")
        .arg(config_file.path());
    if ! is_blocking {
        firecracker
            .stderr(Stdio::null())
            .stdout(Stdio::null())
            .stdin(Stdio::null());
    }
    if Lookup::is_debug() {
        debug!("{:?} {:?}", firecracker.get_program(), firecracker.get_args());
    }
    let child = firecracker.spawn()?;
    let pid = child.id();
    if Lookup::is_debug() {
        debug!("PID {pid}")
    }
    IO::create_meta_file(vm_id_file)?.write_all(pid.to_string().as_bytes())?;
    let mut exit_status = None;
    if let Some(exit_status_receiver) = exit_status_receiver {
        // the command talks to the console of the instance,
        // follow the window size of the caller's terminal for it
        watch_terminal_size(&vsock_uds_path, defaults::TERM_CONSOLE_PORT);
        handle_output(
            child.wait_with_output(), firecracker.get_program(),
            firecracker.get_args()
        )?;
        // sci waits until the exit status got received before it
        // shuts down the instance. Thus it is available now if
        // it was sent
        exit_status = exit_status_receiver.try_recv().ok();
    }
    if let Some(stderr_relay) = stderr_relay {
        stderr_relay.finish()
    }
    Ok(exit_status)
}

pub fn exit_status_listener(
    socket_path: &str
) -> Result<Receiver<i32>, FlakeError> {
    /*!
    Listen for the exit status of the app on the console of
    the instance

    The exit status is provided through the returned channel
    !*/
    IO::no_symlink(socket_path)?;
    if Path::new(socket_path).exists() {
        delete_file(&socket_path.to_string());
    }
    let listener = UnixListener::bind(socket_path)?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            if let Some(exit_status) = read_exit_status(&mut stream) {
                let _ = sender.send(exit_status);
            }
            // Closing the stream tells sci the exit status got
            // received. It is done after it was handed over such
            // that it is available once the instance is gone
        }
    });
    Ok(receiver)
}

pub fn read_exit_status(stream: &mut UnixStream) -> Option<i32> {
    /*!
    Read the exit status sci sends for an app

    The exit status is a single line. Only this line is read
    because sci keeps the connection open until the pilot closes
    it. Firecracker does not pass the shutdown of the sending
    side of sci on to the pilot, waiting for the end of the
    stream would therefore run into the timeout
    !*/
    let _ = stream.set_read_timeout(
        Some(time::Duration::from_millis(defaults::EXIT_STATUS_TIMEOUT_MSEC))
    );
    let mut message = String::new();
    if let Err(error) = io::BufReader::new(stream).read_line(&mut message) {
        if Lookup::is_debug() {
            debug!("Failed to read exit status: {error}");
        }
        return None
    }
    match message.trim().parse::<i32>() {
        Ok(exit_status) => {
            if Lookup::is_debug() {
                debug!("App exit status: {exit_status}");
            }
            Some(exit_status)
        },
        Err(error) => {
            if Lookup::is_debug() {
                debug!("Invalid exit status {message:?}: {error}");
            }
            None
        }
    }
}

pub fn accept_exit_status(listener: &UnixListener) -> Option<i32> {
    /*!
    Accept the connection which sends the exit status of an app
    called through a vsock and read the exit status

    sci connects for this after the output stream of the app has
    ended. An sci which does not support it never connects, thus
    only wait for a limited time
    !*/
    if let Err(error) = listener.set_nonblocking(true) {
        if Lookup::is_debug() {
            debug!("Failed to wait for exit status: {error}");
        }
        return None
    }
    let timeout = time::Duration::from_millis(
        defaults::EXIT_STATUS_TIMEOUT_MSEC
    );
    let start = time::Instant::now();
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                return read_exit_status(&mut stream)
            },
            Err(error) if error.kind() == io::ErrorKind::WouldBlock
                && start.elapsed() < timeout => {
                thread::sleep(time::Duration::from_millis(10));
            },
            Err(error) => {
                if Lookup::is_debug() {
                    debug!("No exit status received: {error}");
                }
                return None
            }
        }
    }
}

pub fn has_separate_stderr() -> bool {
    /*!
    Tell whether the standard error of the caller is kept apart
    from its standard output

    This is the case unless both are terminals, e.g if one of
    them is redirected into a file or a pipe
    !*/
    let is_terminal = |fd| unsafe { libc::isatty(fd) } == 1;
    ! (is_terminal(libc::STDOUT_FILENO) && is_terminal(libc::STDERR_FILENO))
}

pub fn get_stderr_port(port: u32) -> Option<u32> {
    /*!
    Provide the port the standard error of the command is
    received on for the session of the given port
    !*/
    if port & defaults::STDERR_PORT_FLAG != 0 {
        return None
    }
    Some(port | defaults::STDERR_PORT_FLAG)
}

pub struct StderrRelay {
    socket_path: String,
    session_over: Arc<AtomicBool>,
    thread: thread::JoinHandle<()>
}

impl StderrRelay {
    pub fn finish(self) {
        /*!
        Pass on what is left of the standard error of the command
        once its session is over
        !*/
        self.session_over.store(true, Ordering::SeqCst);
        let _ = self.thread.join();
        delete_file(&self.socket_path);
    }
}

pub fn stderr_listener(
    vsock_uds_path: &str, port: u32
) -> Result<Option<StderrRelay>, FlakeError> {
    /*!
    Listen for the standard error of the command run in the
    session of the given port and pass it on to the standard
    error of the caller

    This is only done if the caller keeps standard output and
    standard error apart. Otherwise sci passes on the standard
    error together with the standard output
    !*/
    if ! has_separate_stderr() {
        return Ok(None)
    }
    let stderr_port = match get_stderr_port(port) {
        Some(stderr_port) => stderr_port,
        None => {
            if Lookup::is_debug() {
                debug!("No standard error channel for port {port}");
            }
            return Ok(None)
        }
    };
    let socket_path = format!("{vsock_uds_path}_{stderr_port}");
    IO::no_symlink(&socket_path)?;
    if Path::new(&socket_path).exists() {
        delete_file(&socket_path);
    }
    let listener = UnixListener::bind(&socket_path)?;
    listener.set_nonblocking(true)?;
    let session_over = Arc::new(AtomicBool::new(false));
    let thread = thread::spawn({
        let session_over = Arc::clone(&session_over);
        move || relay_stderr(&listener, &session_over, &mut io::stderr())
    });
    Ok(Some(StderrRelay { socket_path, session_over, thread }))
}

pub fn relay_stderr(
    listener: &UnixListener, session_over: &AtomicBool, output: &mut impl Write
) {
    /*!
    Pass on the standard error sci sends for a command to the
    given output

    sci connects for it before the command runs, an sci which
    does not support it never connects. Once the session is over
    only the data which is still on its way is passed on, the
    relay ends if nothing arrived for STDERR_DRAIN_TIMEOUT_MSEC.
    Thus a process in the instance which inherited the standard
    error of the command, e.g a background process, does not
    keep the caller waiting
    !*/
    let mut stream = loop {
        // A connection made before the session got over is
        // already queued, thus it is taken by the accept below
        let is_session_over = session_over.load(Ordering::SeqCst);
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if is_session_over {
                    return
                }
                thread::sleep(time::Duration::from_millis(10));
            },
            Err(error) => {
                if Lookup::is_debug() {
                    debug!("Standard error connection failed: {error}");
                }
                return
            }
        }
    };
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(time::Duration::from_millis(
        defaults::STDERR_DRAIN_TIMEOUT_MSEC
    )));
    let mut buffer = [0_u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(sz_r) => {
                if output.write_all(&buffer[0..sz_r]).is_err() {
                    if Lookup::is_debug() {
                        debug!("write failure on stderr");
                    }
                    break
                }
                let _ = output.flush();
            },
            Err(error) if error.kind() == io::ErrorKind::Interrupted => { },
            Err(error) if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) => {
                if session_over.load(Ordering::SeqCst) {
                    if Lookup::is_debug() {
                        debug!("Standard error still open after the session");
                    }
                    break
                }
            },
            Err(error) => {
                if Lookup::is_debug() {
                    debug!("read failure on standard error stream: {error}");
                }
                break
            }
        }
    }
}

pub fn get_exec_port() -> u32 {
    /*!
    Find free port

    Note: This method finds a free port within the firecracker-pilot
    managed port assignments. If the selected port is occupied by
    another service in the system it will create a conflict. In this
    case use the pilot call option %port:number to bind to a port
    of your choice
    !*/
    let pilot_options = Lookup::get_pilot_run_options(
        config().pilot_options()
    );
    let port: u32 = if pilot_options.contains_key("%port") {
        pilot_options["%port"].parse::<u32>().unwrap_or_default()
    } else {
        defaults::FIRECRACKER_VSOCK_PORT_START + id()
    };
    port
}

pub fn check_connected(program_name: &String) -> Result<(), FlakeError> {
    /*!
    Check if instance connection is OK
    !*/
    let mut retry_count = 0;
    let vsock_uds_path = get_vsock_uds_path(program_name)?;
    // The socket provides command execution inside of the VM.
    // Only the owner of the VM instance is allowed to talk to it.
    // The socket is created by the firecracker process the pilot
    // started for the calling user and belongs to that user, who
    // therefore changes its permissions without sudo
    let calling_user = get_current_username()
        .and_then(|name| name.into_string().ok())
        .unwrap_or_default();
    chmod(&vsock_uds_path, "600", User::from(calling_user.as_str()))?;
    loop {
        if retry_count == defaults::RETRIES {
            if Lookup::is_debug() {
                debug!("Max retries for VM connection check exceeded")
            }
            return Err(
                FlakeError::OperationError(OperationError::MaxTriesExceeded)
            )
        }
        let mut buffer = [0; 14];
        if let Ok(mut stream) = UnixStream::connect(&vsock_uds_path) {
            let _ = stream.set_write_timeout(
                Some(time::Duration::from_millis(200))
            );
            let _ = stream.set_read_timeout(
                Some(time::Duration::from_millis(200))
            );
            stream.write_all(
                format!("CONNECT {}\n", defaults::VM_PORT
            ).as_bytes())?;
            if stream.read_exact(&mut buffer).is_ok() {
                let output = String::from_utf8(buffer.to_vec()).unwrap();
                if output.starts_with("OK") {
                    return Ok(())
                }
            }
            stream.shutdown(Shutdown::Both).unwrap();
        }
        // VM not yet ready for connections
        let some_time = time::Duration::from_millis(
            defaults::VM_WAIT_TIMEOUT_MSEC
        );
        if Lookup::is_debug() {
            debug!(
                "Sleeping(check_connected): {}ms",
                defaults::VM_WAIT_TIMEOUT_MSEC
            );
        }
        thread::sleep(some_time);
        retry_count += 1
    }
}

pub fn send_command_to_instance(
    program_name: &str, vsock_uds_path: &str, exec_port: u32
) -> i32 {
    /*!
    Send command to the VM via a vsock
    !*/
    let mut status_code;
    let mut retry_count = 0;
    let mut run: Vec<String> = vec![get_target_app_path(program_name)];

    run = Lookup::get_run_cmdline(run, false);
    loop {
        status_code = 1;
        if retry_count == defaults::RETRIES {
            if Lookup::is_debug() {
                debug!("Max retries for VM command transfer exceeded");
            }
            return status_code
        }
        match UnixStream::connect(vsock_uds_path) {
            Ok(mut stream) => {
                stream.write_all(
                    format!("CONNECT {}\n", defaults::VM_PORT).as_bytes()
                ).unwrap();
                let mut buffer = [0; 14];
                match stream.read_exact(&mut buffer) {
                    Ok(_) => {
                        let output = String::from_utf8(
                            buffer.to_vec()
                        ).unwrap();
                        if output.starts_with("OK") {
                            stream.write_all(
                                format!(
                                    "{} {}\n", run.join(" "), exec_port
                                ).as_bytes()
                            ).unwrap();
                            status_code = 0
                        }
                    },
                    Err(_) => {
                        status_code = 1
                    }
                }
                stream.shutdown(Shutdown::Both).unwrap();
            },
            Err(_) => {
                status_code = 1
            }
        }
        if status_code == 1 {
            // VM not yet ready for connections
            let some_time = time::Duration::from_millis(
                defaults::VM_WAIT_TIMEOUT_MSEC
            );
            if Lookup::is_debug() {
                debug!(
                    "Sleeping(send_command_to_instance): {}ms",
                    defaults::VM_WAIT_TIMEOUT_MSEC
                );
            }
            thread::sleep(some_time);
        } else {
            break
        }
        retry_count += 1
    }
    status_code
}

pub fn execute_command_at_instance(
    program_name: &String
) -> Result<Option<i32>, FlakeError> {
    /*!
    Send command to a vsock connected to a running instance

    The exit status reported for the command is returned
    !*/
    let mut retry_count = 0;
    let vsock_uds_path = get_vsock_uds_path(program_name)?;

    // wait for UDS socket to appear
    loop {
        if retry_count == defaults::RETRIES {
            if Lookup::is_debug() {
                debug!("Max retries for UDS socket lookup exceeded");
            }
            return Err(
                FlakeError::OperationError(OperationError::MaxTriesExceeded)
            )
        }
        if Path::new(&vsock_uds_path).exists() {
            break
        }
        let some_time = time::Duration::from_millis(100);
        if Lookup::is_debug() {
            debug!("Sleeping(execute_command_at_instance): 100ms");
        }
        thread::sleep(some_time);
        retry_count += 1
    }

    // make sure instance can be contacted
    check_connected(program_name)?;

    // spawn the listener and wait for sci to run the command
    let exec_port = get_exec_port();
    let command_socket = &format!("{vsock_uds_path}_{exec_port}");
    IO::no_symlink(command_socket)?;
    let thread_handle = stream_listener(command_socket);

    // receive the standard error of the command on a channel of its
    // own if the caller keeps it apart from the standard output
    let stderr_relay = stderr_listener(&vsock_uds_path, exec_port)?;

    // follow the window size of the caller's terminal. The size
    // the instance was booted with does not apply to a session
    // in a terminal which was resized or which is not the one
    // the instance was started from
    watch_terminal_size(&vsock_uds_path, exec_port);

    if send_command_to_instance(
        program_name, &vsock_uds_path, exec_port
    ) != 0 {
        // nobody connects to the listeners, don't wait for them
        if let Some(stderr_relay) = stderr_relay {
            stderr_relay.finish()
        }
        return Err(
            FlakeError::OperationError(OperationError::MaxTriesExceeded)
        )
    }

    let exit_status = thread_handle.join().ok().flatten();
    if let Some(stderr_relay) = stderr_relay {
        stderr_relay.finish()
    }
    Ok(exit_status)
}

pub fn get_terminal_boot_args() -> Vec<String> {
    /*!
    Provide the terminal setup of the caller as kernel boot params

    The terminal type is required in the guest to switch on the
    line editor of interactive commands, e.g the tab completion of
    a shell. The window size is required to let that line editor
    redraw the input line and to arrange the tab completion matches
    in columns
    !*/
    let mut terminal_boot_args: Vec<String> = Vec::new();
    if let Ok(term) = env::var("TERM") {
        // Only pass along a terminal name which cannot be used to
        // smuggle further parameters into the kernel commandline
        let is_terminal_name = ! term.is_empty()
            && term.len() <= defaults::TERM_NAME_MAX_LEN
            && term.chars().all(
                |char| char.is_ascii_alphanumeric() || "-_.+".contains(char)
            );
        if is_terminal_name {
            terminal_boot_args.push(format!("sci_term={term}"))
        } else if Lookup::is_debug() {
            debug!("Unsupported TERM name, not passed to the instance");
        }
    }
    if let Some((lines, columns)) = get_terminal_size() {
        terminal_boot_args.push(format!("sci_lines={lines}"));
        terminal_boot_args.push(format!("sci_columns={columns}"));
    }
    terminal_boot_args
}

pub fn watch_terminal_size(vsock_uds_path: &str, port: u32) {
    /*!
    Follow the window size of the caller's terminal

    The initial geometry of the terminal is handed over to the
    instance as kernel boot params. This is only correct as long
    as the terminal keeps its size and it does not apply at all
    to an instance which was started by someone else before. Thus
    watch the terminal and send its geometry to the instance
    whenever it changed.

    The size is polled and not taken from a SIGWINCH handler.
    Dragging a window border creates a burst of resize events and
    polling combines them to the one size which is left at the
    end of it. It also keeps the data transfer loops of the
    session free from being interrupted by a signal
    !*/
    if get_terminal_size().is_none() {
        // no terminal to follow, e.g the output is redirected
        if Lookup::is_debug() {
            debug!("No terminal to watch for window size changes");
        }
        return
    }
    let vsock_uds_path = vsock_uds_path.to_owned();
    thread::spawn(move || {
        let mut sent_size = None;
        let mut retry_count = 0;
        loop {
            let terminal_size = get_terminal_size();
            if let Some((lines, columns)) = terminal_size {
                if terminal_size != sent_size {
                    // The instance cannot be reached at any time, e.g
                    // it could still be booting. Keep the size to be
                    // sent in this case and try again on the next turn
                    if send_terminal_size_to_instance(
                        &vsock_uds_path, port, lines, columns
                    ) {
                        sent_size = terminal_size;
                        retry_count = 0
                    } else {
                        if retry_count <= defaults::RETRIES {
                            retry_count += 1;
                            if retry_count > defaults::RETRIES
                                && Lookup::is_debug()
                            {
                                debug!(
                                    "Instance does not take window sizes, \
                                     continue watching at a lower rate"
                                );
                            }
                        }
                    }
                }
            } else {
                // the terminal is gone, e.g it was closed
                sent_size = terminal_size
            }
            // Poll at the rate of a resize as long as the instance
            // takes the window sizes. An instance which does not,
            // e.g because it runs an older sci, is contacted at the
            // rate of a connection retry to keep the load low
            let poll_interval = if retry_count > defaults::RETRIES {
                time::Duration::from_millis(defaults::VM_WAIT_TIMEOUT_MSEC)
            } else {
                time::Duration::from_millis(defaults::TERM_RESIZE_POLL_MSEC)
            };
            thread::sleep(poll_interval)
        }
    });
}

fn send_terminal_size_to_instance(
    vsock_uds_path: &str, port: u32, lines: u16, columns: u16
) -> bool {
    /*!
    Send the given window size to the instance

    The size is sent to the resize port of the instance together
    with the port of the session it belongs to. sci applies it to
    the terminal of that session
    !*/
    let timeout = Some(time::Duration::from_millis(
        defaults::TERM_RESIZE_TIMEOUT_MSEC
    ));
    let mut stream = match UnixStream::connect(vsock_uds_path) {
        Ok(stream) => stream,
        Err(error) => {
            if Lookup::is_debug() {
                debug!("Failed to connect {vsock_uds_path}: {error}");
            }
            return false
        }
    };
    let _ = stream.set_read_timeout(timeout);
    let _ = stream.set_write_timeout(timeout);
    let mut sent = stream.write_all(
        format!("CONNECT {}\n", defaults::TERM_RESIZE_PORT).as_bytes()
    ).is_ok();
    if sent {
        // The connection to the port is confirmed with an OK
        // message. An instance which does not listen for window
        // sizes, e.g an older sci, refuses the connection
        let mut buffer = [0; 14];
        let bytes_read = stream.read(&mut buffer).unwrap_or(0);
        sent = String::from_utf8_lossy(&buffer[0..bytes_read]).starts_with("OK")
    }
    if sent {
        sent = stream.write_all(
            format!("{port} {lines} {columns}\n").as_bytes()
        ).is_ok()
    }
    let _ = stream.shutdown(Shutdown::Both);
    if Lookup::is_debug() && ! sent {
        debug!("Failed to send window size {lines}x{columns} to the instance");
    }
    sent
}

pub fn get_terminal_size() -> Option<(u16, u16)> {
    /*!
    Provide the window size of the caller's terminal
    !*/
    let mut window_size = std::mem::MaybeUninit::<libc::winsize>::uninit();
    let result = unsafe {
        libc::ioctl(
            io::stdin().as_raw_fd(),
            libc::TIOCGWINSZ as _,
            window_size.as_mut_ptr()
        )
    };
    if result == -1 {
        return None
    }
    let window_size = unsafe { window_size.assume_init() };
    if window_size.ws_row == 0 || window_size.ws_col == 0 {
        return None
    }
    Some((window_size.ws_row, window_size.ws_col))
}

pub fn create_firecracker_config(
    program_name: &String,
    config_file: &NamedTempFile
) -> Result<(), FlakeError> {
    /*!
    Create json config to call firecracker
    !*/
    let template = File::open(defaults::FIRECRACKER_TEMPLATE)?;
    let mut firecracker_config: FireCrackerConfig = serde_json::from_reader(
        template
    )?;
    let mut boot_args: Vec<String> = Vec::new();
    let RuntimeSection {
        resume, force_vsock, user, firecracker: engine_section, ..
    } = config().runtime();

    // set kernel_image_path
    engine_section.kernel_image_path.clone_into(
        &mut firecracker_config.boot_source.kernel_image_path
    );

    // set initrd_path
    if let Some(initrd_path) = engine_section.initrd_path {
        initrd_path.clone_into(&mut firecracker_config.boot_source.initrd_path);
    }

    // setup run commandline for the command call
    let mut run: Vec<String> = vec![get_target_app_path(program_name)];

    run = Lookup::get_run_cmdline(run, true);

    // set boot_args
    if Lookup::is_debug() {
        boot_args.push("PILOT_DEBUG=1".to_string());
    }
    if engine_section.overlay_size.is_some() {
        boot_args.push("overlay_root=/dev/vdb".to_string());
    }
    // hand over the setup of the caller's terminal. sci uses this
    // information to setup the pseudo terminal of the command such
    // that interactive commands, e.g a shell, can provide features
    // like tab completion
    boot_args.append(&mut get_terminal_boot_args());
    let flake_boot_args = engine_section.get_boot_args(
        &Lookup::get_instance_name()
    );
    let with_network = has_network_setup(&flake_boot_args);
    // Sanity check: The volumes of a VM are provided through NFS.
    // Without a server on the host the guest cannot mount them and
    // would come up without the data the application expects
    let nfs_volumes = get_nfs_volumes(&flake_boot_args);
    if ! nfs_volumes.is_empty() && ! nfsd_is_running() {
        return Err(FlakeError::IOError {
            kind: "NFSServerNotRunning".to_string(),
            message: format!(
                "The flake is configured to use the NFS volume(s) {} but \
                the NFS server of the host is not running. The server is \
                part of the host setup and is not started at boot unless \
                it is enabled, please call '{}' first",
                nfs_volumes.join(" "),
                get_nfs_server_start_command()
            )
        })
    }
    for boot_option in flake_boot_args {
        if is_sci_mode_boot_arg(boot_option) {
            // written by former registrations, replaced by the
            // setting of the runtime section
            continue
        }
        if (resume || force_vsock)
            && ! Lookup::is_debug()
            && boot_option.starts_with("console=")
        {
            // in resume mode the communication is handled
            // through vsocks. Thus we don't need a serial
            // console and only provide one in debug mode
            boot_args.push("console=".to_string());
        } else {
            boot_args.push(boot_option.to_owned());
        }
    }
    // sci must run the instance in the mode the pilot acts upon.
    // Otherwise e.g a resume mode sci keeps the instance running
    // after the command while the pilot does not use it again
    boot_args.append(&mut get_sci_mode_boot_args(resume, force_vsock));
    // sci calls the application as the configured user
    if let Some(user) = user {
        boot_args.push(get_sci_user_boot_arg(user)?);
    }
    if ! firecracker_config.boot_source.boot_args.is_empty() {
        firecracker_config.boot_source.boot_args.push(' ');
    }
    firecracker_config.boot_source.boot_args.push_str(
        &boot_args.join(" ")
    );
    if resume || force_vsock {
        firecracker_config.boot_source.boot_args.push_str(
            " run=vsock"
        )
    } else {
        firecracker_config.boot_source.boot_args.push_str(
            &format!(" run=\"{}\"", run.join(" "))
        )
    }

    // set path_on_host for rootfs
    engine_section.rootfs_image_path.clone_into(
        &mut firecracker_config.drives[0].path_on_host
    );

    // set drive section for overlay
    if engine_section.overlay_size.is_some() {
        let vm_overlay_file = get_meta_file_name(
            program_name, &get_overlay_dir()?,
            defaults::FIRECRACKER_STORAGE_EXTENSION
        );

        let cache_type =
            engine_section.cache_type.unwrap_or_default().to_string();

        let drive = FireCrackerDrive {
            drive_id: "overlay".to_string(),
            path_on_host: vm_overlay_file,
            is_root_device: false,
            is_read_only: false,
            cache_type
        };
        firecracker_config.drives.push(drive);
    }

    // set tap device name
    //
    // The device itself is not created here. It is part of the
    // host setup which is created with 'flake-ctl firecracker
    // network add'
    //
    // A VM whose kernel commandline configures no interface has no
    // network, e.g because it was freshly registered or because
    // its setup was deleted with 'flake-ctl firecracker
    // network remove'. Such a VM gets no network-interfaces
    // section in the config passed to firecracker
    if ! with_network {
        if Lookup::is_debug() {
            debug!("No network configured, deleting network interfaces");
        }
        firecracker_config.network_interfaces.clear();
    } else if firecracker_config.network_interfaces.is_empty() {
        if Lookup::is_debug() {
            debug!("No network interface configured, skipping tap setup");
        }
    } else {
        let tapname = get_tap_name(program_name);
        if Lookup::is_debug() {
            debug!("tap device is {tapname}");
        }
        // Sanity check: A VM configured to use a network can only
        // be connected to the host if its tap device exists
        if ! tap_device_exists(&tapname) {
            return Err(FlakeError::IOError {
                kind: "FileNotFound".to_string(),
                message: format!(
                    "The flake is configured to use a network but its \
                    tap device {tapname} does not exist. The device is \
                    part of the host setup, please call '{}' first",
                    get_network_add_command()
                )
            })
        }
        // Sanity check: The traffic of the VM only reaches the
        // outside world if the firewall setup of the host is active
        if let Some(network_config) = get_network_record() {
            let outgoing_interface = &network_config.outgoing_interface;
            if let Some(missing_rule) = get_missing_firewall_rule(
                &network_config
            ) {
                return Err(FlakeError::IOError {
                    kind: "NetworkSetupMissing".to_string(),
                    message: format!(
                        "The flake is configured to use a network but the \
                        {missing_rule} rule of the firewall setup which \
                        lets its traffic pass through the outgoing \
                        interface {outgoing_interface} is not active. The \
                        rules are part of the host setup and are lost on \
                        reboot, please call '{}' first",
                        get_network_init_command(outgoing_interface)
                    )
                })
            }
        }
        firecracker_config.network_interfaces[0].host_dev_name = tapname;
    }

    // set vsock name
    firecracker_config.vsock.guest_cid = defaults::VM_CID;
    firecracker_config.vsock.uds_path = get_vsock_uds_path(program_name)?;

    // set mem_size_mib
    if let Some(mem_size_mib) = engine_section.mem_size_mib {
        firecracker_config.machine_config.mem_size_mib = mem_size_mib
    }

    // set vcpu_count
    if let Some(vcpu_count) = engine_section.vcpu_count {
        firecracker_config.machine_config.vcpu_count = vcpu_count;
    }
    if Lookup::is_debug() {
        debug!("{}", serde_json::to_string(&firecracker_config)?);
    }
    serde_json::to_writer(
        config_file, &firecracker_config
    )?;

    Ok(())
}

pub fn has_network_setup(boot_args: &[&str]) -> bool {
    /*!
    Check if the given kernel commandline configures a network

    An interface of the VM is only of use if the guest kernel is
    told to bring it up. This is done with the 'ip=' boot option
    which is written by 'flake-ctl firecracker network add' and
    which is deleted by 'flake-ctl firecracker register'.
    The values 'off' and 'none' explicitly switch the network of
    the guest off and are therefore treated like a missing option
    !*/
    boot_args.iter()
        .filter_map(|boot_arg| boot_arg.strip_prefix("ip="))
        .any(|setup| setup != "off" && setup != "none")
}

pub fn get_sci_mode_boot_args(resume: bool, force_vsock: bool) -> Vec<String> {
    /*!
    Provide the kernel commandline options which tell sci to run
    the instance in the given mode

    In resume mode sci keeps the instance running and waits for
    further commands through a vsock. Without resume, sci shuts
    down the instance once the command has ended, the command is
    received through a vsock if force_vsock is set
    !*/
    let mut boot_args = Vec::new();
    if resume {
        boot_args.push(format!("{}=1", defaults::SCI_RESUME_BOOT_ARG));
    }
    if force_vsock {
        boot_args.push(format!("{}=1", defaults::SCI_FORCE_VSOCK_BOOT_ARG));
    }
    boot_args
}

pub fn get_sci_user_boot_arg(user: &str) -> Result<String, FlakeError> {
    /*!
    Provide the kernel commandline option which tells sci the
    name of the user to call the application as

    The name becomes part of the kernel commandline. A name which
    could break it up, e.g by whitespace, is refused
    !*/
    if ! Lookup::is_safe_user_name(user) {
        return Err(FlakeError::IOError {
            kind: "InvalidUserName".to_string(),
            message: format!(
                "Invalid user name '{user}' in the runtime section: \
                expected up to 32 characters out of [A-Za-z0-9._-], \
                starting with [A-Za-z_]"
            )
        })
    }
    Ok(format!("{}={user}", defaults::SCI_USER_BOOT_ARG))
}

pub fn is_sci_mode_boot_arg(boot_arg: &str) -> bool {
    /*!
    Check if the given kernel commandline option sets the mode
    sci runs the instance in
    !*/
    let name = boot_arg.split_once('=').map_or(boot_arg, |(name, _)| name);
    name == defaults::SCI_RESUME_BOOT_ARG
        || name == defaults::SCI_FORCE_VSOCK_BOOT_ARG
}

pub fn get_nfs_volumes<'a>(boot_args: &[&'a str]) -> Vec<&'a str> {
    /*!
    Provide the NFS volumes configured in the given commandline

    A volume is attached to a VM with 'flake-ctl firecracker
    volume add' which exports the host path through NFS and adds
    it to the 'nfs=' option of the guest kernel commandline. All
    volumes are kept in one comma separated list, sci mounts them
    in the guest
    !*/
    boot_args.iter()
        .filter_map(|boot_arg| boot_arg.split_once('='))
        .filter(|(name, _)| *name == defaults::NFS_VOLUME_BOOT_ARG)
        .flat_map(
            |(_, volumes)| volumes.split(defaults::NFS_VOLUME_DELIMITER)
        )
        .map(str::trim)
        .filter(|volume| ! volume.is_empty())
        .collect()
}

pub fn nfsd_is_running() -> bool {
    /*!
    Check if the NFS server of the host is running

    The volumes are exported by the NFS server of the kernel. It
    is not started by the pilot but is part of the host setup and,
    unless the service was enabled, is gone after a reboot.

    The kernel provides the statistics of the server as soon as
    the nfsd module is loaded. No record means there is no server,
    a record which does not provide the number of threads allows
    no statement about it and is not reported as an error
    !*/
    let nfsd_stat = match fs::read_to_string(defaults::NFS_SERVER_STAT) {
        Ok(nfsd_stat) => nfsd_stat,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if Lookup::is_debug() {
                debug!(
                    "No {} record, nfsd is not loaded",
                    defaults::NFS_SERVER_STAT
                );
            }
            return false
        },
        Err(error) => {
            if Lookup::is_debug() {
                debug!(
                    "Cannot read {}: {error}, skipping the check",
                    defaults::NFS_SERVER_STAT
                );
            }
            return true
        }
    };
    match get_nfsd_threads(&nfsd_stat) {
        Some(threads) => {
            if Lookup::is_debug() {
                debug!("nfsd runs {threads} thread(s)");
            }
            threads > 0
        },
        None => {
            if Lookup::is_debug() {
                debug!(
                    "No thread count in {}, skipping the check",
                    defaults::NFS_SERVER_STAT
                );
            }
            true
        }
    }
}

pub fn get_nfsd_threads(nfsd_stat: &str) -> Option<u32> {
    /*!
    Provide the number of nfsd threads from the given statistics

    The kernel reports them as the first value of the 'th' record
    !*/
    nfsd_stat.lines()
        .filter_map(|record| record.strip_prefix("th "))
        .filter_map(|threads| threads.split_whitespace().next())
        .find_map(|threads| threads.parse().ok())
}

pub fn tap_device_exists(tap: &str) -> bool {
    /*!
    Check if the given TAP device is present on the host

    The device is not created by the pilot but is part of the
    host setup which is created with 'flake-ctl firecracker
    network add'. All network interfaces of the host are
    reported by the kernel below /sys/class/net
    !*/
    Path::new(defaults::SYS_CLASS_NET).join(tap).exists()
}

pub fn get_network_record() -> Option<NetworkConfig> {
    /*!
    Provide the record of the host network setup the VM uses

    This is the setup 'flake-ctl firecracker network init' created
    and recorded. Without a record there is no known setup, e.g
    because it was created by hand, and the firewall setup of the
    host cannot be checked.

    The record belonging to the registration is the one to look
    at first. As the system wide setup can be used by a flake of
    a user and the other way round, the record of the other
    setup is taken if there is none
    !*/
    let network_config = read_network_config(is_usermode())
        .or_else(|| read_network_config(! is_usermode()));
    if Lookup::is_debug() {
        match &network_config {
            Some(network_config) => debug!(
                "outgoing interface is {}", network_config.outgoing_interface
            ),
            None => debug!("No network setup record found")
        }
    }
    network_config
}

pub fn get_missing_firewall_rule(
    network_config: &NetworkConfig
) -> Option<String> {
    /*!
    Provide the chain of the first rule of the host firewall setup
    which is not active, None if the setup is complete

    The rules are the ones 'flake-ctl firecracker network init'
    creates: the jump rules into the chains of the setup and the
    rule which masquerades the traffic of the VM network. They
    are not created by the pilot but are part of the host setup
    and, as runtime state of the host, are gone after a reboot.
    A record without a network predates the selection of the
    network, only the jump rules are checked in this case.

    A rule which cannot be read, e.g because the caller is not
    allowed to look at the netfilter rules, is not reported as
    missing. In that case there is nothing to check
    !*/
    let mut rules = get_jump_rules();
    if let Some(network) = &network_config.network {
        rules.push(
            get_masquerade_rule(network, &network_config.outgoing_interface)
        );
    }
    for rule in rules {
        if firewall_rule_exists(&rule) {
            continue
        }
        if ! firewall_chain_readable(&rule) {
            if Lookup::is_debug() {
                debug!(
                    "Cannot read the {} rules, skipping the check", rule.chain
                );
            }
            return None
        }
        return Some(rule.chain)
    }
    None
}

fn firewall_rule_exists(rule: &FirewallRule) -> bool {
    /*!
    Check if the given rule is active on the host
    !*/
    let mut call = iptables(rule.table);
    call.arg("-C").arg(&rule.chain).args(&rule.spec);
    iptables_ok(&mut call)
}

fn firewall_chain_readable(rule: &FirewallRule) -> bool {
    /*!
    Check if the rules of the chain of the given rule can be read

    Reading the netfilter rules requires root privileges. Telling
    a chain without the searched rule apart from one which could
    not be read at all needs this extra look, both of them let
    the search for the rule fail
    !*/
    let mut call = iptables(rule.table);
    call.arg("-S").arg(&rule.chain);
    iptables_ok(&mut call)
}

fn iptables(table: &str) -> Command {
    /*!
    Create an iptables call for the given table

    The netfilter rules are only readable for root. The call is
    nevertheless made with the privileges of the caller and not
    passed to sudo: the rules are only looked at for a sanity
    check, and a sudo rule for iptables would allow to change any
    rule of the host. For a caller who is not root the rules cannot
    be read and the check is skipped, see get_missing_firewall_rule()
    !*/
    let mut call = Command::new(defaults::IPTABLES_TOOL);
    call.arg("-t").arg(table);
    call
}

fn iptables_ok(call: &mut Command) -> bool {
    /*!
    Run the given iptables call and tell whether it succeeded

    A rule which is not active and a chain which cannot be read
    are reported on stdout/stderr. Neither of them is an error
    in this context and therefore not shown
    !*/
    if Lookup::is_debug() {
        debug!("{:?} {:?}", call.get_program(), call.get_args());
    }
    match call.stdout(Stdio::null()).stderr(Stdio::null()).status() {
        Ok(status) => status.success(),
        Err(_) => false
    }
}

pub fn get_network_init_command(outgoing_interface: &str) -> String {
    /*!
    Construct the 'flake-ctl firecracker network init' call which
    creates the netfilter rules of the host setup

    The setup is created for the interface the traffic of all VMs
    of the host leaves through. Therefore the call to report is
    the same for every flake
    !*/
    format!(
        "flake-ctl firecracker network init \
        --outgoing-interface {outgoing_interface}"
    )
}

pub fn get_network_add_command() -> String {
    /*!
    Construct the 'flake-ctl firecracker network add' call which
    creates the host network setup for the calling flake

    Each instance of an application needs a network setup of its
    own. Therefore the @NAME instance selector, if given, is part
    of the call to report
    !*/
    let mut command = format!(
        "flake-ctl firecracker network add --app {}",
        config().vm.host_app_path
    );
    let instance_name = Lookup::get_instance_name();
    if ! instance_name.is_empty() {
        command.push_str(&format!(" --instance {instance_name}"));
    }
    // The TAP device belongs to the user who runs the VM. A system
    // wide flake is set up by root, who has to name that user
    if ! is_usermode() {
        let user = get_current_username()
            .map(|user| user.to_string_lossy().to_string())
            .unwrap_or_else(|| get_current_uid().to_string());
        command.push_str(&format!(" --user {user}"));
    }
    command
}

pub fn get_nfs_server_start_command() -> String {
    /*!
    Construct the call which starts the NFS server of the host

    The server is started by 'flake-ctl firecracker volume add'
    when the first volume is exported. That call does not enable
    the service though, which is why the reported call does so and
    keeps the server across a reboot of the host
    !*/
    format!("sudo systemctl enable --now {}", defaults::NFS_SERVER_SERVICE)
}

pub fn get_target_app_path(program_name: &str) -> String {
    /*!
    setup application command path name

    This is either the program name specified at registration
    time or the configured target application from the flake
    configuration file
    !*/
    config().vm.target_app_path.unwrap_or(program_name).to_owned()

}

pub fn get_ids_dir() -> Result<String, FlakeError> {
    /*!
    Provide the private directory of the calling user to store
    the VM ID files and the sockets to talk to the VM instances

    The base directory shared by all users is not created by the
    pilot, see IO::private_dir
    !*/
    IO::private_dir(&get_firecracker_ids_dir(false))
}

pub fn get_overlay_dir() -> Result<String, FlakeError> {
    /*!
    Provide the directory to store the VM overlay images

    The overlay images are stored in the home directory of the
    calling user. If there is no home directory the private
    meta data directory of the user is used. A directory shared
    with other users must not be used because the overlay image
    becomes the root filesystem of the VM.

    The home directory is read from the system user database and
    not from the HOME environment variable. flake-ctl looks up
    the overlay images of an instance the same way and would not
    find them at a location the environment of the pilot call
    pointed to
    !*/
    match get_user_by_uid(get_current_uid()) {
        Some(user) => Ok(
            format!(
                "{}/{}",
                user.home_dir().to_string_lossy(),
                defaults::FIRECRACKER_OVERLAY_DIR
            )
        ),
        None => Ok(
            format!("{}/{}", get_ids_dir()?, defaults::FIRECRACKER_STORAGE_DIR)
        )
    }
}

pub fn get_vsock_uds_path(program_name: &String) -> Result<String, FlakeError> {
    /*!
    Provide the path of the socket used to talk to the VM instance

    The socket allows to run commands inside of the VM and is
    therefore placed in the private directory of the calling user
    !*/
    Ok(format!(
        "{}/{}{}.sock",
        get_ids_dir()?,
        defaults::FIRECRACKER_VSOCK_PREFIX,
        get_meta_name(program_name)
    ))
}

pub fn init_meta_dirs() -> Result<(), FlakeError> {
    /*!
    Create meta data directory structure

    The directories are created for and owned by the calling user.
    Meta data of one user must not be modifiable by another user
    !*/
    get_ids_dir()?;
    let overlay_dir = get_overlay_dir()?;
    if ! Path::new(&overlay_dir).is_dir() {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&overlay_dir)?;
    }
    Ok(())
}

pub fn vm_running(vmid: &String) -> Result<bool, FlakeError> {
    /*!
    Check if VM with specified vmid is running
    !*/
    if Lookup::is_debug() {
        debug!("vm id is {vmid}");
    }
    if vmid == "0" {
        return Ok(false)
    }
    let mut running = Command::new("kill");
    running.arg("-0").arg(vmid);
    if Lookup::is_debug() {
        debug!("{:?} {:?}", running.get_program(), running.get_args());
    }

    let output = running.output()?;
    Ok(output.status.success())
}

pub fn get_meta_file_name(
    program_name: &String, target_dir: &str, extension: &str
) -> String {
    /*!
    Construct meta data file name from given program name
    !*/
    let meta_file = format!(
        "{}/{}.{}", target_dir, get_meta_name(program_name), extension
    );
    meta_file
}

pub fn get_meta_name(program_name: &String) -> String {
    /*!
    Construct meta data basename from given program name
    !*/
    let args: Vec<String> = env::args().collect();
    let mut meta_file = program_name.to_string();
    for arg in &args[1..] {
        if arg.starts_with('@') {
            // The special @NAME argument is not passed to the
            // actual call and can be used to run different VM
            // instances for the same application
            meta_file = format!("{meta_file}{arg}");
        }
    }
    meta_file
}

pub fn get_tap_name(program_name: &String) -> String {
    /*!
    Construct the name of the TAP device for the given program name

    The device belongs to the calling user, who runs the VM
    !*/
    get_tap_device_name(get_current_uid(), &get_meta_name(program_name))
}

pub fn delete_file(filename: &String) -> bool {
    /*!
    Delete file

    The deletion is done in process. Calling out to rm would
    resolve the program through the callers PATH
    !*/
    match fs::remove_file(filename) {
        Ok(_) => { },
        Err(error) => {
            error!("Failed to remove: {filename}: {error:?}");
            return false
        }
    }
    true
}

pub struct TerminalMode {
    fd: i32,
    saved: Option<libc::termios>
}

impl TerminalMode {
    pub fn raw(fd: i32) -> Self {
        /*!
        Switch the given terminal into raw mode

        In raw mode the terminal driver no longer buffers the input
        until the line is complete and no longer echoes what was
        typed. Every single key stroke is passed on unmodified and
        the echo is left to the command on the other end of the
        connection. Output post processing stays switched on such
        that output which only provides a line feed is still
        displayed correctly. The original terminal setup is
        restored when the returned object is dropped.

        If the given file descriptor is not a terminal, e.g if the
        input is piped, nothing is changed
        !*/
        if unsafe { libc::isatty(fd) } != 1 {
            return TerminalMode { fd, saved: None }
        }
        let mut terminal = std::mem::MaybeUninit::<libc::termios>::uninit();
        if unsafe { libc::tcgetattr(fd, terminal.as_mut_ptr()) } != 0 {
            if Lookup::is_debug() {
                debug!(
                    "tcgetattr failed with: {}", io::Error::last_os_error()
                );
            }
            return TerminalMode { fd, saved: None }
        }
        let saved = unsafe { terminal.assume_init() };
        let mut raw = saved;
        raw.c_lflag &= !(
            libc::ECHO | libc::ECHONL | libc::ICANON |
            libc::ISIG | libc::IEXTEN
        );
        raw.c_iflag &= !(libc::ICRNL | libc::IXON);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            if Lookup::is_debug() {
                debug!(
                    "tcsetattr failed with: {}", io::Error::last_os_error()
                );
            }
            return TerminalMode { fd, saved: None }
        }
        TerminalMode { fd, saved: Some(saved) }
    }
}

impl Drop for TerminalMode {
    fn drop(&mut self) {
        if let Some(saved) = self.saved {
            unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &saved) };
        }
    }
}

pub fn stream_listener(socket_path: &str) -> thread::JoinHandle<Option<i32>> {
    /*!
    Listen for the I/O stream of a command called through a vsock

    The first connection carries the I/O of the command. Once it
    has ended, sci connects a second time to send the exit status
    of the command which is returned by the thread
    !*/
    let mut socket = String::new();
    socket.push_str(socket_path);
    let handle = move |socket: String| {
        match UnixListener::bind(socket) {
            Ok(listener) => {
                if let Some(stream) = listener.incoming().next() {
                    match stream {
                        Ok(stream) => {
                            stream_io(stream);
                            return accept_exit_status(&listener)
                        }
                        Err(error) => {
                            error!("VM Connection failed: {error}");
                        }
                    }
                }
            }
            Err(error) => {
                error!("VM socket listener failed: {error}")
            }
        }
        None
    };
    thread::spawn(move || {handle(socket)})
}

pub fn stream_io(mut stream: UnixStream) {
    let mut stdout = io::stdout();

    let stream_fd = stream.as_raw_fd();
    let stdin_fd = libc::STDIN_FILENO;

    // The input of the caller can end before the command in the
    // guest is done, e.g if it is read from a file. The output of
    // the command is passed on until the guest closes the stream
    let mut stdin_open = true;
    // Tell whether the input sent so far ends with a complete line
    let mut at_line_start = true;

    // Switch the caller's terminal into raw mode for the time of
    // the session. Only in raw mode a single key stroke, e.g the
    // TAB key, is passed on to the guest instead of being buffered
    // until the line is complete. This is the precondition for the
    // line editor of an interactive command in the guest, e.g a
    // shell, to provide tab completion. The original terminal
    // setup is restored when this function returns
    let _terminal_mode = TerminalMode::raw(stdin_fd);

    // main send/recv loop
    let mut buffer = [0_u8; 1];
    loop {
        // prepare file descriptors to be watched for by select()
        let raw_fdset = std::mem::MaybeUninit::<libc::fd_set>::uninit();
        let mut fdset = unsafe { raw_fdset.assume_init() };
        let mut max_fd = -1;
        unsafe { libc::FD_ZERO(&mut fdset) };

        if stdin_open {
            unsafe { libc::FD_SET(stdin_fd, &mut fdset) };
            max_fd = std::cmp::max(max_fd, stdin_fd);
        }

        unsafe { libc::FD_SET(stream_fd, &mut fdset) };
        max_fd = std::cmp::max(max_fd, stream_fd);

        // block this thread until something new happens
        // on these file-descriptors
        unsafe {
            libc::select(
                max_fd + 1,
                &mut fdset,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut()
            )
        };
        // this thread is not blocked any more,
        // try to handle what happened on the file descriptors
        if unsafe { libc::FD_ISSET(stdin_fd, &fdset) } {
            // something new happened on stdin,
            // try to receive some bytes and send them through the stream
            //
            // Read from the terminal device itself and not through the
            // buffered stdin of the standard library. The buffered
            // reader fetches as much as the device provides but hands
            // out only the requested amount of bytes. The rest stays
            // in its buffer where select() cannot see it and would
            // only be delivered on the next key stroke. Keys like the
            // cursor keys send a multi byte escape sequence and would
            // therefore arrive at the guest in pieces or not at all
            let sz_r = unsafe {
                libc::read(
                    stdin_fd,
                    buffer.as_mut_ptr() as *mut libc::c_void,
                    buffer.len()
                )
            };
            if sz_r == -1 {
                if Lookup::is_debug() {
                    debug!("read failure on stdin: {}", io::Error::last_os_error());
                }
                break;
            }
            if sz_r == 0 {
                if Lookup::is_debug() {
                    debug!("EOF detected on stdin");
                }
                // Closing the stream would also end the output of the
                // command. Instead, like a terminal does it, end the
                // input with the EOF character. An incomplete line
                // needs one of them to be passed on to the command
                // and another one to end the input
                stdin_open = false;
                let eof_count = if at_line_start { 1 } else { 2 };
                if stream.write_all(
                    &vec![defaults::TERM_EOF; eof_count]
                ).is_err() {
                    if Lookup::is_debug() {
                        debug!("write failure on stream");
                    }
                    break;
                }
            } else {
                let input = &buffer[0..sz_r as usize];
                if stream.write_all(input).is_err() {
                    if Lookup::is_debug() {
                        debug!("write failure on stream");
                    }
                    break;
                }
                at_line_start = matches!(input.last(), Some(b'\n' | b'\r'));
            }
            let _ = stdout.flush();
        }
        if unsafe { libc::FD_ISSET(stream_fd, &fdset) } {
            // something new happened on the stream
            // try to receive some bytes and send them to stdout
            if let Ok(sz_r) = stream.read(&mut buffer) {
                if sz_r == 0 {
                    if Lookup::is_debug() {
                        debug!("EOF detected on stream");
                    }
                    break;
                }
                if stdout.write_all(&buffer[0..sz_r]).is_err() {
                    if Lookup::is_debug() {
                        debug!("write failure on stdout");
                    }
                    break;
                }
                let _ = stdout.flush();
            } else {
                if Lookup::is_debug() {
                    debug!("read failure on stream");
                }
                break;
            }
        }
    }
}
