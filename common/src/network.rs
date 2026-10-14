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
use serde::{Serialize, Deserialize};
use std::fs;

use crate::config::get_user_home;
use crate::defaults;

// NetworkConfig is the record of the host network setup created
// by 'flake-ctl firecracker network init'
#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Interface the traffic of the VMs is routed to the
    /// outside world through
    pub outgoing_interface: String,
    /// The private network between the host and the VMs in its
    /// ADDRESS/PREFIX_LEN notation. Records which were written
    /// before the network became selectable provide none, they
    /// were created for the preferred network
    #[serde(default)]
    pub network: Option<String>,
    /// The host runs a resolver the VMs use through their gateway.
    /// Only then the VMs get the gateway as their name server and
    /// may reach the DNS port of the host. Records which were
    /// written before this became selectable provide none, their
    /// VMs use the public name server
    #[serde(default)]
    pub host_resolver: bool,
}

pub fn get_network_config_file(usermode: bool) -> String {
    /*!
    Provide the path of the host network setup record

    The system wide setup is recorded next to the other flake
    configuration files, the setup a user created is recorded
    in the home directory of that user
    !*/
    if ! usermode {
        return defaults::NETWORK_CONFIG.to_string()
    }
    format!("{}/{}", get_user_home(), defaults::NETWORK_CONFIG_USER)
}

pub fn read_network_config(usermode: bool) -> Option<NetworkConfig> {
    /*!
    Read the record of the host network setup

    There is no record if the host was never prepared with
    'flake-ctl firecracker network init' or if the setup was
    created by hand
    !*/
    let config_file = get_network_config_file(usermode);
    let yaml_data = fs::read_to_string(&config_file).ok()?;
    match serde_yaml::from_str(&yaml_data) {
        Ok(network_config) => Some(network_config),
        Err(error) => {
            error!("Failed to parse {config_file}: {error:?}");
            None
        }
    }
}

pub fn get_tap_name(uid: u32, meta_name: &str) -> String {
    /*!
    Construct the name of the TAP device of a flake instance

    The meta_name is the name of the flake application optionally
    extended by the @NAME instance selector. The device belongs to
    the user with the given ID, who is the only one allowed to
    connect a VM to it. The ID is part of the name, which keeps
    the devices of two users running the same flake apart. The
    name has to be created the same way by the tool that creates
    the device and by the pilot that connects the VM to it
    !*/
    get_valid_interface_name(
        defaults::TAP_DEVICE_PREFIX, &format!("{uid}_{meta_name}")
    )
}

// FirewallRule is a netfilter rule of the host firewall setup
#[derive(Debug, PartialEq)]
pub struct FirewallRule {
    /// Name of the iptables table, e.g filter or nat
    pub table: &'static str,
    /// Name of the chain the rule belongs to
    pub chain: String,
    /// Match and target of the rule
    pub spec: Vec<String>
}

impl FirewallRule {
    pub fn new(table: &'static str, chain: &str, spec: &[&str]) -> Self {
        FirewallRule {
            table,
            chain: chain.to_string(),
            spec: spec.iter().map(|arg| arg.to_string()).collect()
        }
    }
}

pub fn get_tap_match() -> String {
    /*!
    Provide the interface match of all TAP devices of VM instances
    in the notation of iptables
    !*/
    format!("{}+", defaults::TAP_DEVICE_PREFIX)
}

pub fn get_jump_rules() -> Vec<FirewallRule> {
    /*!
    Provide the rules which lead from the builtin chains into the
    chains of the host firewall setup

    All traffic which is forwarded or masqueraded passes the chains
    of the setup. Their rules only match traffic of the TAP devices,
    other traffic returns to the builtin chain unchanged. Traffic to
    the host itself only enters the setup if it comes from a TAP
    device
    !*/
    let tap_match = get_tap_match();
    vec![
        FirewallRule::new(
            "nat", "POSTROUTING", &["-j", defaults::FIREWALL_NAT_CHAIN]
        ),
        FirewallRule::new(
            "filter", "FORWARD", &["-j", defaults::FIREWALL_FORWARD_CHAIN]
        ),
        FirewallRule::new(
            "filter", "INPUT",
            &["-i", &tap_match, "-j", defaults::FIREWALL_INPUT_CHAIN]
        )
    ]
}

pub fn get_masquerade_rule(
    network: &str, outgoing_interface: &str
) -> FirewallRule {
    /*!
    Provide the rule which masquerades the traffic of the VMs

    Only traffic which originates from the private network of the
    VMs and leaves through the outgoing interface is rewritten
    !*/
    FirewallRule::new(
        "nat", defaults::FIREWALL_NAT_CHAIN,
        &["-s", network, "-o", outgoing_interface, "-j", "MASQUERADE"]
    )
}

