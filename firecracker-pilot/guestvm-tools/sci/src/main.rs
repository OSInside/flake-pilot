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
#[macro_use]
extern crate log;
extern crate shell_words;

pub mod defaults;

use std::env;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;
use std::os::unix::process::CommandExt;
use system_shutdown::force_reboot;
use std::fs;
use sys_mount::Mount;
use env_logger::Env;
use std::{thread, time};
use vsock::{VsockListener, VsockStream};
use std::io::Read;
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::io::Write;
use pty::prelude::Fork;
use termios::*;

use std::sync::{Mutex, MutexGuard};
use std::process::{Child, ExitStatus};

use crate::defaults::debug;

// Process IDs of the child processes sci waits for on its own.
// The reaper must not read the exit status of these processes
static OWNED_CHILDREN: Mutex<Vec<i32>> = Mutex::new(Vec::new());

// Terminal of a command sci runs for a caller
struct TerminalSession {
    // Port the caller listens on. It identifies the session and
    // is defaults::TERM_CONSOLE_PORT for the console of the instance
    port: u32,
    // Descriptor of the terminal, -1 as long as it does not exist
    fd: i32,
    // Window size the caller has sent for this session, if any
    window_size: Option<(u16, u16)>
}

// Terminals sci serves. A window resize of the caller is applied
// to the terminal of the session it belongs to
static TERMINAL_SESSIONS: Mutex<Vec<TerminalSession>> = Mutex::new(Vec::new());

