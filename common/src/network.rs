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
use std::fmt;
use std::fs;
use std::fs::File;
use std::io::{self, ErrorKind};
use std::iter;
use std::net::Ipv4Addr;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::process::Command;

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
    /// The host runs a DHCP server the VMs use through their
    /// gateway. Only then the VMs may reach the DHCP port of the
    /// host. Records which were written before this became
    /// selectable provide none, their VMs use a static address
    #[serde(default)]
    pub host_dhcp: bool,
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

// HostServices are the optional services of the host the VMs
// use through their gateway. A service which is not used stays
// closed to the VMs, see get_guest_service_rules()
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct HostServices {
    /// The host runs the DNS resolver of the VMs
    pub resolver: bool,
    /// The host runs a DHCP server for the VMs
    pub dhcp: bool
}

pub fn get_free_address(
    network: &Ipv4Network, used: &[Ipv4Addr]
) -> Option<Ipv4Addr> {
    /*!
    Provide the lowest address of the private network which is
    not in use. The network and broadcast address as well as the
    address of the gateway are never handed out
    !*/
    let gateway = network.gateway();
    for address in network.hosts() {
        if address != gateway && ! used.contains(&address) {
            return Some(address)
        }
    }
    error!("No free address left in {network}");
    None
}

pub fn get_nameserver(network: &Ipv4Network, services: HostServices) -> String {
    /*!
    Provide the name server of the VMs, the gateway if the host
    runs the resolver, the public name server otherwise
    !*/
    if services.resolver {
        network.gateway().to_string()
    } else {
        defaults::NETWORK_DNS.to_string()
    }
}

pub fn get_default_route_interface() -> Option<String> {
    /*!
    Provide the interface of the IPv4 default route
    !*/
    let mut call = Command::new(defaults::IP_TOOL);
    call.arg("-4").arg("route").arg("show").arg("default");
    let output = call.output().ok()?;
    if ! output.status.success() {
        return None
    }
    get_route_list_default_device(&String::from_utf8_lossy(&output.stdout))
}

pub fn get_route_list_default_device(routes: &str) -> Option<String> {
    /*!
    Provide the device of the first default route of the given
    output of "ip route show default"

    The routes are listed in the order of their metric, the first
    one is therefore the route which is in use. A default route
    without a device, e.g an unreachable route, is skipped
    !*/
    routes.lines().find_map(|route| {
        let mut fields = route.split_whitespace();
        fields.find(|field| *field == "dev")?;
        fields.next().map(ToOwned::to_owned)
    })
}

pub fn get_flake_allocations<'a>(
    state: &'a NetworkState, config_file: &str, instance: Option<&str>
) -> Vec<&'a Allocation> {
    /*!
    Provide the address allocations of the given state for the
    given instance of the flake of the given configuration file.
    An instance is connected for one user at a time, a consistent
    state therefore provides at most one
    !*/
    state.allocations.iter()
        .filter(|allocation| allocation.config_file == config_file
            && allocation.instance.as_deref() == instance
        )
        .collect()
}

pub fn is_vm_address(network: &Ipv4Network, address: &Ipv4Addr) -> bool {
    /*!
    Check if the given address can be the address of a VM in the
    given network. The network address, the gateway, which is the
    host side, and the broadcast address are not
    !*/
    network.contains(address)
        && *address != network.address
        && *address != network.gateway()
        && u32::from(*address) != network.broadcast()
}

pub fn get_free_network() -> Option<Ipv4Network> {
    /*!
    Provide a private network which does not collide with the
    networks the host is connected to

    The preferred network is taken if the host does not use it,
    in all other cases the fallback ranges are searched for a
    free network
    !*/
    select_free_network(&get_host_networks()?)
}

pub fn select_free_network(host_networks: &[Ipv4Network]) -> Option<Ipv4Network> {
    /*!
    Provide the first of the private networks to select from
    which does not overlap with one of the given networks
    !*/
    for candidate in get_network_candidates() {
        if ! host_networks.iter().any(
            |host_network| host_network.overlaps(&candidate)
        ) {
            return Some(candidate)
        }
    }
    error!("No free private network left on this host");
    None
}

pub fn network_collides(network: &Ipv4Network) -> bool {
    /*!
    Check if the given network overlaps with one of the networks
    the host is connected to. A host setup which cannot be read
    is not reported as a collision
    !*/
    match get_host_networks() {
        Some(host_networks) => host_networks.iter().any(
            |host_network| host_network.overlaps(network)
        ),
        None => false
    }
}

