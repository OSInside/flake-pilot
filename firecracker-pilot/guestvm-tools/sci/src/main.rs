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

use crate::defaults::debug;

fn main() {
    /*!
    Simple Command Init (sci) is a tool which executes the provided
    command in the run=... cmdline variable or through a vsock
    after preparation of an execution environment for the purpose to
    run a command inside of a firecracker instance.

    if provided via the overlay_root=/dev/block_device kernel boot
    parameter, sci also prepares the root filesystem as an overlay
    using the given block device for writing.
    !*/
    setup_logger();

    let mut args: Vec<String> = vec![];
    let mut call: Command;
    let mut do_exec = false;
    let mut ok = true;

    // print user space env
    for (key, value) in env::vars() {
        debug(&format!("{}: {}", key, value));
    }

    // parse commandline from run environment variable
    match env::var("run").ok() {
        Some(call_cmd) => {
            match shell_words::split(&call_cmd) {
                Ok(call_params) => {
                    args = call_params
                },
                Err(error) => {
                    debug(&format!("Failed to parse {}: {}", call_cmd, error));
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
                    debug(&format!("Loading overlay module failed: {}", error));
                }
            }
            debug(&format!("Mounting overlayfs RW({})", overlay.as_str()));
            match Mount::builder()
                .fstype("ext2").mount(overlay.as_str(), "/overlayroot")
            {
                Ok(_) => {
                    debug(&format!("Mounted {:?} on /overlayroot", overlay));
                    ok = true
                },
                Err(error) => {
                    debug(&format!("Failed to mount overlayroot: {}", error));
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
                            "Failed to mount overlayroot: {}", error
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
                            "Failed to change working directory: {}", error
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
                            debug(&format!("Failed to pivot_root: {}", error));
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

    // Setup command call parameters
    for arg in &args[1..] {
        call.arg(arg);
    }

    // Perform execution tasks
    if ! ok {
        do_reboot(ok)
    }
    match env::var("sci_resume").ok() {
        Some(_) => {
            // resume mode; check if vhost transport is loaded
            let mut modprobe = Command::new(defaults::PROBE_MODULE);
            modprobe.arg(defaults::VHOST_TRANSPORT);
            debug(&format!(
                "SCI CALL: {} -> {:?}", defaults::PROBE_MODULE, modprobe.get_args()
            ));
            match modprobe.status() {
                Ok(_) => { },
                Err(error) => {
                    debug(&format!(
                        "Loading {} module failed: {}",
                        defaults::VHOST_TRANSPORT, error
                    ));
                }
            }
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
                                            "Failed to read data {}", error
                                        ));
                                    }
                                };
                                stream.shutdown(Shutdown::Both).unwrap();
                                if call_str.is_empty() {
                                    // Caused by handshake check from the
                                    // pilot, if the vsock connection between
                                    // guest and host can be established
                                    continue
                                }
                                debug(&format!(
                                    "SCI CALL RAW BUF: {:?}", call_str
                                ));
                                let mut call_stack: Vec<&str> =
                                    call_str.split(' ').collect();
                                let exec_port = call_stack.pop().unwrap();
                                let exec_cmd = call_stack.join(" ");
                                let mut exec_port_num = 0;
                                match exec_port.parse::<u32>() {
                                    Ok(num) => { exec_port_num = num },
                                    Err(error) => {
                                        debug(&format!(
                                            "Failed to parse port: {}: {}",
                                            exec_port, error
                                        ));
                                    }
                                }
                                debug(&format!(
                                    "CALL SCI: {}", exec_cmd
                                ));

                                // Establish a VSOCK connection with the farend
                                thread::spawn(move || {
                                    match VsockStream::connect_with_cid_port(
                                        2, exec_port_num
                                    ) {
                                        Ok(vsock_stream) => {
                                            redirect_command(
                                                &exec_cmd, vsock_stream
                                            );
                                        },
                                        Err(error) => {
                                            debug(&format!(
                                                "VSOCK-CONNECT failed with: {}",
                                                error
                                            ));
                                        }
                                    }
                                });
                            },
                            Err(error) => {
                                debug(&format!(
                                    "Failed to accept incoming connection: {}",
                                    error
                                ));
                            }
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
        },
        None => {
            // run regular command and close vm
            if do_exec {
                // replace ourselves
                debug(&format!("EXEC: {} -> {:?}", &args[0], call.get_args()));
                call.exec();
            } else {
                // call a command and keep control
                debug(&format!(
                    "SCI CALL: {} -> {:?}", &args[0], call.get_args()
                ));
                let _ = call.status();
            }
        }
    }
    
    // Close firecracker session
    do_reboot(ok)
}