fn main() {
    /*!
    Simple Command Init (sci) is a tool which executes the provided
    command in the run=... cmdline variable or through a vsock
    after preparation of an execution environment for the purpose to
    run a command inside of a firecracker instance.

    if provided via the overlay_root=/dev/block_device kernel boot
    parameter, sci also prepares the root filesystem as an overlay
    using the given block device for writing.

    if provided via the nfs=... kernel boot parameter, sci mounts
    the listed NFS volumes before the command is called.
    !*/
    setup_logger();

    let mut args: Vec<String> = vec![];
    let mut call: Command;
    let mut do_exec = false;
    let mut ok = true;

    env::set_var("PS1", defaults::PROMPT);

    // provide a terminal type for the command call
    setup_terminal_environment();

    // print user space env
    for (key, value) in env::vars() {
        debug(&format!("{key}: {value}"));
    }

    // parse commandline from run environment variable
    match env::var("run").ok() {
        Some(call_cmd) => {
            match shell_words::split(&call_cmd) {
                Ok(call_params) => {
                    args = call_params
                },
                Err(error) => {
                    debug(&format!("Failed to parse {call_cmd}: {error}"));
                    do_reboot(false)
                }
            }
        },
        None => {
            debug("No run=... cmdline parameter in env");
            do_reboot(false)
        }
    }

    // sanity check on command to call
    if args[0].is_empty() {
        debug("No command to execute specified");
    }

    // check if given command requires process replacement
    if args[0] == "/usr/lib/systemd/systemd" {
        do_exec = true;
    }

    // check for resume mode
    let resume = env::var("sci_resume").ok().is_some();

    // check for console setting
    let mut console_vsock = false;
    if resume || env::var("sci_force_vsock").ok().is_some() {
        console_vsock = true
    }

    // mount /proc, /sys and /run, skip if already mounted
    mount_basic_fs();

    // mount overlay if requested
    match env::var("overlay_root").ok() {
        Some(overlay) => {
            // overlay device is specified, mount the device and
            // prepare the folder structure
            let mut modprobe = Command::new(defaults::PROBE_MODULE);
            modprobe.arg("overlay");
            debug(&format!(
                "SCI CALL: {} -> {:?}",
                defaults::PROBE_MODULE, modprobe.get_args()
            ));
            match modprobe.status() {
                Ok(_) => { },
                Err(error) => {
                    debug(&format!("Loading overlay module failed: {error}"));
                }
            }
            debug(&format!("Mounting overlayfs RW({})", overlay.as_str()));
            match Mount::builder()
                .fstype("ext2").mount(overlay.as_str(), "/overlayroot")
            {
                Ok(_) => {
                    debug(&format!("Mounted {overlay:?} on /overlayroot"));
                    ok = true
                },
                Err(error) => {
                    debug(&format!("Failed to mount overlayroot: {error}"));
                    ok = false
                }
            }
            if ok {
                let overlay_dirs = [
                    defaults::OVERLAY_ROOT,
                    defaults::OVERLAY_UPPER,
                    defaults::OVERLAY_WORK
                ];
                for overlay_dir in overlay_dirs.iter() {
                    match fs::create_dir_all(overlay_dir) {
                        Ok(_) => { ok = true },
                        Err(error) => {
                            debug(&format!(
                                "Error creating directory {}: {}",
                                defaults::OVERLAY_ROOT, error
                            ));
                            ok = false;
                            break;
                        }
                    }
                }
            }
            if ok {
                match Mount::builder()
                    .fstype("overlay")
                    .data(
                        &format!("lowerdir=/,upperdir={},workdir={}",
                            defaults::OVERLAY_UPPER, defaults::OVERLAY_WORK
                        )
                    )
                    .mount("overlay", defaults::OVERLAY_ROOT)
                {
                    Ok(_) => {
                        debug(&format!(
                            "Mounted overlay on {}", defaults::OVERLAY_ROOT
                        ));
                        ok = true;
                    },
                    Err(error) => {
                        debug(&format!(
                            "Failed to mount overlayroot: {error}"
                        ));
                        ok = false;
                    }
                }
            }
            // Call specified command through switch root into the overlay
            if ok {
                move_mounts(defaults::OVERLAY_ROOT);
                let root = Path::new(defaults::OVERLAY_ROOT);
                match env::set_current_dir(root) {
                    Ok(_) => {
                        debug(&format!(
                            "Changed working directory to {}", root.display()
                        ));
                        ok = true;
                    },
                    Err(error) => {
                        debug(&format!(
                            "Failed to change working directory: {error}"
                        ));
                        ok = false;
                    }
                }
            }
            if do_exec {
                call = Command::new(defaults::SWITCH_ROOT);
                call.arg(".").arg(&args[0]);
            } else {
                call = Command::new(&args[0]);
                if ok {
                    let mut pivot = Command::new(defaults::PIVOT_ROOT);
                    pivot.arg(".").arg("mnt");
                    debug(&format!(
                        "SCI CALL: {} -> {:?}",
                        defaults::PIVOT_ROOT, pivot.get_args()
                    ));
                    match pivot.status() {
                        Ok(_) => {
                            debug(&format!(
                                "{} is now the new root", defaults::OVERLAY_ROOT
                            ));
                            ok = true;
                        },
                        Err(error) => {
                            debug(&format!("Failed to pivot_root: {error}"));
                            ok = false;
                        }
                    }
                    mount_basic_fs();
                    setup_resolver_link();
                }
            }
        },
        None => {
            // Call command in current environment
            call = Command::new(&args[0]);
        }
    };

    // mount nfs volumes if requested
    mount_nfs_volumes();

    // start sshd if present
    start_sshd();

    // take care for processes which got re-parented to sci
    start_child_reaper();

    // Setup command call parameters
    for arg in &args[1..] {
        call.arg(arg);
    }

    // Perform execution tasks
    if ! ok {
        do_reboot(ok)
    }
    if console_vsock {
        // vsock required; check if vhost transport is loaded
        load_vhost_transport();
        // follow the window size of the caller's terminal
        start_terminal_resize_listener();
        // start vsock listener on VM_PORT, wait for command(s) in a loop
        // A received command turns into a vsock stream process calling
        // the command with an expected listener.
        debug(&format!(
            "Binding vsock CID={} on port={}",
            defaults::GUEST_CID, defaults::VM_PORT
        ));
        match VsockListener::bind_with_cid_port(
            defaults::GUEST_CID, defaults::VM_PORT
        ) {
            Ok(listener) => {
                // Enter main loop
                loop {
                    ok = true;
                    match listener.accept() {
                        Ok((mut stream, addr)) => {
                            // read command string from incoming connection
                            debug(&format!(
                                "Accepted incoming connection from: {}:{}",
                                addr.cid(), addr.port()
                            ));
                            let mut call_str = String::new();
                            let mut call_buf = Vec::new();
                            match stream.read_to_end(&mut call_buf) {
                                Ok(_) => {
                                    call_str = String::from_utf8(
                                        call_buf.to_vec()
                                    ).unwrap();
                                    let len_to_truncate = call_str
                                        .trim_end()
                                        .len();
                                    call_str.truncate(len_to_truncate);
                                },
                                Err(error) => {
                                    debug(&format!(
                                        "Failed to read data {error}"
                                    ));
                                    ok = false
                                }
                            };
                            stream.shutdown(Shutdown::Both).unwrap();
                            if call_str.is_empty() {
                                // Caused by handshake checks that connects
                                // without sending data
                                debug("No data received until connection end");
                                continue
                            }
                            debug(&format!(
                                "SCI CALL RAW BUF: {call_str:?}"
                            ));
                            // The call string consists of the command to
                            // execute followed by the port the caller
                            // listens on. The command is split into its
                            // arguments like a shell does it. Thus an
                            // argument can be quoted with ' or " to keep
                            // the whitespace and the quoting characters
                            // it contains
                            let mut exec_cmd: Vec<String> = Vec::new();
                            let mut exec_port_num = 0;
                            match shell_words::split(&call_str) {
                                Ok(mut call_stack) => {
                                    let exec_port = call_stack.pop()
                                        .unwrap_or_default();
                                    match exec_port.parse::<u32>() {
                                        Ok(num) => { exec_port_num = num },
                                        Err(error) => {
                                            debug(&format!(
                                                "Failed to parse port: {exec_port}: {error}"
                                            ));
                                            ok = false
                                        }
                                    }
                                    exec_cmd = call_stack
                                },
                                Err(error) => {
                                    debug(&format!(
                                        "Failed to parse {call_str}: {error}"
                                    ));
                                    ok = false
                                }
                            }
                            if exec_cmd.is_empty() {
                                debug("No command to execute received");
                                ok = false
                            }
                            debug(&format!(
                                "CALL SCI: {exec_cmd:?} u32:{exec_port_num}"
                            ));

                            // Establish a VSOCK connection with the farend
                            if ok {
                                let thread_handle = thread::spawn(move || {
                                    let mut retry_count = 0;
                                    loop {
                                        if retry_count == defaults::RETRIES {
                                            break
                                        }
                                        match VsockStream::connect_with_cid_port(
                                            2, exec_port_num
                                        ) {
                                            Ok(vsock_stream) => {
                                                redirect_command(
                                                    &exec_cmd, vsock_stream,
                                                    exec_port_num
                                                );
                                                break
                                            },
                                            Err(error) => {
                                                debug(&format!(
                                                    "[{retry_count}] VSOCK-CONNECT failed with: {error}"
                                                ));
                                                let some_time = time::Duration::from_millis(
                                                    defaults::VM_WAIT_TIMEOUT_MSEC
                                                );
                                                thread::sleep(some_time);
                                            }
                                        }
                                        retry_count += 1
                                    }
                                });
                                if ! resume {
                                    // Wait for the thread to finish if not in resume mode
                                    let _ = thread_handle.join();
                                }
                            }
                        },
                        Err(error) => {
                            debug(&format!(
                                "Failed to accept incoming connection: {error}"
                            ));
                            ok = false
                        }
                    }

                    // we are not in resume mode, exit after the command is done
                    if ! resume {
                        break
                    }
                }
            },
            Err(error) => {
                debug(&format!(
                    "Failed to bind vsock: CID: {}: {}",
                    defaults::GUEST_CID, error
                ));
                ok = false
            }
        }
    } else {
        // run regular command and close vm
        //
        // Without a vsock the command talks to the console of the
        // instance. Setup this terminal such that the line editor of
        // an interactive command can move the cursor correctly
        if unsafe { libc::isatty(libc::STDIN_FILENO) } == 1 {
            set_interactive_terminal_flags(libc::STDIN_FILENO);
            if ! do_exec {
                // Follow the window size of the caller's terminal.
                // A process replacement would terminate the listener
                // thread, thus this is only done if sci stays alive
                register_terminal_session(
                    defaults::TERM_CONSOLE_PORT, libc::STDIN_FILENO
                );
                start_terminal_resize_listener();
            }
        }
        if do_exec {
            // replace ourselves
            debug(&format!("EXEC: {} -> {:?}", args[0], call.get_args()));
            let _ = call.exec();
        } else {
            // call a command and keep control
            debug(&format!(
                "SCI CALL: {} -> {:?}", args[0], call.get_args()
            ));
            let _ = run_child(&mut call);
        }
    }
    // Close firecracker session
    do_reboot(ok)
}

