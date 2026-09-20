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

pub fn get_tap_name(meta_name: &str) -> String {
    /*!
    Construct the name of the TAP device of a flake instance

    The meta_name is the name of the flake application optionally
    extended by the @NAME instance selector. The name has to be
    created the same way by the tool that creates the device and
    by the pilot that connects the VM to it
    !*/
    get_valid_interface_name(defaults::TAP_DEVICE_PREFIX, meta_name)
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
    use super::{get_network_config_file, NetworkConfig};

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
            "outgoing_interface: eth0\nnetwork: 172.16.0.0/24\n"
        ).unwrap();
        assert_eq!("eth0", network_config.outgoing_interface);
        assert_eq!(Some("172.16.0.0/24".to_string()), network_config.network);
        // a record which predates the selection of the network
        // provides no network
        let network_config: NetworkConfig = serde_yaml::from_str(
            "outgoing_interface: eth0\n"
        ).unwrap();
        assert_eq!("eth0", network_config.outgoing_interface);
        assert_eq!(None, network_config.network);
    }
}
