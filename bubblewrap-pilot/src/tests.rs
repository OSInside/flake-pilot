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
use crate::bubblewrap::{
    check_persist_dirs, get_engine_arguments, get_sandbox_options
};
use crate::config::config_from_str;
use crate::overlay::{get_dir, get_user_dir, mounts_below};

use uzers::get_current_uid;

use std::env;
use std::path::Path;

fn sandbox_setup(
    options: Option<Vec<&str>>, workdir: &str
) -> (Vec<String>, Vec<String>) {
    /*!
    Provide the layers and the bwrap options of an instance named
    myapp

    The root of the sandbox is the overlay mount of the rootfs
    in the namespace of the sandbox. All tests refer to the same
    one, thus they can set the variable which provides it in
    parallel
    !*/
    env::set_var("OVERLAYROOT", get_dir("merged"));
    let (layers, _, engine_arguments) = get_engine_arguments(options).unwrap();
    (
        layers,
        get_sandbox_options(
            &get_dir("merged"), engine_arguments, workdir
        )
    )
}

fn sandbox_options(
    options: Option<Vec<&str>>, workdir: &str
) -> Vec<String> {
    /*!
    Provide the bwrap options of an instance named myapp
    !*/
    sandbox_setup(options, workdir).1
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
    bind the overlay of the rootfs writable, which lives in the
    overlay directory of the calling user
    !*/
    vec!["--bind".to_string(), get_dir("merged"), "/".to_string()]
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
        "--unshare-ipc",
        "--unshare-uts",
        "--new-session",
        "--die-with-parent",
        "--clearenv"
    ]));
    // the environment is cleared, only the variables of the allow
    // list which are set for the caller are set again
    for name in ["PATH", "HOME", "USER", "LOGNAME", "TERM", "LANG", "LC_ALL", "TZ"] {
        if let Ok(value) = env::var(name) {
            expected.extend(options(&["--setenv", name, &value]));
        }
    }
    expected.extend(options(&["--chdir", "/"]));
    let (layers, sandbox_options) = sandbox_setup(None, "/");
    assert!(layers.is_empty());
    assert_eq!(expected, sandbox_options);
    // a variable of the caller outside of the allow list is not set
    assert!(! sandbox_options.iter().any(|option| option == "OVERLAYROOT"));
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
fn test_cleared_environment_of_configured_options() {
    // a configuration which clears the environment gets the allow
    // list as well, a variable it sets itself is kept as configured.
    // The environment is only read, other tests run in parallel
    let cleared = sandbox_options(
        Some(vec!["--clearenv", "--setenv HOME /home/sandbox"]), "/"
    );
    let setenv: Vec<(&str, &str)> = cleared.windows(3)
        .filter(|args| args[0] == "--setenv")
        .map(|args| (args[1].as_str(), args[2].as_str()))
        .collect();
    assert!(setenv.contains(&("HOME", "/home/sandbox")));
    assert_eq!(1, setenv.iter().filter(|(name, _)| *name == "HOME").count());
    if let Ok(path) = env::var("PATH") {
        assert!(setenv.contains(&("PATH", path.as_str())));
    }
    // without --clearenv the environment of the caller is used and
    // nothing is set
    assert!(
        ! sandbox_options(Some(vec!["--unshare-pid"]), "/")
            .iter().any(|option| option == "--setenv")
    );
}

#[test]
fn test_configured_overlay_sources() {
    // a source configured for the root of the sandbox is a layer
    // stacked on top of the rootfs, in the configured order. The
    // layers are part of the overlay on the host and are not
    // passed to bwrap
    let mut expected = sandbox_root_options();
    expected.extend(options(&["--unshare-all", "--chdir", "/"]));
    let (layers, sandbox_options) = sandbox_setup(
        Some(vec![
            "--overlay-src /data/one",
            "--unshare-all",
            "--overlay-src /data/two"
        ]),
        "/"
    );
    assert_eq!(options(&["/data/one", "/data/two"]), layers);
    assert_eq!(expected, sandbox_options);
}