fn load_vhost_transport() {
    /*!
    Load the vsock transport module of the guest

    Loading an already loaded module is a no-op, thus this can be
    called from every place which requires a vsock connection
    !*/
    let mut modprobe = Command::new(defaults::PROBE_MODULE);
    modprobe.arg(defaults::VHOST_TRANSPORT);
    debug(&format!(
        "SCI CALL: {} -> {:?}", defaults::PROBE_MODULE, modprobe.get_args()
    ));
    match run_child(&mut modprobe) {
        Ok(_) => { },
        Err(error) => {
            debug(&format!(
                "Loading {} module failed: {}",
                defaults::VHOST_TRANSPORT, error
            ));
        }
    }
}

fn start_terminal_resize_listener() {
    /*!
    Start listening for window size changes of the caller

    The window size of the caller's terminal is handed over at
    boot time through the sci_lines=... and sci_columns=... boot
    parameters. This is a snapshot of the geometry at the time the
    instance was started. If the caller resizes its terminal
    afterwards, or connects to a running instance with a terminal
    of a different geometry, it sends the new window size to this
    listener which applies it to the terminal of the session
    !*/
    thread::spawn(|| {
        load_vhost_transport();
        debug(&format!(
            "Binding terminal resize vsock CID={} on port={}",
            defaults::GUEST_CID, defaults::TERM_RESIZE_PORT
        ));
        match VsockListener::bind_with_cid_port(
            defaults::GUEST_CID, defaults::TERM_RESIZE_PORT
        ) {
            Ok(listener) => {
                for connection in listener.incoming() {
                    match connection {
                        Ok(mut stream) => {
                            // A request is a single line and the
                            // connection ends with it. Don't wait
                            // for a caller which does not send it
                            let _ = stream.set_read_timeout(
                                Some(time::Duration::from_millis(
                                    defaults::VM_WAIT_TIMEOUT_MSEC
                                ))
                            );
                            let mut message = Vec::new();
                            match stream.read_to_end(&mut message) {
                                Ok(_) => handle_resize_request(
                                    &String::from_utf8_lossy(&message)
                                ),
                                Err(error) => {
                                    debug(&format!(
                                        "Failed to read window size: {error}"
                                    ));
                                }
                            }
                            let _ = stream.shutdown(Shutdown::Both);
                        },
                        Err(error) => {
                            debug(&format!(
                                "Failed to accept resize connection: {error}"
                            ));
                        }
                    }
                }
            },
            Err(error) => {
                debug(&format!(
                    "Failed to bind terminal resize vsock: CID: {}: {}",
                    defaults::GUEST_CID, error
                ));
            }
        }
    });
}

fn handle_resize_request(message: &str) {
    /*!
    Apply the window size of the given resize request

    A request which cannot be read is dropped. The session it
    belongs to keeps the window size it currently uses
    !*/
    match get_resize_request(message) {
        Some((port, lines, columns)) => {
            debug(&format!(
                "Received window size {lines}x{columns} for port {port}"
            ));
            resize_terminal_session(port, lines, columns)
        },
        None => {
            debug(&format!("Invalid resize request [skipped]: {message:?}"));
        }
    }
}

fn get_resize_request(message: &str) -> Option<(u32, u16, u16)> {
    /*!
    Read the values of the given resize request

    A request consists of the port of the session followed by the
    number of lines and columns of the caller's terminal:

    PORT LINES COLUMNS
    !*/
    let mut values = message.split_whitespace();
    let port = values.next()?.parse::<u32>().ok()?;
    let lines = values.next()?.parse::<u16>().ok()?;
    let columns = values.next()?.parse::<u16>().ok()?;
    if values.next().is_some() || lines == 0 || columns == 0 {
        return None
    }
    Some((port, lines, columns))
}

fn resize_terminal_session(port: u32, lines: u16, columns: u16) {
    /*!
    Resize the terminal of the session with the given port

    The window size is stored with the session such that it can
    also be applied to a terminal which does not exist yet. The
    caller sends its geometry when the session starts and the
    command it wants to run can still be on its way into its
    terminal at this time
    !*/
    let mut sessions = lock_terminal_sessions();
    let session = get_terminal_session(&mut sessions, port);
    session.window_size = Some((lines, columns));
    if session.fd >= 0 {
        set_terminal_window_size(session.fd, lines, columns)
    }
}