pub fn get_network_candidates() -> impl Iterator<Item = Ipv4Network> {
    /*!
    Provide the private networks to select from, in the order of
    their preference. The preferred network comes first, followed
    by the networks of the fallback ranges
    !*/
    let preferred = get_preferred_network();
    let step = 1 << (32 - defaults::NETWORK_PREFIX_LEN as u32);
    let fallback = defaults::NETWORK_RANGES.iter().flat_map(
        move |(first_network, count)| {
            let first = u32::from(get_default_network(first_network).address);
            (0..*count).map(
                move |index| Ipv4Network::new(
                    Ipv4Addr::from(first + index * step),
                    defaults::NETWORK_PREFIX_LEN
                )
            )
        }
    );
    iter::once(preferred).chain(
        fallback.filter(move |network| *network != preferred)
    )
}

pub fn get_preferred_network() -> Ipv4Network {
    /*!
    Provide the network which is used if the host allows it
    !*/
    get_default_network(defaults::NETWORK_PREFERRED)
}

pub fn get_default_network(address: &str) -> Ipv4Network {
    /*!
    Provide the network of one of the compiled in addresses
    !*/
    match address.parse() {
        Ok(address) => Ipv4Network::new(
            address, defaults::NETWORK_PREFIX_LEN
        ),
        Err(_) => panic!("Invalid network default: {}", address)
    }
}

pub fn get_host_networks() -> Option<Vec<Ipv4Network>> {
    /*!
    Provide the IPv4 networks the host is connected to

    The networks are read from the addresses of the interfaces
    of the host and from its routing table. The TAP devices of
    the flakes are left out, they are part of the setup this
    information is collected for
    !*/
    let mut networks = get_interface_networks()?;
    networks.extend(get_route_networks()?);
    Some(networks)
}

pub fn get_interface_networks() -> Option<Vec<Ipv4Network>> {
    /*!
    Provide the networks of the addresses of the host interfaces
    !*/
    let mut call = Command::new(defaults::IP_TOOL);
    call.arg("-4").arg("-oneline").arg("addr").arg("show");
    Some(get_address_list_networks(&run_output(&mut call)?))
}

pub fn get_address_list_networks(addresses: &str) -> Vec<Ipv4Network> {
    /*!
    Read the networks from the output of 'ip -4 -oneline addr show'

    The expected format of a line is:
    'INDEX: DEVICE inet ADDRESS/PREFIX_LEN ...'
    !*/
    let mut networks = Vec::new();
    for line in addresses.lines() {
        let device = line.split_whitespace().nth(1).unwrap_or_default();
        if is_flake_tap_device(device) {
            continue
        }
        networks.extend(
            get_field_value(line, "inet").and_then(Ipv4Network::parse)
        );
    }
    networks
}

pub fn get_route_networks() -> Option<Vec<Ipv4Network>> {
    /*!
    Provide the networks the host has a route for
    !*/
    let mut call = Command::new(defaults::IP_TOOL);
    call.arg("-4").arg("-oneline").arg("route").arg("show");
    Some(get_route_list_networks(&run_output(&mut call)?))
}

pub fn get_route_list_networks(routes: &str) -> Vec<Ipv4Network> {
    /*!
    Read the networks from the output of 'ip -4 -oneline route show'

    The expected format of a line is:
    'DESTINATION [via GATEWAY] dev DEVICE ...'. The destination of
    the default route is reported as 'default', it describes no
    network and is therefore skipped
    !*/
    let mut networks = Vec::new();
    for line in routes.lines() {
        let device = get_field_value(line, "dev").unwrap_or_default();
        if is_flake_tap_device(device) {
            continue
        }
        networks.extend(
            line.split_whitespace().next().and_then(Ipv4Network::parse)
        );
    }
    networks
}

pub fn is_flake_tap_device(device: &str) -> bool {
    /*!
    Check if the given device is a TAP device of a flake

    Devices attached to a parent device are reported in the
    'NAME@PARENT' notation, in the address list the name of the
    device is followed by a colon
    !*/
    device
        .split('@').next().unwrap_or_default()
        .trim_end_matches(':')
        .starts_with(defaults::TAP_DEVICE_PREFIX)
}

pub fn get_field_value<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    /*!
    Provide the value following the given field name in a line
    of the iproute2 output
    !*/
    let mut fields = line.split_whitespace();
    while let Some(field) = fields.next() {
        if field == name {
            return fields.next()
        }
    }
    None
}

pub fn run_output(call: &mut Command) -> Option<String> {
    /*!
    Run the given call and provide its standard output
    !*/
    match call.output() {
        Ok(output) => {
            if ! output.status.success() {
                error!(
                    "Failed to execute {:?}: {}", call, output.status
                );
                return None
            }
            Some(String::from_utf8_lossy(&output.stdout).to_string())
        },
        Err(error) => {
            error!("Failed to execute {call:?}: {error:?}");
            None
        }
    }
}

