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
use crate::config::config_file;
use crate::config::config_from_str;
use flakes::network::get_valid_interface_name;

#[test]
fn simple_config() {
    let cfg = config_from_str(
            r#"vm:
 name: JoJo
 host_app_path: /myapp
include:
 tar: ~
"#,
    );
    assert_eq!(cfg.vm.name, "JoJo");
}

#[test]
fn combine_configs() {
    let cfg = config_from_str(
            r#"vm:
 name: JoJo
 host_app_path: /myapp
include:
 tar: ~
vm:
 name: Dio
 host_app_path: /other
"#,
    );
    assert_eq!(cfg.vm.name, "Dio");
}

fn instance_engine_section() -> crate::config::EngineSection<'static> {
    // Leaked to make the borrow of the parsed config static, the
    // same way the config singleton in config.rs is set up
    let config = Box::leak(Box::new(config_from_str(
            r#"vm:
 name: JoJo
 host_app_path: /myapp
 runtime:
  runas: root
  firecracker:
   rootfs_image_path: /rootfs
   kernel_image_path: /kernel
   boot_args:
     - "init=/usr/sbin/sci"
     - "root=/dev/vda"
     - "ip=dhcp"
     - "quiet"
   instance:
     "@one":
       boot_args:
         - "ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off"
         - "nameserver=8.8.8.8"
     "two":
       boot_args:
         - "root=/dev/vdb"
     "@none":
       boot_args: ~
include:
 tar: ~
"#,
    )));
    config.runtime().firecracker
}

#[test]
fn test_boot_args_without_instance_are_unchanged() {
    let engine_section = instance_engine_section();
    assert_eq!(
        vec!["init=/usr/sbin/sci", "root=/dev/vda", "ip=dhcp", "quiet"],
        engine_section.get_boot_args("")
    );
}

#[test]
fn test_boot_args_of_unknown_instance_are_unchanged() {
    let engine_section = instance_engine_section();
    assert_eq!(
        engine_section.boot_args, engine_section.get_boot_args("@other")
    );
    assert_eq!(
        engine_section.boot_args, engine_section.get_boot_args("@none")
    );
}

#[test]
fn test_instance_boot_args_replace_and_append() {
    let engine_section = instance_engine_section();
    // ip= takes the place of the global ip=dhcp, the
    // nameserver= option is not set globally and is appended
    assert_eq!(
        vec![
            "init=/usr/sbin/sci",
            "root=/dev/vda",
            "ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off",
            "quiet",
            "nameserver=8.8.8.8"
        ],
        engine_section.get_boot_args("@one")
    );
}

#[test]
fn test_instance_name_without_at_sign_is_accepted_as_key() {
    let engine_section = instance_engine_section();
    assert_eq!(
        vec!["init=/usr/sbin/sci", "root=/dev/vdb", "ip=dhcp", "quiet"],
        engine_section.get_boot_args("@two")
    );
}

#[test]
fn test_program_config_file() {
    let config_file = config_file("app", false);
    assert_eq!("/usr/share/flakes/app.yaml", config_file);
}

fn is_valid_interface_name(name: &str) -> bool {
    // conditions taken from dev_valid_name() in the kernel,
    // extended by the '@' character which iproute2 uses to
    // report the parent of an interface
    ! name.is_empty()
        && name.len() < flakes::defaults::IFNAMSIZ
        && name != "."
        && name != ".."
        && ! name.chars().any(
            |letter| letter == '/'
                || letter == ':'
                || letter == '@'
                || letter.is_whitespace()
        )
}

#[test]
fn test_tap_name_is_kept_if_valid() {
    assert_eq!("tap-app", get_valid_interface_name("tap-", "app"));
}

#[test]
fn test_tap_name_replaces_invalid_characters() {
    assert_eq!("tap-app_id", get_valid_interface_name("tap-", "app@id"));
    assert_eq!("tap-a_b_c_d", get_valid_interface_name("tap-", "a/b:c d"));
}

#[test]
fn test_tap_name_is_shortened() {
    let name = get_valid_interface_name(
        "tap-", "some-very-long-application-name"
    );
    assert_eq!("tap-some_bbb9de", name);
    assert!(is_valid_interface_name(&name));
}

#[test]
fn test_tap_name_of_long_names_stays_unique() {
    let name_a = get_valid_interface_name(
        "tap-", "some-very-long-application-name@a"
    );
    let name_b = get_valid_interface_name(
        "tap-", "some-very-long-application-name@b"
    );
    assert_ne!(name_a, name_b);
}

#[test]
fn test_tap_name_of_empty_name_is_valid() {
    assert!(is_valid_interface_name(&get_valid_interface_name("tap-", "")));
}

#[test]
fn test_tap_name_is_valid_for_arbitrary_names() {
    for name in [
        "", ".", "..", "app", "app@id", "/usr/bin/app", "sömé äpp",
        "abcdefghijkl", "@", "0123456789012345678901234567890123456789"
    ] {
        let interface_name = get_valid_interface_name("tap-", name);
        assert!(
            is_valid_interface_name(&interface_name),
            "invalid interface name {} from {}", interface_name, name
        );
    }
}