#[test]
fn test_overlay_source_configured_without_its_value() {
    // an option can be configured in an entry of its own, the
    // source which follows it is the layer
    let mut expected = sandbox_root_options();
    expected.extend(options(&["--unshare-all", "--chdir", "/"]));
    let (layers, sandbox_options) = sandbox_setup(
        Some(vec!["--unshare-all", "--overlay-src", "/data/one"]), "/"
    );
    assert_eq!(options(&["/data/one"]), layers);
    assert_eq!(expected, sandbox_options);
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
    // the overlay setup of a user is mounted on a directory of
    // its own, named after the ID of that user
    assert_eq!(
        format!("/var/tmp/bwrap_{}", get_current_uid()), get_user_dir()
    );
}

#[test]
fn test_overlay_dir() {
    // all directories of the overlay setup live in the tmpfs
    // mounted on the directory of the calling user
    assert_eq!(format!("{}/merged", get_user_dir()), get_dir("merged"));
}

#[test]
fn test_mounts_below() {
    let mountinfo = "\
        22 1 0:21 / / rw,relatime shared:1 - btrfs /dev/vda2 rw\n\
        23 22 0:5 / /proc rw shared:2 - proc proc rw\n\
        24 22 0:22 / /srv/rootfs rw shared:3 - btrfs /dev/vda2 rw\n\
        25 22 0:23 / /data/my\\040tree/proc rw shared:4 - proc proc rw\n";
    // the root of the host has mount points below it
    assert!(mounts_below(Path::new("/"), mountinfo));
    // a mount on the directory itself does not count
    assert!(! mounts_below(Path::new("/srv/rootfs"), mountinfo));
    assert!(! mounts_below(Path::new("/srv/other"), mountinfo));
    // a path prefix is no parent directory
    assert!(! mounts_below(Path::new("/pro"), mountinfo));
    // escaped mount points are resolved
    assert!(mounts_below(Path::new("/data/my tree"), mountinfo));
    assert!(! mounts_below(Path::new("/data/my"), mountinfo));
}

#[test]
fn test_configured_persist_mounts() {
    let (layers, persist_dirs, engine_arguments) = get_engine_arguments(Some(vec![
        "--persist-mount /mnt",
        "--dev-bind /dev /dev",
        "--persist-mount /run/media"
    ])).unwrap();
    assert!(layers.is_empty());
    assert_eq!(options(&["/mnt", "/run/media"]), persist_dirs);
    // the directories are handed to the helper, not to bwrap
    assert_eq!(options(&["--dev-bind", "/dev", "/dev"]), engine_arguments);
    assert!(check_persist_dirs(&persist_dirs, &engine_arguments).is_ok());
}

#[test]
fn test_invalid_persist_mounts() {
    let arguments = options(&[
        "--bind", "/run", "/run", "--tmpfs", "/mnt/scratch",
        "--size", "1000", "--dev", "/dev"
    ]);
    // the root and relative paths are refused
    assert!(check_persist_dirs(&options(&["/"]), &[]).is_err());
    assert!(check_persist_dirs(&options(&["mnt"]), &[]).is_err());
    // bwrap must not mount at or below the directory
    assert!(check_persist_dirs(&options(&["/mnt"]), &arguments).is_err());
    assert!(check_persist_dirs(&options(&["/dev"]), &arguments).is_err());
    assert!(check_persist_dirs(&options(&["/run"]), &arguments).is_err());
    // a mount above the directory is fine, so is a path prefix
    assert!(check_persist_dirs(&options(&["/run/media"]), &arguments).is_ok());
    assert!(check_persist_dirs(&options(&["/mn"]), &arguments).is_ok());
    assert!(check_persist_dirs(&options(&["/srv"]), &arguments).is_ok());
}