pub fn get_valid_interface_name(prefix: &str, name: &str) -> String {
    /*!
    Construct a valid network interface name from prefix and name

    The name of a flake is a free form string, e.g the basename of
    the calling program optionally extended by the @NAME instance
    selector. The kernel on the other hand only accepts interface
    names which are shorter than IFNAMSIZ and which do not contain
    '/', ':' or whitespace. In addition tools like iproute2 use
    the '@' character to report the parent of an interface and
    therefore can't handle it as part of a name.

    Thus all characters outside of [A-Za-z0-9_] are replaced by '_'
    and names that are too long are shortened. To keep shortened
    names unique they are suffixed with a hash of the original name
    !*/
    let name_size = defaults::IFNAMSIZ - 1 - prefix.len();
    let mut interface_name = String::new();
    for letter in name.chars() {
        if letter.is_ascii_alphanumeric() || letter == '_' {
            interface_name.push(letter)
        } else {
            interface_name.push('_')
        }
    }
    if interface_name.len() > name_size || interface_name.is_empty() {
        let hash_size = defaults::IFNAME_HASH_LEN;
        let short_name: String = interface_name.chars().take(
            name_size.saturating_sub(hash_size + 1)
        ).collect();
        interface_name = format!(
            "{}_{:0width$x}",
            short_name,
            name_hash(name) & ((1 << (4 * hash_size)) - 1),
            width = hash_size
        );
    }
    format!("{prefix}{interface_name}")
}

fn name_hash(name: &str) -> u64 {
    /*!
    FNV-1a hash of the given name

    Used to keep shortened interface names unique. An own
    implementation is used because the result must stay the
    same across program calls and rust versions, which the
    hashers from the standard library do not guarantee
    !*/
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::{
        get_jump_rules, get_masquerade_rule, get_network_config_file,
        get_tap_name, NetworkConfig
    };
    use crate::defaults::IFNAMSIZ;

    #[test]
    fn test_get_tap_name() {
        // the ID of the user is part of the name, the devices of two
        // users running the same flake are kept apart
        assert_eq!("tap-1000_myapp", get_tap_name(1000, "myapp"));
        assert_ne!(get_tap_name(1000, "myapp"), get_tap_name(1001, "myapp"));
        // a name which is too long is shortened and made unique
        let long_name = get_tap_name(1000, "myapplication@instance");
        assert!(long_name.len() < IFNAMSIZ);
        assert!(long_name.starts_with("tap-1000_"));
        assert_ne!(long_name, get_tap_name(1000, "myapplication@other"));
    }

    #[test]
    fn test_firewall_rules() {
        let jump_rules: Vec<(String, String, String)> = get_jump_rules()
            .into_iter()
            .map(|rule| (rule.table.to_string(), rule.chain, rule.spec.join(" ")))
            .collect();
        assert_eq!(
            vec![
                (
                    "nat".to_string(), "POSTROUTING".to_string(),
                    "-j FLAKE_PILOT_NAT".to_string()
                ),
                (
                    "filter".to_string(), "FORWARD".to_string(),
                    "-j FLAKE_PILOT_FORWARD".to_string()
                ),
                (
                    "filter".to_string(), "INPUT".to_string(),
                    "-i tap-+ -j FLAKE_PILOT_INPUT".to_string()
                )
            ],
            jump_rules
        );
        let masquerade = get_masquerade_rule("172.16.0.0/24", "eth0");
        assert_eq!("nat", masquerade.table);
        assert_eq!("FLAKE_PILOT_NAT", masquerade.chain);
        assert_eq!(
            "-s 172.16.0.0/24 -o eth0 -j MASQUERADE", masquerade.spec.join(" ")
        );
    }

    #[test]
    fn test_get_network_config_file() {
        assert_eq!(
            "/etc/flakes/network.yaml", get_network_config_file(false)
        );
        assert!(get_network_config_file(true).ends_with(
            "/.config/flakes/firecracker/network.yaml"
        ));
    }

    #[test]
    fn test_read_network_record() {
        let network_config: NetworkConfig = serde_yaml::from_str(
            "outgoing_interface: eth0\nnetwork: 172.16.0.0/24\n\
            host_resolver: true\n"
        ).unwrap();
        assert_eq!("eth0", network_config.outgoing_interface);
        assert_eq!(Some("172.16.0.0/24".to_string()), network_config.network);
        assert!(network_config.host_resolver);
        // a record which predates the selection of the network
        // provides no network
        let network_config: NetworkConfig = serde_yaml::from_str(
            "outgoing_interface: eth0\n"
        ).unwrap();
        assert_eq!("eth0", network_config.outgoing_interface);
        assert_eq!(None, network_config.network);
        // a record which predates the host resolver setting
        // does not use it
        assert!(! network_config.host_resolver);
    }
}