fn register_terminal_session(port: u32, fd: i32) {
    /*!
    Register the terminal of the session with the given port

    A window size which arrived before the terminal existed is
    applied to it now
    !*/
    let mut sessions = lock_terminal_sessions();
    let session = get_terminal_session(&mut sessions, port);
    session.fd = fd;
    if let Some((lines, columns)) = session.window_size {
        set_terminal_window_size(fd, lines, columns)
    }
}

fn unregister_terminal_session(port: u32) {
    /*!
    Delete the session with the given port

    The terminal of the session is about to be closed. Deleting
    the session under the lock which is also held for the time of
    a resize makes sure the descriptor cannot be used any more
    when this function returns
    !*/
    lock_terminal_sessions().retain(|session| session.port != port)
}

fn get_terminal_session(
    sessions: &mut Vec<TerminalSession>, port: u32
) -> &mut TerminalSession {
    // Provide the session with the given port and create it
    // if it does not exist yet
    match sessions.iter().position(|session| session.port == port) {
        Some(index) => &mut sessions[index],
        None => {
            sessions.push(
                TerminalSession { port, fd: -1, window_size: None }
            );
            sessions.last_mut().unwrap()
        }
    }
}

fn lock_terminal_sessions() -> MutexGuard<'static, Vec<TerminalSession>> {
    // Provide access to the list of the terminal sessions.
    // A poisoned lock is taken over because the list stays usable
    TERMINAL_SESSIONS.lock().unwrap_or_else(|error| error.into_inner())
}

fn start_child_reaper() {
    /*!
    Start reaping of terminated child processes

    sci runs as process ID 1 in the instance. Any process whose
    parent has terminated gets re-parented to sci and stays in the
    process list as a defunct(zombie) process until someone reads
    its exit status. As sci has not spawned these processes it
    also does not wait for them and it is therefore up to the
    reaper to clean them up.

    The reaper looks for terminated processes on a regular base.
    It deliberately does not use SIGCHLD to get informed about
    them. Receiving the signal synchronously would require to
    block it, but the signal mask is inherited by all processes
    started from here and would e.g. break the job control of a
    shell in the instance. Handling the signal asynchronously in
    turn would interrupt the data transfer loops with EINTR
    !*/
    thread::spawn(|| {
        let reap_interval = time::Duration::from_millis(
            defaults::REAP_INTERVAL_MSEC
        );
        loop {
            reap_child_processes();
            thread::sleep(reap_interval)
        }
    });
}

fn reap_child_processes() {
    /*!
    Read the exit status of terminated child processes

    Processes for which sci waits somewhere else are left alone,
    their exit status is expected to be read by that place. Such a
    process stays in the list of terminated processes and hides
    the ones behind it. Therefore this run ends when a process
    shows up again and the hidden ones are taken care of by one
    of the next runs
    !*/
    let mut checked_pids: Vec<i32> = Vec::new();
    while let Some(pid) = get_terminated_child() {
        if checked_pids.contains(&pid) {
            break
        }
        checked_pids.push(pid);
        if is_owned_child(pid) {
            continue
        }
        // Reading the exit status removes the process from
        // the process list
        if unsafe {
            libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG)
        } > 0 {
            debug(&format!("Reaped process: {pid}"));
        }
    }
}

fn get_terminated_child() -> Option<i32> {
    /*!
    Provide the process ID of a terminated child process

    The exit status of the process is explicitly not read such
    that the process is still available to be waited for
    !*/
    let mut process_info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    let result = unsafe {
        libc::waitid(
            libc::P_ALL, 0, process_info.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT
        )
    };
    if result == -1 {
        // No child processes at all
        return None
    }
    let process_info = unsafe { process_info.assume_init() };
    let pid = unsafe { process_info.si_pid() };
    if pid == 0 {
        // No child process has terminated
        return None
    }
    Some(pid)
}

fn spawn_child(call: &mut Command) -> std::io::Result<Child> {
    /*!
    Spawn the given command as a child process of sci

    The process is created while the list of the child processes
    sci waits for is locked. This makes sure the reaper cannot
    read the exit status of the new process before it is
    registered in that list
    !*/
    let mut owned_children = lock_owned_children();
    let child = call.spawn();
    if let Ok(child) = &child {
        owned_children.push(child.id() as i32)
    }
    child
}

fn run_child(call: &mut Command) -> std::io::Result<ExitStatus> {
    /*!
    Run the given command and wait for it to terminate
    !*/
    let mut child = spawn_child(call)?;
    let status = child.wait();
    disown_child(child.id() as i32);
    status
}

fn lock_owned_children() -> MutexGuard<'static, Vec<i32>> {
    // Provide access to the list of child processes sci waits for.
    // A poisoned lock is taken over because the list stays usable
    OWNED_CHILDREN.lock().unwrap_or_else(|error| error.into_inner())
}

fn disown_child(pid: i32) {
    // Delete the given process ID from the list of child
    // processes sci waits for
    lock_owned_children().retain(|owned_pid| *owned_pid != pid)
}

fn is_owned_child(pid: i32) -> bool {
    // Check if sci waits for the given process ID somewhere
    lock_owned_children().contains(&pid)
}