fn redirect_command(command: &str, stream: vsock::VsockStream) {
    // start the given command as a child process in a new PTY
    // or on raw channels if no pseudo terminal can be allocated
    // connect its standard channels to the stream
    // transfer all channel data when there is data as long as the child exists
    match Fork::from_ptmx() {
        Ok(fork) => {
            redirect_command_to_pty(command, stream, fork)
        },
        Err(error) => {
            debug(&format!(
                "Terminal allocation failed, using raw channels: {:?}", error
            ));
            redirect_command_to_raw_channels(command, stream)
        }
    }
}

fn redirect_command_to_raw_channels(
    command: &str, mut stream: vsock::VsockStream
) {
    let mut call_args: Vec<&str> = command.split(' ').collect();
    let program = call_args.remove(0);
    let mut call = Command::new(program);
    call
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for arg in call_args {
        call.arg(arg);
    }
    debug(&format!(
        "SCI CALL: {} -> {:?}", program, call.get_args()
    ));
    match call.spawn() {
        Ok(mut child) => {
            // access useful I/O and file descriptors
            let stdin = child.stdin.as_mut().unwrap();
            let stdout = child.stdout.as_mut().unwrap();
            let stderr = child.stderr.as_mut().unwrap();

            let stream_fd = stream.as_raw_fd();
            let stdout_fd = stdout.as_raw_fd();
            let stderr_fd = stderr.as_raw_fd();
            // main send/recv loop
            let mut buffer = [0_u8; 100];
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
        },
        Err(error) => {
            debug(&format!(
                "SCI guest command failed with: {}", error
            ));
        }
    }
}

fn redirect_command_to_pty(
    command: &str, mut stream: vsock::VsockStream, pty_fork: Fork
) {
    if let Ok(mut master) = pty_fork.is_parent() {
        let stdout_fd = master.as_raw_fd();
        let stream_fd = stream.as_raw_fd();

        // main send/recv loop
        let mut buffer = [0_u8; 100];
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
                // try to receive some bytes an send them through the stream
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
                // try to receive some bytes an send them on stdin
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
        let _ = pty_fork.wait();
    } else {
        let mut call_args: Vec<&str> = command.split(' ').collect();
        let program = call_args.remove(0);
        let mut call = Command::new(program);
        for arg in call_args {
            call.arg(arg);
        }
        debug(&format!(
            "SCI CALL: {} -> {:?}", program, call.get_args()
        ));
        match call.status() {
            Ok(_) => { },
            Err(error) => {
                debug(&format!(
                    "SCI guest command failed with: {}", error
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

fn move_mounts(new_root: &str) {
    /*!
    Move filesystems from current root to new_root
    !*/
    // /run
    let mut call = Command::new("mount");
    call.arg("--bind").arg("/run").arg(&format!("{}/run", new_root));
    debug(&format!("EXEC: mount -> {:?}", call.get_args()));
    match call.status() {
        Ok(_) => debug("Bind mounted /run"),
        Err(error) => {
            debug(&format!("Failed to bind mount /run: {}", error));
            match Mount::builder()
                .fstype("tmpfs").mount("tmpfs", format!("{}/run", new_root))
            {
                Ok(_) => debug("Mounted tmpfs on /run"),
                Err(error) => {
                    debug(&format!("Failed to mount /run: {}", error));
                }
            }
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
            debug(&format!("Failed to mount /proc [skipped]: {}", error));
        }
    }
    match Mount::builder().fstype("sysfs").mount("sysfs", "/sys") {
        Ok(_) => debug("Mounted sysfs on /sys"),
        Err(error) => {
            debug(&format!("Failed to mount /sys: {}", error));
        }
    }
    match Mount::builder().fstype("devtmpfs").mount("devtmpfs", "/dev") {
        Ok(_) => debug("Mounted devtmpfs on /dev"),
        Err(error) => {
            debug(&format!("Failed to mount /dev: {}", error));
        }
    }
    match Mount::builder().fstype("devpts").mount("devpts", "/dev/pts") {
        Ok(_) => debug("Mounted devpts on /dev/pts"),
        Err(error) => {
            debug(&format!("Failed to mount /dev/pts: {}", error));
        }
    }
}

fn setup_logger() {
    /*!
    Set up the logger internally
    !*/
    let env = Env::default()
        .filter_or("MY_LOG_LEVEL", "trace")
        .write_style_or("MY_LOG_STYLE", "always");

    env_logger::init_from_env(env);
}
