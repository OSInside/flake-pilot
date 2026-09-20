//
// Copyright (c) 2026 Marcus Schäfer
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
use crate::bubblewrap::get_instance_name;
use crate::bubblewrap::get_sandbox_options;
use crate::config::config_from_str;
use crate::overlay::{get_dir, get_mount_options, get_user_dir};

use uzers::get_current_uid;

use std::env;

fn sandbox_options(
    options: Option<Vec<&str>>, workdir: &str
) -> Vec<String> {
    /*!
    Provide the bwrap options of an instance named myapp

    The root of the sandbox is the overlay mount of the rootfs
    on the host. All tests refer to the same one, thus they
    can set the variable which provides it in parallel
    !*/
    env::set_var("OVERLAYROOT", get_dir("myapp", "merged"));
    get_sandbox_options("myapp", options, workdir).unwrap()
}

fn options(arguments: &[&str]) -> Vec<String> {
    /*!
    Provide the given bwrap arguments in the format the pilot
    creates them
    !*/
    arguments.iter().map(|argument| argument.to_string()).collect()
}

fn sandbox_root_options() -> Vec<String> {
    /*!
    Provide the bwrap options which mount the root of the
    sandbox. They are the first options of every sandbox and
    refer to the directories of the overlay setup, which live
    in the overlay directory of the calling user
    !*/
    vec![
        "--overlay-src".to_string(), get_dir("myapp", "merged"),
        "--overlay".to_string(),
        get_dir("myapp", "rw"), get_dir("myapp", "work"),
        "/".to_string()
    ]
}