fn redirect_command(
    command: &[String], stream: vsock::VsockStream, port: u32
) {
    // start the given command as a child process in a new PTY
    // or on raw channels if no pseudo terminal can be allocated
    // connect its standard channels to the stream
    // transfer all channel data when there is data as long as the child exists
    //
    // The terminal fork is done while the list of the child
    // processes sci waits for is locked. This makes sure the
    // reaper cannot read the exit status of the new process
    // before it is registered in that list
    let fork_result = {
        let mut owned_children = lock_owned_children();
        let fork_result = Fork::from_ptmx();
        if let Ok(Fork::Parent(pid, _)) = fork_result {
            owned_children.push(pid)
        }
        fork_result
    };
    match fork_result {
        Ok(fork) => {
            redirect_command_to_pty(command, stream, fork, port)
        },
        Err(error) => {
            debug(&format!(
                "Terminal allocation failed, using raw channels: {error:?}"
            ));
            redirect_command_to_raw_channels(command, stream);
            // there is no terminal in this session, drop a window
            // size which arrived for it
            unregister_terminal_session(port)
        }
    }
}

fn setup_terminal_environment() {
    /*!
    Provide a terminal type in the environment

    The environment of sci is created from the kernel commandline
    and therefore normally does not provide a TERM setting. Shells
    like bash switch off their line editor (readline) if the
    terminal type is unset or set to 'dumb'. Without the line
    editor there is no tab completion and no history handling for
    the caller. The terminal type of the caller can be handed over
    through the sci_term=... boot parameter and defaults to
    defaults::TERM_TYPE
    !*/
    let term = env::var("TERM").unwrap_or_default();
    if term.is_empty() {
        let mut term_type = env::var("sci_term").unwrap_or_default();
        if term_type.is_empty() {
            term_type = defaults::TERM_TYPE.to_string()
        }
        term_type = get_supported_terminal_type(&term_type);
        debug(&format!("Setting terminal type to: {term_type}"));
        env::set_var("TERM", term_type);
    }
}

fn get_supported_terminal_type(term_type: &str) -> String {
    /*!
    Provide a terminal type the guest has a terminfo entry for

    The line editor of e.g a shell reads the capabilities of the
    terminal from the terminfo database to be able to move the
    cursor. If the guest image does not provide an entry for the
    terminal type of the caller, keys like the cursor keys are
    still read but the cursor can no longer be placed correctly.
    In this case fall back to a simpler terminal type the guest
    provides an entry for
    !*/
    if has_terminfo_entry(term_type) {
        return term_type.to_string()
    }
    for fallback_term_type in defaults::TERM_TYPE_FALLBACK.iter() {
        if has_terminfo_entry(fallback_term_type) {
            debug(&format!(
                "No terminfo entry for {term_type}, using {fallback_term_type}"
            ));
            return fallback_term_type.to_string()
        }
    }
    // No terminfo database in the guest, stay with the caller's setting
    debug("No terminfo database found");
    term_type.to_string()
}

fn has_terminfo_entry(term_type: &str) -> bool {
    // Check if the terminfo database of the guest provides an entry
    // for the given terminal type. Entries are stored in a directory
    // named after the first character of the terminal type, either
    // as the character itself or as its hex representation
    let first_char = match term_type.chars().next() {
        Some(first_char) => first_char,
        None => return false
    };
    let entry_dirs = [
        first_char.to_string(), format!("{:x}", first_char as u32)
    ];
    for terminfo_dir in defaults::TERMINFO_DIRS.iter() {
        for entry_dir in entry_dirs.iter() {
            let entry = format!("{terminfo_dir}/{entry_dir}/{term_type}");
            if Path::new(&entry).exists() {
                debug(&format!("Found terminfo entry: {entry}"));
                return true
            }
        }
    }
    false
}

fn set_interactive_terminal_flags(fd: i32) {
    /*!
    Setup the given terminal for interactive use

    Keep the standard line discipline of the terminal switched on
    such that the line editor of an interactive command, e.g the
    tab completion of a shell, stays in control of the input
    handling and echoes back what it has read. The terminal of the
    caller is switched to raw mode by the pilot, thus every single
    key stroke, including TAB, arrives here unmodified
    !*/
    match Termios::from_fd(fd) {
        Ok(mut termios) => {
            termios.c_lflag |= ECHO | ECHOE | ECHOK | ICANON | ISIG | IEXTEN;
            termios.c_iflag |= ICRNL;
            termios.c_oflag |= OPOST | ONLCR;
            match tcsetattr(fd, TCSANOW, &termios) {
                Ok(_) => {}
                Err(error) => {
                    debug(&format!("tcsetattr failed with: {error}"));
                }
            }
        },
        Err(error) => {
            debug(&format!(
                "Term I/O failed with: {error}"
            ));
        }
    }
    set_terminal_size(fd)
}

fn set_terminal_size(fd: i32) {
    /*!
    Set the window size of the given terminal

    A newly allocated pseudo terminal comes with no window size
    assigned. The line editor needs the size of the caller's
    terminal to be able to redraw the input line and to arrange
    the list of tab completion matches in columns. The size of the
    caller's terminal can be handed over through the sci_lines=...
    and sci_columns=... boot parameters and defaults to
    defaults::TERM_LINES x defaults::TERM_COLUMNS

    These parameters provide the geometry of the caller's terminal
    at the time the instance was started. A resize of that terminal
    afterwards is handed over through the resize listener
    !*/
    set_terminal_window_size(
        fd,
        get_terminal_size_value("sci_lines", defaults::TERM_LINES),
        get_terminal_size_value("sci_columns", defaults::TERM_COLUMNS)
    )
}