// Ipv4Network is an IPv4 network in its ADDRESS/PREFIX_LEN notation
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ipv4Network {
    /// Address of the network itself, e.g 172.16.0.0
    pub address: Ipv4Addr,
    /// Number of leading bits which form the network part
    pub prefix_len: u8
}

impl Ipv4Network {
    pub fn new(address: Ipv4Addr, prefix_len: u8) -> Ipv4Network {
        /*!
        Provide the network of the given prefix length the given
        address belongs to
        !*/
        let prefix_len = prefix_len.min(32);
        Ipv4Network {
            address: Ipv4Addr::from(
                u32::from(address) & Ipv4Network::netmask_bits(prefix_len)
            ),
            prefix_len
        }
    }

    pub fn parse(network: &str) -> Option<Ipv4Network> {
        /*!
        Read a network from its ADDRESS/PREFIX_LEN notation. An
        address given without a prefix length is a single host
        !*/
        let (address, prefix_len) = match network.split_once('/') {
            Some((address, prefix_len)) => (address, prefix_len.parse().ok()?),
            None => (network, 32)
        };
        if prefix_len > 32 {
            return None
        }
        Some(Ipv4Network::new(address.parse().ok()?, prefix_len))
    }

    pub fn netmask_bits(prefix_len: u8) -> u32 {
        /*!
        Provide the netmask of the given prefix length
        !*/
        match prefix_len {
            0 => 0,
            prefix_len => u32::MAX << (32 - prefix_len as u32)
        }
    }

    pub fn netmask(&self) -> Ipv4Addr {
        /*!
        Provide the netmask of the network, e.g 255.255.255.0
        !*/
        Ipv4Addr::from(Ipv4Network::netmask_bits(self.prefix_len))
    }

    pub fn gateway(&self) -> Ipv4Addr {
        /*!
        Provide the address of the host side of the network. It
        is the first address of the network and is configured on
        the TAP device of every instance
        !*/
        Ipv4Addr::from(u32::from(self.address) + 1)
    }

    pub fn gateway_route(&self) -> String {
        /*!
        Provide the route to the gateway as the guest expects it
        in its rd.route= option, e.g 172.16.0.1/24
        !*/
        format!("{}/{}", self.gateway(), self.prefix_len)
    }

    pub fn broadcast(&self) -> u32 {
        /*!
        Provide the last address of the network
        !*/
        u32::from(self.address) | ! Ipv4Network::netmask_bits(self.prefix_len)
    }

    pub fn hosts(&self) -> impl Iterator<Item = Ipv4Addr> {
        /*!
        Provide the addresses of the network which can be handed
        out. The network and the broadcast address are not part
        of them
        !*/
        (u32::from(self.address) + 1..self.broadcast()).map(Ipv4Addr::from)
    }

    pub fn contains(&self, address: &Ipv4Addr) -> bool {
        /*!
        Check if the given address belongs to the network
        !*/
        u32::from(*address) & Ipv4Network::netmask_bits(self.prefix_len)
            == u32::from(self.address)
    }

    pub fn overlaps(&self, other: &Ipv4Network) -> bool {
        /*!
        Check if the given network shares addresses with this one
        !*/
        u32::from(self.address) <= other.broadcast()
            && u32::from(other.address) <= self.broadcast()
    }
}

impl fmt::Display for Ipv4Network {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "{}/{}", self.address, self.prefix_len)
    }
}

// NetworkState is the host wide record of the network setup,
// see defaults::NETWORK_STATE
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct NetworkState {
    /// IP forwarding was switched off and has been switched on by
    /// the network setup of the host, its cleanup switches it off
    /// again
    #[serde(default)]
    pub ip_forward_enabled: bool,
    /// The private network of the VMs in its ADDRESS/PREFIX_LEN
    /// notation, recorded by the network setup of the host. A state
    /// written before the setup was recorded here provides none
    #[serde(default)]
    pub network: Option<String>,
    /// Interface the traffic of the VMs leaves the host through,
    /// recorded along with the network
    #[serde(default)]
    pub outgoing_interface: Option<String>,
    /// The host runs the DNS resolver of the VMs
    #[serde(default)]
    pub host_resolver: bool,
    /// The host runs a DHCP server for the VMs
    #[serde(default)]
    pub host_dhcp: bool,
    /// Networks in their ADDRESS/PREFIX_LEN notation the traffic
    /// of the VMs to the outside world must not go to, see
    /// get_egress_networks(). A state written before the egress
    /// policy became selectable provides none
    #[serde(default)]
    pub egress_deny: Vec<String>,
    /// The addresses handed out to the VM instances of all users
    #[serde(default)]
    pub allocations: Vec<Allocation>
}

impl NetworkState {
    pub fn get_network(&self) -> Option<Ipv4Network> {
        /*!
        Provide the recorded private network of the VMs
        !*/
        Ipv4Network::parse(self.network.as_deref()?)
    }