#[test]
fn simple_config() {
    let cfg = config_from_str(
r#"sandbox:
 name: /var/lib/flakes/leap
 host_app_path: /myapp
"#);
    assert_eq!(cfg.sandbox.name, "/var/lib/flakes/leap");
    assert_eq!(cfg.sandbox.host_app_path, "/myapp");
    assert_eq!(cfg.sandbox.target_app_path, None);
    assert_eq!(cfg.runtime().runas, "");
    assert!(cfg.runtime().bubblewrap.is_none());
}

#[test]
fn combine_configs() {
    let cfg = config_from_str(
r#"sandbox:
 name: /var/lib/flakes/leap
 host_app_path: /myapp
sandbox:
 name: /var/lib/flakes/tumbleweed
 host_app_path: /other
"#);
    assert_eq!(cfg.sandbox.name, "/var/lib/flakes/tumbleweed");
}

fn runtime_config() -> crate::config::Config<'static> {
    config_from_str(
r#"sandbox:
 name: /var/lib/flakes/leap
 target_app_path: /usr/bin/bash
 host_app_path: /myapp
 runtime:
  runas: root
  pilot_options:
    - "%interactive"
    - "port:2000"
  bubblewrap:
    - "--ro-bind /etc/resolv.conf /etc/resolv.conf"
    - "--unshare-pid"
"#)
}

#[test]
fn test_runtime_config() {
    let cfg = runtime_config();
    assert_eq!(cfg.sandbox.target_app_path, Some("/usr/bin/bash"));
    assert_eq!(cfg.runtime().runas, "root");
    assert_eq!(
        vec!["--ro-bind /etc/resolv.conf /etc/resolv.conf", "--unshare-pid"],
        cfg.runtime().bubblewrap.unwrap()
    );
}

#[test]
fn test_configured_pilot_options() {
    let cfg = runtime_config();
    assert_eq!(vec!["%interactive", "port:2000"], cfg.pilot_options());
    // an option can be configured with or without the '%' marker
    // and is provided with the marker in both cases
    let pilot_options = flakes::lookup::Lookup::get_pilot_run_options(
        cfg.pilot_options()
    );
    assert_eq!(Some(&"".to_string()), pilot_options.get("%interactive"));
    assert_eq!(Some(&"2000".to_string()), pilot_options.get("%port"));
}

#[test]
fn test_no_pilot_options_configured() {
    let cfg = config_from_str(
r#"sandbox:
 name: /var/lib/flakes/leap
 host_app_path: /myapp
"#);
    assert!(cfg.pilot_options().is_empty());
}

#[test]
fn test_default_sandbox_options() {
    // the root of the sandbox is the overlay of the rootfs and
    // the default setup applies if the flake configures no options
    let mut expected = sandbox_root_options();
    expected.extend(options(&[
        "--dev", "/dev",
        "--proc", "/proc",
        "--tmpfs", "/tmp",
        "--unshare-pid",
        "--die-with-parent",
        "--chdir", "/"
    ]));
    assert_eq!(expected, sandbox_options(None, "/"));
}

#[test]
fn test_configured_sandbox_options() {
    // an option configured together with its values is passed
    // on as separate arguments and is added after the options
    // which mount the root of the sandbox
    let mut expected = sandbox_root_options();
    expected.extend(options(&[
        "--ro-bind", "/etc/resolv.conf", "/etc/resolv.conf",
        "--unshare-all",
        "--chdir", "/"
    ]));
    assert_eq!(
        expected,
        sandbox_options(
            Some(vec![
                "--ro-bind /etc/resolv.conf /etc/resolv.conf",
                "--unshare-all"
            ]),
            "/"
        )
    );
}

#[test]
fn test_configured_overlay_sources() {
    // a source configured for the root of the sandbox is stacked
    // on top of the overlay of the rootfs. The sources have to be
    // given before the mount they belong to, they are therefore
    // moved in front of it, in the configured order
    let mut expected = vec![
        "--overlay-src".to_string(), get_dir("myapp", "merged"),
        "--overlay-src".to_string(), "/data/one".to_string(),
        "--overlay-src".to_string(), "/data/two".to_string(),
        "--overlay".to_string(),
        get_dir("myapp", "rw"), get_dir("myapp", "work"),
        "/".to_string()
    ];
    expected.extend(options(&["--unshare-all", "--chdir", "/"]));
    assert_eq!(
        expected,
        sandbox_options(
            Some(vec![
                "--overlay-src /data/one",
                "--unshare-all",
                "--overlay-src /data/two"
            ]),
            "/"
        )
    );
}

#[test]
fn test_configured_working_directory_is_kept() {
    // no default working directory is added if the flake
    // configures one
    let mut expected = sandbox_root_options();
    expected.extend(options(&["--chdir", "/data"]));
    assert_eq!(expected, sandbox_options(Some(vec!["--chdir /data"]), "/"));
}

#[test]
fn test_requested_working_directory() {
    // the working directory requested through the %chdir pilot
    // option is used instead of the root of the sandbox
    let mut expected = sandbox_root_options();
    expected.extend(options(&["--unshare-all", "--chdir", "/data"]));
    assert_eq!(
        expected, sandbox_options(Some(vec!["--unshare-all"]), "/data")
    );
}

#[test]
fn test_variable_expansion_of_sandbox_options() {
    // a variable which is not set in the environment is
    // provided as a shell style variable reference
    let mut expected = sandbox_root_options();
    expected.extend(
        options(&["--bind", "$FLAKETESTVARIABLE", "/data", "--chdir", "/"])
    );
    assert_eq!(
        expected,
        sandbox_options(Some(vec!["--bind %FLAKETESTVARIABLE /data"]), "/")
    );
}

#[test]
fn test_instance_name() {
    // the name of the instance is the name of the flake command
    // if it was called without @NAME arguments
    assert_eq!("myapp", get_instance_name("myapp"));
}

#[test]
fn test_overlay_user_dir() {
    // the overlay setups of a user are created in a directory
    // of their own, named after the ID of that user
    assert_eq!(
        format!("/var/tmp/bwrap_{}", get_current_uid()), get_user_dir()
    );
}

#[test]
fn test_overlay_dir() {
    // all directories of the overlay setup are named after the
    // instance they belong to and live in the overlay directory
    // of the calling user
    assert_eq!(
        format!("{}/myapp@one_merged", get_user_dir()),
        get_dir("myapp@one", "merged")
    );
}

#[test]
fn test_overlay_mount_options() {
    // the rootfs is the read only layer of the overlay, the
    // data written to it is kept in the tmpfs of the instance
    let tmpfs_dir = get_dir("myapp", "overlay");
    assert_eq!(
        format!(
            "lowerdir=/var/lib/flakes/leap,\
            upperdir={tmpfs_dir}/upper,\
            workdir={tmpfs_dir}/work"
        ),
        get_mount_options("myapp", "/var/lib/flakes/leap")
    );
}