fn set_terminal_window_size(fd: i32, lines: u16, columns: u16) {
    /*!
    Set the given window size on the given terminal

    Beside of storing the new geometry the kernel sends SIGWINCH
    to the foreground process group of the terminal. The line
    editor of an interactive command re-reads the window size on
    this signal and redraws the input line to match it
    !*/
    let window_size = libc::winsize {
        ws_row: lines,
        ws_col: columns,
        ws_xpixel: 0,
        ws_ypixel: 0
    };
    let result = unsafe {
        libc::ioctl(
            fd, libc::TIOCSWINSZ as _, &window_size as *const libc::winsize
        )
    };
    if result == -1 {
        debug(&format!(
            "Failed to set terminal size: {}",
            std::io::Error::last_os_error()
        ));
    } else {
        debug(&format!("Terminal size set to {lines}x{columns}"));
    }
}

fn get_terminal_size_value(name: &str, default_value: u16) -> u16 {
    // Read a terminal geometry value from the given environment
    // variable and fall back to the given default value
    match env::var(name).unwrap_or_default().parse::<u16>() {
        Ok(value) if value > 0 => value,
        _ => default_value
    }
}

fn set_output_terminal_flags(fd: i32) {
    // Disable echo and canonical mode on stdout
    match Termios::from_fd(fd) {
        Ok(mut termios) => {
            termios.c_lflag &= !(ECHO | ECHONL | ICANON | ISIG | IEXTEN);
            match tcsetattr(fd, TCSANOW, &termios) {
                Ok(_) => {}
                Err(error) => {
                    debug(&format!("tcsetattr failed with: {error}"));
                }
            }
        },
        Err(error) => {
            debug(&format!(
                "Term I/O failed with: {error}"
            ));
        }
    }
}

fn get_echo_data(data: &[u8]) -> Vec<u8> {
    /*!
    Provide the readable part of the given input data

    On raw channels there is no terminal and no line editor which
    could interpret the input. Control sequences, e.g the ones sent
    by the cursor keys, would show up as unreadable characters if
    they were echoed back. Thus only printable characters and the
    line break are echoed to make typing visible
    !*/
    data.iter().filter(|byte|
        matches!(**byte, b'\r' | b'\n' | b'\t' | 0x20..=0x7e)
    ).cloned().collect()
}

fn redirect_command_to_raw_channels(
    command: &[String], mut stream: vsock::VsockStream
) {
    let (program, call_args) = match command.split_first() {
        Some(call) => call,
        None => {
            debug("No command to execute specified");
            return
        }
    };
    let mut call = Command::new(program);
    call
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    call.args(call_args);
    debug(&format!(
        "SCI CALL: {} -> {:?}", program, call.get_args()
    ));
    match spawn_child(&mut call) {
        Ok(mut child) => {
            // access useful I/O and file descriptors
            let stdin = child.stdin.as_mut().unwrap();
            let stdout = child.stdout.as_mut().unwrap();
            let stderr = child.stderr.as_mut().unwrap();

            let stream_fd = stream.as_raw_fd();
            let stdout_fd = stdout.as_raw_fd();
            let stderr_fd = stderr.as_raw_fd();

            set_output_terminal_flags(stdout_fd);

            // main send/recv loop
            let mut buffer = [0_u8; 1];
            loop {
                // prepare file descriptors to be watched for by select()
                let raw_fdset = std::mem::MaybeUninit::<libc::fd_set>::uninit();
                let mut fdset = unsafe { raw_fdset.assume_init() };
                let mut max_fd = -1;
                unsafe { libc::FD_ZERO(&mut fdset) };
                unsafe { libc::FD_SET(stdout_fd, &mut fdset) };
                max_fd = std::cmp::max(max_fd, stdout_fd);
                unsafe { libc::FD_SET(stderr_fd, &mut fdset) };
                max_fd = std::cmp::max(max_fd, stderr_fd);
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
                if unsafe { libc::FD_ISSET(stdout_fd, &fdset) } {
                    // something new happened on stdout,
                    // try to receive some bytes an send them through the stream
                    if let Ok(sz_r) = stdout.read(&mut buffer) {
                        if sz_r == 0 {
                            debug("EOF detected on stdout");
                            break;
                        }
                        if stream.write_all(&buffer[0..sz_r]).is_err() {
                            debug("write failure on stream");
                            break;
                        }
                    } else {
                        debug("read failure on process stdout");
                        break;
                    }
                }
                if unsafe { libc::FD_ISSET(stderr_fd, &fdset) } {
                    // something new happened on stderr,
                    // try to receive some bytes an send them through the stream
                    if let Ok(sz_r) = stderr.read(&mut buffer) {
                        if sz_r == 0 {
                            debug("EOF detected on stderr");
                            break;
                        }
                        if stream.write_all(&buffer[0..sz_r]).is_err() {
                            debug("write failure on stream");
                            break;
                        }
                    } else {
                        debug("read failure on process stderr");
                        break;
                    }
                }
                if unsafe { libc::FD_ISSET(stream_fd, &fdset) } {
                    // something new happened on the stream
                    // try to receive some bytes an send them on stdin
                    if let Ok(sz_r) = stream.read(&mut buffer) {
                        if sz_r == 0 {
                            debug("EOF detected on stream");
                            break;
                        }
                        // On raw channels there is no terminal which
                        // could echo back the input. As the caller's
                        // terminal is in raw mode and no longer echoes
                        // locally, send the readable input back to
                        // make typing visible
                        let echo_data = get_echo_data(&buffer[0..sz_r]);
                        if stream.write_all(&echo_data).is_err() {
                            debug("write failure on stream");
                            break;
                        }
                        if stdin.write_all(&buffer[0..sz_r]).is_err() {
                            debug("write failure on stdin");
                            break;
                        }
                    } else {
                        debug("read failure on stream");
                        break;
                    }
                }
            }
            let _ = child.wait();
            disown_child(child.id() as i32);
        },
        Err(error) => {
            debug(&format!(
                "SCI guest command failed with: {error}"
            ));
        }
    }
}