    pub fn get_services(&self) -> HostServices {
        /*!
        Provide the recorded services of the host the VMs use
        !*/
        HostServices { resolver: self.host_resolver, dhcp: self.host_dhcp }
    }

    pub fn get_egress_deny(&self) -> Option<Vec<Ipv4Network>> {
        /*!
        Provide the recorded networks of the egress policy, None
        if one of them is invalid
        !*/
        self.egress_deny.iter()
            .map(|network| Ipv4Network::parse(network))
            .collect()
    }
}

pub fn get_egress_networks(
    egress_deny: &[String]
) -> Result<Vec<Ipv4Network>, String> {
    /*!
    Provide the networks of the egress policy of the VM network

    Each entry is the name of one of the sets of networks in
    defaults::NETWORK_EGRESS_PRESETS or a network in its
    ADDRESS/PREFIX_LEN notation. An address without a prefix length
    is a single host. Networks which are given more than once are
    only listed once, in the order they are given first
    !*/
    let mut networks: Vec<Ipv4Network> = Vec::new();
    for entry in egress_deny {
        let preset = defaults::NETWORK_EGRESS_PRESETS.iter()
            .find(|(name, _)| name == entry);
        let entry_networks = match preset {
            Some((_, preset_networks)) => preset_networks.iter()
                .filter_map(|network| Ipv4Network::parse(network))
                .collect(),
            None => vec![Ipv4Network::parse(entry).ok_or(format!(
                "Invalid egress policy entry {entry:?}: expected one of {} \
                or ADDRESS[/PREFIX_LEN]",
                defaults::NETWORK_EGRESS_PRESETS.iter()
                    .map(|(name, _)| *name).collect::<Vec<_>>().join(", ")
            ))?]
        };
        for network in entry_networks {
            if ! networks.contains(&network) {
                networks.push(network)
            }
        }
    }
    Ok(networks)
}

// Allocation is an address of the private network handed out to
// the instance of a flake application run by one user
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Allocation {
    /// Address of the VM instance
    pub address: Ipv4Addr,
    /// ID of the user who runs the instance and owns its TAP device
    pub uid: u32,
    /// Name of the TAP device the instance is connected to
    pub tap: String,
    /// Path of the flake configuration of the application
    pub config_file: String,
    /// The '@NAME' instance selector, None for the application itself
    #[serde(default)]
    pub instance: Option<String>
}

// NetworkStateLock holds the lock of the network state, see
// lock_network_state(). The lock is released when it is dropped
pub struct NetworkStateLock {
    _lock_file: Option<File>
}

pub fn lock_network_state(exclusive: bool) -> Option<NetworkStateLock> {
    /*!
    Lock the host wide network state

    A call which changes the network setup reads the state,
    changes it and writes it again. Calls of different users can
    run at the same time and would otherwise lose the changes of
    each other, e.g hand out the same address twice. Such a call
    therefore holds an exclusive lock while it runs. A call which
    only reads the state holds a shared lock, the state is written
    in place and must not be read while it is written.

    The lock is a flock(2) on a lock file of its own. The state
    file is not suitable, it does not exist before the first
    network setup. The lock file belongs to root like the state
    file. Only the helper changes the state, it runs as root and
    creates the lock file with its first change. A call which only
    reads the state does not create it and reads the state without
    lock if there is no lock file yet
    !*/
    let lock_file = defaults::NETWORK_STATE_LOCK;
    let mut locked = lock_file_of(lock_file, exclusive, 0);
    if exclusive {
        if let Err(error) = &locked {
            if error.kind() == ErrorKind::NotFound {
                if ! create_network_state_lock() {
                    return None
                }
                locked = lock_file_of(lock_file, exclusive, 0);
            }
        }
    }
    match locked {
        Ok(file) => Some(NetworkStateLock { _lock_file: Some(file) }),
        Err(error) if error.kind() == ErrorKind::NotFound && ! exclusive => {
            Some(NetworkStateLock { _lock_file: None })
        },
        Err(error) => {
            error!("Failed to lock {lock_file}: {error}");
            None
        }
    }
}

fn create_network_state_lock() -> bool {
    /*!
    Create the lock file of the network state

    The file belongs to the caller, which has to be root, see
    lock_network_state(). A lock file which exists already is kept
    !*/
    let lock_file = Path::new(defaults::NETWORK_STATE_LOCK);
    if let Some(state_dir) = lock_file.parent() {
        if let Err(error) = fs::create_dir_all(state_dir) {
            error!("Failed to create {}: {error}", state_dir.display());
            return false
        }
    }
    match fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o644)
        .custom_flags(libc::O_NOFOLLOW)
        .open(lock_file)
    {
        Ok(_) => true,
        Err(error) => {
            error!("Failed to create {}: {error}", lock_file.display());
            false
        }
    }
}

