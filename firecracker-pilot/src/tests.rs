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
use crate::firecracker::has_network_setup;
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
fn test_network_setup_from_boot_args() {
    // a registration with '--no-net' and a flake whose setup was
    // deleted are left without an ip= option
    assert!(! has_network_setup(&["init=/usr/sbin/sci", "quiet"]));
    assert!(has_network_setup(&["ip=dhcp"]));
    assert!(has_network_setup(
        &["ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off"]
    ));
    // an explicitly switched off network is no network either
    assert!(! has_network_setup(&["ip=off"]));
    assert!(! has_network_setup(&["ip=none"]));
}

#[test]
fn test_network_interfaces_section_is_dropped_when_empty() {
    // the template of the config passed to firecracker, read from
    // the source tree because the installed one may not exist
    let template = std::fs::File::open("template/firecracker.json")
        .expect("Failed to open firecracker template");
    let mut firecracker_config: crate::firecracker::FireCrackerConfig =
        serde_json::from_reader(template).expect("Failed to read template");
    assert!(
        serde_json::to_string(&firecracker_config).unwrap()
            .contains("network-interfaces")
    );
    firecracker_config.network_interfaces.clear();
    assert!(
        ! serde_json::to_string(&firecracker_config).unwrap()
            .contains("network-interfaces")
    );
}

#[test]
fn test_network_setup_of_instance() {
    let engine_section = instance_engine_section();
    // the instance takes the place of the global ip=dhcp
    assert!(has_network_setup(&engine_section.get_boot_args("@one")));
    assert!(has_network_setup(&engine_section.get_boot_args("@two")));
}

#[test]
fn test_program_config_file() {
    let config_file = config_file("app", false);
    assert_eq!("/usr/share/flakes/app.yaml", config_file);
}

#[test]
fn test_configured_pilot_options() {
    let cfg = config_from_str(
            r#"vm:
 name: JoJo
 host_app_path: /myapp
 runtime:
  runas: root
  pilot_options:
    - "%port:2000"
  firecracker:
   rootfs_image_path: /rootfs
   kernel_image_path: /kernel
   boot_args:
     - "init=/usr/sbin/sci"
include:
 tar: ~
"#,
    );
    assert_eq!(vec!["%port:2000"], cfg.pilot_options());
    let pilot_options = flakes::lookup::Lookup::get_pilot_run_options(
        cfg.pilot_options()
    );
    assert_eq!(Some(&"2000".to_string()), pilot_options.get("%port"));
}

#[test]
fn test_no_pilot_options_configured() {
    let cfg = config_from_str(
            r#"vm:
 name: JoJo
 host_app_path: /myapp
include:
 tar: ~
"#,
    );
    assert!(cfg.pilot_options().is_empty());
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