fn redirect_command_to_pty(
    command: &[String], mut stream: vsock::VsockStream, pty_fork: Fork,
    port: u32
) {
    if let Ok(mut master) = pty_fork.is_parent() {
        let mut child_pid = -1;
        if let Fork::Parent(pid, _) = &pty_fork {
            child_pid = *pid
        }
        let stdout_fd = master.as_raw_fd();
        let stream_fd = stream.as_raw_fd();

        // Keep the line discipline of the pseudo terminal active.
        // The command in the terminal, e.g a shell, takes care for
        // reading and echoing the input which is the precondition
        // for features like tab completion to work
        set_interactive_terminal_flags(stdout_fd);

        // Follow the window size of the caller's terminal for
        // the time of the session
        register_terminal_session(port, stdout_fd);

        // main send/recv loop
        let mut buffer = [0_u8; 1];
        loop {
            // prepare file descriptors to be watched for by select()
            let raw_fdset = std::mem::MaybeUninit::<libc::fd_set>::uninit();
            let mut fdset = unsafe { raw_fdset.assume_init() };
            let mut max_fd = -1;
            unsafe { libc::FD_ZERO(&mut fdset) };
            unsafe { libc::FD_SET(stdout_fd, &mut fdset) };
            max_fd = std::cmp::max(max_fd, stdout_fd);
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
            if unsafe { libc::FD_ISSET(stdout_fd, &fdset) } {
                // something new happened on master,
                // try to receive some bytes and send them through the stream
                if let Ok(sz_r) = master.read(&mut buffer) {
                    if sz_r == 0 {
                        debug("EOF detected on stdout");
                        break;
                    }
                    if stream.write_all(&buffer[0..sz_r]).is_err() {
                        debug("write failure on stream");
                        break;
                    }
                } else {
                    debug("read failure on process stdout");
                    break;
                }
            }
            if unsafe { libc::FD_ISSET(stream_fd, &fdset) } {
                // something new happened on the stream
                // try to receive some bytes and send them to stdout
                if let Ok(sz_r) = stream.read(&mut buffer) {
                    if sz_r == 0 {
                        debug("EOF detected on stream");
                        break;
                    }
                    if master.write_all(&buffer[0..sz_r]).is_err() {
                        debug("write failure on stdin");
                        break;
                    }
                } else {
                    debug("read failure on stream");
                    break;
                }
            }
        }
        // The terminal is about to be closed, no longer resize it
        unregister_terminal_session(port);
        let _ = pty_fork.wait();
        disown_child(child_pid);
    } else {
        let (program, call_args) = match command.split_first() {
            Some(call) => call,
            None => {
                debug("No command to execute specified");
                return
            }
        };
        let mut call = Command::new(program);
        call.args(call_args);
        debug(&format!(
            "SCI CALL: {} -> {:?}", program, call.get_args()
        ));
        match call.status() {
            Ok(_) => { },
            Err(error) => {
                debug(&format!(
                    "SCI guest command failed with: {error}"
                ));
            }
        }
    }
}

fn do_reboot(ok: bool) {
    debug("Rebooting...");
    if ! ok {
        // give potential error messages some time to settle
        let some_time = time::Duration::from_millis(10);
        thread::sleep(some_time);
    }
    match force_reboot() {
        Ok(_) => { },
        Err(error) => {
            panic!("Failed to reboot: {}", error)
        }
    }
}

fn setup_resolver_link() {
    if Path::new(defaults::SYSTEMD_NETWORK_RESOLV_CONF).exists() {
        match symlink(
            defaults::SYSTEMD_NETWORK_RESOLV_CONF, "/etc/resolv.conf"
        ) {
            Ok(_) => { },
            Err(error) => {
                debug(&format!("Error creating symlink \"{} -> {}\": {:?}",
                    "/etc/resolv.conf",
                    defaults::SYSTEMD_NETWORK_RESOLV_CONF,
                    error
                ));
            }
        }
    }
}

fn start_sshd() {
    if Path::new(defaults::SSHD).exists() {
        let mut sshd = Command::new(defaults::SSHD);
        match sshd.status() {
            Ok(_) => {},
            Err(error) => {
                debug(&format!("Failed to start sshd: {error}"));
            }
        }
    }
}

fn move_mounts(new_root: &str) {
    /*!
    Move filesystems from current root to new_root
    !*/
    // /run
    let mut call = Command::new(defaults::MOUNT_TOOL);
    call.arg("--bind").arg("/run").arg(format!("{new_root}/run"));
    debug(&format!("EXEC: mount -> {:?}", call.get_args()));
    match call.status() {
        Ok(_) => debug("Bind mounted /run"),
        Err(error) => {
            debug(&format!("Failed to bind mount /run: {error}"));
            match Mount::builder()
                .fstype("tmpfs").mount("tmpfs", format!("{new_root}/run"))
            {
                Ok(_) => debug("Mounted tmpfs on /run"),
                Err(error) => {
                    debug(&format!("Failed to mount /run: {error}"));
                }
            }
        }
    }
}

fn mount_nfs_volumes() {
    /*!
    Mount the NFS volumes given in the nfs=... cmdline variable

    The variable provides a comma separated list of volumes. Each
    of them is specified in the format:

    NAME_OR_IP:/export_path:/mount_path

    A volume which cannot be read from the specification is
    skipped, all other volumes are still mounted
    !*/
    let nfs_volumes = env::var("nfs").unwrap_or_default();
    if nfs_volumes.is_empty() {
        return
    }
    for nfs_volume in nfs_volumes.split(defaults::NFS_VOLUME_DELIMITER) {
        let nfs_volume = nfs_volume.trim();
        if nfs_volume.is_empty() {
            continue
        }
        match get_nfs_volume(nfs_volume) {
            Some((source, target)) => mount_nfs_volume(source, target),
            None => {
                debug(&format!(
                    "Invalid nfs volume specification [skipped]: {nfs_volume}"
                ));
            }
        }
    }
}