pub fn lock_file_of(lock_file: &str, exclusive: bool, owner: u32) -> io::Result<File> {
    /*!
    Lock the given lock file, which has to belong to owner

    The call waits until the lock is available. A lock file which
    does not exist is reported as NotFound. A lock file which got
    replaced while waiting for its lock does not protect anything,
    the replacement is locked instead
    !*/
    let operation = if exclusive { libc::LOCK_EX } else { libc::LOCK_SH };
    for _ in 0..defaults::LOCK_ATTEMPTS {
        // the lock file is only opened for reading, flock does
        // not need more and the file belongs to root
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(lock_file)?;
        match flock(&file, operation | libc::LOCK_NB) {
            Ok(_) => { },
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                info!("Waiting for {lock_file}, held by another call...");
                flock(&file, operation)?
            },
            Err(error) => return Err(error)
        }
        let locked = file.metadata()?;
        if ! locked.is_file() || locked.uid() != owner {
            return Err(io::Error::new(
                ErrorKind::PermissionDenied,
                format!(
                    "{lock_file} is not a regular file owned by user ID \
                    {owner}, refusing to use it"
                )
            ))
        }
        match fs::symlink_metadata(lock_file) {
            Ok(current) if current.dev() == locked.dev()
                && current.ino() == locked.ino() => return Ok(file),
            Ok(_) => { },
            Err(error) if error.kind() == ErrorKind::NotFound => { },
            Err(error) => return Err(error)
        }
        debug!("{lock_file} was replaced while waiting for it, locking again");
    }
    Err(io::Error::other(format!(
        "{lock_file} kept being replaced, gave up after {} attempts",
        defaults::LOCK_ATTEMPTS
    )))
}

pub fn flock(file: &File, operation: libc::c_int) -> io::Result<()> {
    /*!
    Apply the given flock(2) operation to the given file
    !*/
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
            return Ok(())
        }
        let error = io::Error::last_os_error();
        if error.kind() != ErrorKind::Interrupted {
            return Err(error)
        }
    }
}

pub fn read_network_state() -> Option<NetworkState> {
    /*!
    Read the host wide network state

    There is no state before the first network setup, which is
    read as an empty one. The state is only trusted if it belongs
    to root, everybody else could hand out addresses with it
    !*/
    read_network_state_from(defaults::NETWORK_STATE)
}

pub fn read_network_state_from(state_file: &str) -> Option<NetworkState> {
    /*!
    Read the host wide network state from the given file, see
    read_network_state()
    !*/
    match fs::symlink_metadata(state_file) {
        Ok(metadata) => {
            if ! metadata.is_file() || metadata.uid() != 0 {
                error!(
                    "{state_file} is not a regular file owned by root, \
                    refusing to use it"
                );
                return None
            }
        },
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Some(NetworkState::default())
        },
        Err(error) => {
            error!("Failed to read {state_file}: {error}");
            return None
        }
    }
    let yaml_data = match fs::read_to_string(state_file) {
        Ok(yaml_data) => yaml_data,
        Err(error) => {
            error!("Failed to read {state_file}: {error}");
            return None
        }
    };
    match serde_yaml::from_str(&yaml_data) {
        Ok(state) => Some(state),
        Err(error) => {
            error!("Failed to parse {state_file}: {error}");
            None
        }
    }
}

pub fn tap_exists(tap: &str) -> bool {
    /*!
    Check if the given TAP device is already present
    !*/
    let mut call = Command::new(defaults::IP_TOOL);
    call.arg("tuntap").arg("list");
    match call.output() {
        Ok(output) => String::from_utf8_lossy(&output.stdout).lines().any(
            |line| get_tuntap_device_name(line) == tap
        ),
        Err(error) => {
            error!("Failed to execute {}: {error:?}", defaults::IP_TOOL);
            false
        }
    }
}

pub fn get_tuntap_device_name(tuntap_list_line: &str) -> &str {
    /*!
    Read the device name from a line of the 'ip tuntap list' output

    The expected format of a line is: 'NAME: tun|tap FLAGS...'.
    For devices attached to a parent device the name is reported
    in the 'NAME@PARENT' notation
    !*/
    tuntap_list_line
        .split(':').next().unwrap_or_default()
        .split('@').next().unwrap_or_default()
        .trim()
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use uzers::get_current_uid;

    use super::{
        get_jump_rules, get_masquerade_rule, get_network_config_file,
        get_tap_name, NetworkConfig, get_address_list_networks,
        get_egress_networks, get_flake_allocations, get_free_address,
        get_nameserver, get_network_candidates, get_preferred_network,
        get_route_list_default_device, get_route_list_networks,
        is_vm_address, lock_file_of, select_free_network, Allocation,
        HostServices, Ipv4Network, NetworkState
    };
    use crate::defaults::IFNAMSIZ;

    fn network(network: &str) -> Ipv4Network {
        Ipv4Network::parse(network).unwrap()
    }

    fn address(address: &str) -> Ipv4Addr {
        address.parse().unwrap()
    }

    fn allocation(
        address: &str, uid: u32, tap: &str, config_file: &str,
        instance: Option<&str>
    ) -> Allocation {
        Allocation {
            address: address.parse().unwrap(),
            uid,
            tap: tap.to_string(),
            config_file: config_file.to_string(),
            instance: instance.map(ToOwned::to_owned)
        }
    }

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
            host_resolver: true\nhost_dhcp: true\n"
        ).unwrap();
        assert_eq!("eth0", network_config.outgoing_interface);
        assert_eq!(Some("172.16.0.0/24".to_string()), network_config.network);
        assert!(network_config.host_resolver);
        assert!(network_config.host_dhcp);
        // a record which predates the selection of the network
        // provides no network
        let network_config: NetworkConfig = serde_yaml::from_str(
            "outgoing_interface: eth0\n"
        ).unwrap();
        assert_eq!("eth0", network_config.outgoing_interface);
        assert_eq!(None, network_config.network);
        // a record which predates the host service settings
        // uses none of them
        assert!(! network_config.host_resolver);
        assert!(! network_config.host_dhcp);
    }

    #[test]
    fn test_get_flake_allocations() {
        let user_flake = "/home/tux/.config/flakes/myapp.yaml";
        let state = NetworkState {
            allocations: vec![
                allocation(
                    "172.16.0.2", 1000, "tap-1000_myapp", user_flake, None
                ),
                allocation(
                    "172.16.0.3", 1000, "tap-1000_myapp_one",
                    user_flake, Some("@one")
                ),
                allocation(
                    "172.16.0.4", 0, "tap-0_myapp",
                    "/usr/share/flakes/myapp.yaml", None
                )
            ],
            ..Default::default()
        };
        let addresses = |instance| get_flake_allocations(
            &state, user_flake, instance
        ).iter().map(|allocation| allocation.address).collect::<Vec<_>>();

        // the instance selects the allocation, another flake of the
        // same name is not taken into account
        assert_eq!(vec![address("172.16.0.2")], addresses(None));
        assert_eq!(vec![address("172.16.0.3")], addresses(Some("@one")));
        // an instance which is not connected has none
        assert!(addresses(Some("@two")).is_empty());
        assert!(get_flake_allocations(
            &state, "/home/tux/.config/flakes/other.yaml", None
        ).is_empty());
    }

    #[test]
    fn test_lock_file_of() {
        use std::fs;
        use std::io::ErrorKind;
        use std::os::unix::fs::{symlink, MetadataExt};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::{thread, time};

        let uid = get_current_uid();
        let tmp_dir = tempfile::tempdir().unwrap();
        let lock = tmp_dir.path().join("lock");
        let lock_file = lock.to_str().unwrap().to_string();

        // a missing lock file is reported as such
        assert_eq!(
            lock_file_of(&lock_file, true, uid).unwrap_err().kind(),
            ErrorKind::NotFound
        );
        fs::write(&lock, "").unwrap();

        // shared locks are held at the same time
        let shared = lock_file_of(&lock_file, false, uid).unwrap();
        let other_shared = lock_file_of(&lock_file, false, uid).unwrap();
        drop((shared, other_shared));

        // an exclusive lock waits until the holder releases it
        let held = lock_file_of(&lock_file, true, uid).unwrap();
        let released = Arc::new(AtomicBool::new(false));
        let waiter = {
            let lock_file = lock_file.clone();
            let released = Arc::clone(&released);
            thread::spawn(move || {
                let _lock = lock_file_of(&lock_file, true, uid).unwrap();
                assert!(released.load(Ordering::SeqCst));
            })
        };
        thread::sleep(time::Duration::from_millis(200));
        released.store(true, Ordering::SeqCst);
        drop(held);
        waiter.join().unwrap();

        // a lock file replaced while waiting for it is not used,
        // the replacement is locked instead
        let held = lock_file_of(&lock_file, true, uid).unwrap();
        let waiter = {
            let lock_file = lock_file.clone();
            thread::spawn(move || lock_file_of(&lock_file, true, uid).unwrap())
        };
        thread::sleep(time::Duration::from_millis(200));
        let replacement = tmp_dir.path().join("replacement");
        fs::write(&replacement, "").unwrap();
        fs::rename(&replacement, &lock).unwrap();
        drop(held);
        let locked = waiter.join().unwrap();
        assert_eq!(
            locked.metadata().unwrap().ino(),
            fs::metadata(&lock).unwrap().ino()
        );
        drop(locked);

        // a lock file of another owner is refused
        assert_eq!(
            lock_file_of(&lock_file, true, uid + 1).unwrap_err().kind(),
            ErrorKind::PermissionDenied
        );

        // a symlink is refused
        let link = tmp_dir.path().join("link");
        symlink(&lock, &link).unwrap();
        assert!(lock_file_of(link.to_str().unwrap(), false, uid).is_err());
    }

    #[test]
    fn test_network_state() {
        // a state file without the optional entries is read as empty
        let state: NetworkState = serde_yaml::from_str("{}").unwrap();
        assert_eq!(NetworkState::default(), state);
        let state = NetworkState {
            ip_forward_enabled: true,
            network: Some("172.16.0.0/24".to_string()),
            outgoing_interface: Some("eth0".to_string()),
            host_resolver: true,
            host_dhcp: false,
            egress_deny: vec!["169.254.0.0/16".to_string()],
            allocations: vec![
                allocation(
                    "172.16.0.2", 1000, "tap-1000_myapp",
                    "/home/tux/.config/flakes/myapp.yaml", Some("@one")
                )
            ]
        };
        let yaml_data = serde_yaml::to_string(&state).unwrap();
        assert!(yaml_data.contains("address: 172.16.0.2"));
        assert_eq!(Some(network("172.16.0.0/24")), state.get_network());
        assert_eq!(
            HostServices { resolver: true, dhcp: false }, state.get_services()
        );
        assert_eq!(
            Some(vec![network("169.254.0.0/16")]), state.get_egress_deny()
        );
        assert_eq!(
            state, serde_yaml::from_str::<NetworkState>(&yaml_data).unwrap()
        );
        // an invalid network of the egress policy is not ignored
        let state = NetworkState {
            egress_deny: vec!["10.0.0.0/8".to_string(), "bogus".to_string()],
            ..Default::default()
        };
        assert_eq!(None, state.get_egress_deny());
    }

    #[test]
    fn test_get_egress_networks() {
        let egress_networks = |entries: &[&str]| get_egress_networks(
            &entries.iter().map(|entry| entry.to_string()).collect::<Vec<_>>()
        );
        assert_eq!(Ok(vec![]), egress_networks(&[]));
        assert_eq!(
            Ok(vec![network("169.254.0.0/16")]),
            egress_networks(&["link-local"])
        );
        // presets and networks are combined, duplicates listed once
        assert_eq!(
            Ok(vec![
                network("10.0.0.0/8"), network("172.16.0.0/12"),
                network("192.168.0.0/16"), network("100.100.100.200/32")
            ]),
            egress_networks(&["private", "10.0.0.0/8", "100.100.100.200"])
        );
        // a network is taken by its network address
        assert_eq!(
            Ok(vec![network("10.1.0.0/16")]),
            egress_networks(&["10.1.2.3/16"])
        );
        assert!(egress_networks(&["metadata"]).is_err());
        assert!(egress_networks(&["10.0.0.0/33"]).is_err());
        assert!(egress_networks(&["-j ACCEPT"]).is_err());
    }

    #[test]
    fn test_get_nameserver() {
        let resolver = HostServices { resolver: true, dhcp: false };
        let dhcp = HostServices { resolver: false, dhcp: true };
        assert_eq!(
            "8.8.8.8",
            get_nameserver(&network("172.16.0.0/24"), HostServices::default())
        );
        // a DHCP server on the host does not change the name server
        assert_eq!("8.8.8.8", get_nameserver(&network("172.16.0.0/24"), dhcp));
        assert_eq!(
            "172.16.0.1", get_nameserver(&network("172.16.0.0/24"), resolver)
        );
        assert_eq!(
            "192.168.3.1", get_nameserver(&network("192.168.3.0/24"), resolver)
        );
    }

    #[test]
    fn test_parse_network() {
        let private = network("172.16.0.0/24");
        assert_eq!("172.16.0.0/24", private.to_string());
        assert_eq!(address("255.255.255.0"), private.netmask());
        assert_eq!(address("172.16.0.1"), private.gateway());
        assert_eq!("172.16.0.1/24", private.gateway_route());
        // the address is reduced to the network it belongs to
        assert_eq!(private, network("172.16.0.42/24"));
        // an address without a prefix length is a single host
        assert_eq!("10.0.0.1/32", network("10.0.0.1").to_string());
        assert_eq!(None, Ipv4Network::parse("172.16.0.0/33"));
        assert_eq!(None, Ipv4Network::parse("172.16.0.0/foo"));
        assert_eq!(None, Ipv4Network::parse("default"));
    }

    #[test]
    fn test_network_contains_and_overlaps() {
        let private = network("172.16.0.0/24");
        assert!(private.contains(&address("172.16.0.1")));
        assert!(! private.contains(&address("172.16.1.1")));
        // a network which is part of another one overlaps with it
        assert!(private.overlaps(&network("172.16.0.0/16")));
        assert!(network("172.16.0.0/16").overlaps(&private));
        assert!(private.overlaps(&network("172.16.0.5")));
        assert!(! private.overlaps(&network("172.16.1.0/24")));
        assert!(! private.overlaps(&network("192.168.0.0/16")));
    }

    #[test]
    fn test_network_hosts() {
        let hosts: Vec<Ipv4Addr> = network("172.16.0.0/24").hosts().collect();
        // the network and the broadcast address are not handed out
        assert_eq!(254, hosts.len());
        assert_eq!(address("172.16.0.1"), hosts[0]);
        assert_eq!(address("172.16.0.254"), hosts[253]);
    }

    #[test]
    fn test_get_free_address() {
        let private = network("10.1.2.0/24");
        // the gateway address is never handed out
        assert_eq!(
            Some(address("10.1.2.2")), get_free_address(&private, &[])
        );
        assert_eq!(
            Some(address("10.1.2.4")),
            get_free_address(&private, &[
                address("10.1.2.2"), address("10.1.2.3")
            ])
        );
        // addresses of another network do not take part
        assert_eq!(
            Some(address("10.1.2.2")),
            get_free_address(&private, &[address("172.16.0.2")])
        );
        let used: Vec<Ipv4Addr> = private.hosts().collect();
        assert_eq!(None, get_free_address(&private, &used));
    }

    #[test]
    fn test_get_network_candidates() {
        let candidates: Vec<Ipv4Network> = get_network_candidates()
            .take(3).collect();
        assert_eq!(
            vec![
                network("172.16.0.0/24"),
                network("172.16.1.0/24"),
                network("172.16.2.0/24")
            ],
            candidates
        );
        // the preferred network is only offered once
        assert_eq!(
            1, get_network_candidates()
                .filter(|network| *network == get_preferred_network()).count()
        );
    }

    #[test]
    fn test_select_free_network() {
        // the preferred network is taken if the host allows it
        assert_eq!(
            Some(network("172.16.0.0/24")),
            select_free_network(&[network("192.168.0.0/24")])
        );
        // a network of the host is not handed out
        assert_eq!(
            Some(network("172.16.1.0/24")),
            select_free_network(&[network("172.16.0.0/24")])
        );
        // this includes networks which only overlap with it. A
        // host which uses the entire first range gets a network
        // of the next one
        assert_eq!(
            Some(network("192.168.0.0/24")),
            select_free_network(&[network("172.16.0.0/12")])
        );
    }

    #[test]
    fn test_get_address_list_networks() {
        let addresses = "\
1: lo    inet 127.0.0.1/8 scope host lo\\       valid_lft forever
2: eth0    inet 172.16.0.5/24 brd 172.16.0.255 scope global eth0\\       valid_lft forever
3: tap-app    inet 172.16.0.1/24 scope global tap-app\\       valid_lft forever
";
        assert_eq!(
            vec![network("127.0.0.0/8"), network("172.16.0.0/24")],
            get_address_list_networks(addresses)
        );
    }

    #[test]
    fn test_get_route_list_networks() {
        let routes = "\
default via 192.168.1.1 dev eth0 proto dhcp metric 100
192.168.1.0/24 dev eth0 proto kernel scope link src 192.168.1.23
172.16.0.2 dev tap-app scope link
";
        assert_eq!(
            vec![network("192.168.1.0/24")], get_route_list_networks(routes)
        );
    }

    #[test]
    fn test_get_route_list_default_device() {
        let routes = "\
unreachable default metric 50
default via 192.168.1.1 dev eth0 proto dhcp metric 100
default via 10.0.0.1 dev wlan0 proto dhcp metric 600
";
        // the first default route with a device is in use
        assert_eq!(
            Some("eth0".to_string()), get_route_list_default_device(routes)
        );
        // without a default route there is no device
        assert_eq!(None, get_route_list_default_device(""));
        assert_eq!(
            None, get_route_list_default_device("unreachable default\n")
        );
    }

    #[test]
    fn test_is_vm_address() {
        let network = Ipv4Network::parse("172.16.0.0/24").unwrap();
        assert!(is_vm_address(&network, &"172.16.0.2".parse().unwrap()));
        assert!(is_vm_address(&network, &"172.16.0.254".parse().unwrap()));
        // network, gateway and broadcast address
        assert!(! is_vm_address(&network, &"172.16.0.0".parse().unwrap()));
        assert!(! is_vm_address(&network, &"172.16.0.1".parse().unwrap()));
        assert!(! is_vm_address(&network, &"172.16.0.255".parse().unwrap()));
        // outside of the private network
        assert!(! is_vm_address(&network, &"192.168.1.2".parse().unwrap()));
    }
}