fn get_nfs_volume(nfs_volume: &str) -> Option<(&str, &str)> {
    /*!
    Read source and mount point of the given NFS volume

    The mount point is the last element of the specification.
    Everything in front of it is the source of the volume, the
    export path on the server, which contains a colon itself
    !*/
    let (source, target) = nfs_volume.rsplit_once(':')?;
    if ! source.contains(':') || ! target.starts_with('/') {
        return None
    }
    Some((source, target))
}

fn mount_nfs_volume(source: &str, target: &str) {
    /*!
    Mount the given NFS source on the given mount point

    The mount point is created if it does not exist. The mount is
    done through the mount tool of the guest because an NFS mount
    requires the mount helper of the filesystem to be called
    !*/
    match fs::create_dir_all(target) {
        Ok(_) => { },
        Err(error) => {
            debug(&format!("Error creating directory {target}: {error}"));
            return
        }
    }
    let mut call = Command::new(defaults::MOUNT_TOOL);
    call.arg("-t").arg(defaults::NFS_FSTYPE).arg(source).arg(target);
    debug(&format!(
        "SCI CALL: {} -> {:?}", defaults::MOUNT_TOOL, call.get_args()
    ));
    match run_child(&mut call) {
        Ok(status) => {
            if status.success() {
                debug(&format!("Mounted {source} on {target}"))
            } else {
                debug(&format!(
                    "Failed to mount {source} on {target}: {status}"
                ))
            }
        },
        Err(error) => {
            debug(&format!("Failed to mount {source} on {target}: {error}"))
        }
    }
}

fn mount_basic_fs() {
    /*!
    Mount standard filesystems
    !*/
    match Mount::builder().fstype("proc").mount("proc", "/proc") {
        Ok(_) => debug("Mounted proc on /proc"),
        Err(error) => {
            debug(&format!("Failed to mount /proc [skipped]: {error}"));
        }
    }
    match Mount::builder().fstype("sysfs").mount("sysfs", "/sys") {
        Ok(_) => debug("Mounted sysfs on /sys"),
        Err(error) => {
            debug(&format!("Failed to mount /sys: {error}"));
        }
    }
    match Mount::builder().fstype("devtmpfs").mount("devtmpfs", "/dev") {
        Ok(_) => debug("Mounted devtmpfs on /dev"),
        Err(error) => {
            debug(&format!("Failed to mount /dev: {error}"));
        }
    }
    match Mount::builder().fstype("devpts").mount("devpts", "/dev/pts") {
        Ok(_) => debug("Mounted devpts on /dev/pts"),
        Err(error) => {
            debug(&format!("Failed to mount /dev/pts: {error}"));
        }
    }
}

fn setup_logger() {
    /*!
    Set up the logger internally
    !*/
    let env = Env::default()
        .filter_or("FLAKE_LOG_LEVEL", "trace")
        .write_style_or("FLAKE_LOG_STYLE", "always");

    env_logger::init_from_env(env);
}

#[cfg(test)]
mod tests {
    use super::get_nfs_volume;
    use super::get_resize_request;

    #[test]
    fn test_get_resize_request() {
        assert_eq!(get_resize_request("52 24 80"), Some((52, 24, 80)));
        // the caller sends the request as a line
        assert_eq!(get_resize_request("52 24 80\n"), Some((52, 24, 80)));
        // the console of the instance has no port of its own
        assert_eq!(get_resize_request("0 43 132"), Some((0, 43, 132)));
    }

    #[test]
    fn test_get_invalid_resize_request() {
        // no geometry
        assert_eq!(get_resize_request("52 24"), None);
        assert_eq!(get_resize_request("52"), None);
        assert_eq!(get_resize_request(""), None);
        // no geometry a terminal could use
        assert_eq!(get_resize_request("52 0 80"), None);
        assert_eq!(get_resize_request("52 24 0"), None);
        // not a number
        assert_eq!(get_resize_request("52 24 columns"), None);
        assert_eq!(get_resize_request("52 -1 80"), None);
        // out of range
        assert_eq!(get_resize_request("52 65536 80"), None);
        // no request at all
        assert_eq!(get_resize_request("52 24 80 rm -rf /"), None);
    }

    #[test]
    fn test_get_nfs_volume() {
        assert_eq!(
            get_nfs_volume("some.host:/host/path:/local/path"),
            Some(("some.host:/host/path", "/local/path"))
        );
        assert_eq!(
            get_nfs_volume("172.16.0.1:/host/path:/local/path"),
            Some(("172.16.0.1:/host/path", "/local/path"))
        );
        // an IPv6 address is enclosed in brackets and can be
        // told apart from the delimiter of the mount point
        assert_eq!(
            get_nfs_volume("[fd00::1]:/host/path:/local/path"),
            Some(("[fd00::1]:/host/path", "/local/path"))
        );
    }

    #[test]
    fn test_get_invalid_nfs_volume() {
        // no mount point
        assert_eq!(get_nfs_volume("some.host:/host/path"), None);
        // relative mount point
        assert_eq!(get_nfs_volume("some.host:/host/path:local"), None);
        // no export path
        assert_eq!(get_nfs_volume("some.host:/local/path"), None);
        assert_eq!(get_nfs_volume("/local/path"), None);
        assert_eq!(get_nfs_volume(""), None);
    }
}

