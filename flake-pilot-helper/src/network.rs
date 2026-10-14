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
//! The network of the firecracker VMs
//!
//! The setup of the host and the connections of the single VMs
//! are recorded in the network state, see flakes::network. Only
//! the helper writes it.
//!
use std::fs;
use std::io::Write;
use std::net::Ipv4Addr;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::NamedTempFile;

use flakes::defaults;
use flakes::network::{
    get_free_address, get_network_candidates, get_tap_name,
    is_flake_tap_device, is_vm_address, lock_network_state, network_collides,
    read_network_config, read_network_state, tap_exists, Allocation,
    Ipv4Network, NetworkState
};

use crate::caller::Caller;
use crate::firewall::{
    allow_guest_services, delete_firewall, delete_legacy_rules,
    firewall_is_active, revoke_guest_services, setup_firewall
};
use crate::run::run_ok;
use crate::validate;

// Directory the kernel lists the network interfaces of the host in
const SYS_CLASS_NET: &str = "/sys/class/net";

// VmInstance is the network identity of the VM of a flake instance
pub struct VmInstance<'a> {
    /// Path of the flake configuration file of the application
    pub config_file: &'a str,
    /// The '@NAME' instance selector, None for the application itself
    pub instance: Option<String>,
    /// Name of the TAP device the instance is connected to
    pub tap: String,
    /// ID of the user who runs the VM and owns its TAP device
    pub uid: u32
}

pub fn init(
    caller: &Caller, network: &str, outgoing_interface: &str,
    host_resolver: bool, host_dhcp: bool
) -> bool {
    /*!
    Prepare the host for NAT based VM networking

    Firecracker connects a VM to the host through a TUN/TAP device
    which by itself has no connection to the outside world. Routing
    the traffic of that device requires IP forwarding on the host
    and a NAT rule which lets the traffic appear as if it would
    originate from the outgoing interface. The netfilter rules of
    the setup live in chains of their own, see setup_firewall().

    The network has to be one of the private networks flake-pilot
    selects from, and it must not collide with a network the host
    is connected to. The outgoing interface has to exist. Both are
    recorded in the network state along with the services of the
    host the VMs use. connect() takes them from there and nowhere
    else: the address of a VM, the route to it and its firewall
    rules are only ever created within the recorded network.

    If IP forwarding was switched off, the state records that
    flake-pilot switched it on, cleanup() switches it off again
    !*/
    let network = match validate_network(network) {
        Ok(network) => network,
        Err(error) => {
            error!("{error}");
            return false
        }
    };
    if let Err(error) = validate_outgoing_interface(outgoing_interface) {
        error!("{error}");
        return false
    }
    let _lock = match lock_network_state(true) {
        Some(lock) => lock,
        None => return false
    };
    let mut state = match read_network_state() {
        Some(state) => state,
        None => return false
    };
    info!(
        "Network setup of the host requested by {} (user ID {})",
        caller.name, caller.uid
    );
    if let Some(recorded) = state.get_network().filter(|recorded| *recorded != network) {
        warn!("Replacing the recorded network {recorded} by {network}");
        warn!("The applications which are connected to it have to be connected again");
    }
    match ip_forward_is_enabled() {
        Some(true) => info!("IP forwarding is already enabled"),
        _ => {
            if ! set_ip_forward(true) {
                return false
            }
            state.ip_forward_enabled = true;
        }
    }
    state.network = Some(network.to_string());
    state.outgoing_interface = Some(outgoing_interface.to_string());
    state.host_resolver = host_resolver;
    state.host_dhcp = host_dhcp;
    if ! write_network_state(&state) {
        return false
    }
    if ! setup_firewall(&network, outgoing_interface) {
        return false
    }
    delete_legacy_rules(outgoing_interface);
    true
}

pub fn cleanup(caller: &Caller) -> bool {
    /*!
    Delete the network setup of the host

    This is the counterpart of init(): the firewall setup is deleted
    as a whole, all TAP devices of the address allocations are
    deleted and IP forwarding is switched off again if flake-pilot
    switched it on. The address allocations and the recorded setup
    are kept. The host is left in the state it has after a reboot,
    calling init() and connect() again connects the applications
    with the addresses they had before
    !*/
    let _lock = match lock_network_state(true) {
        Some(lock) => lock,
        None => return false
    };
    let mut state = match read_network_state() {
        Some(state) => state,
        None => return false
    };
    info!(
        "Network cleanup of the host requested by {} (user ID {})",
        caller.name, caller.uid
    );
    let mut ok = true;
    for allocation in &state.allocations {
        ok &= delete_tap(&allocation.tap);
    }
    ok &= delete_firewall();
    let mut outgoing_interfaces: Vec<String> = Vec::new();
    outgoing_interfaces.extend(state.outgoing_interface.clone());
    // The system wide record of former versions belongs to root
    outgoing_interfaces.extend(
        read_network_config(false)
            .map(|network_config| network_config.outgoing_interface)
    );
    outgoing_interfaces.dedup();
    for outgoing_interface in outgoing_interfaces {
        if validate::interface_name(&outgoing_interface).is_ok() {
            delete_legacy_rules(&outgoing_interface);
        }
    }
    if state.ip_forward_enabled {
        if set_ip_forward(false) {
            state.ip_forward_enabled = false;
            ok &= write_network_state(&state);
        } else {
            ok = false
        }
    } else {
        info!("Keeping IP forwarding, it was not enabled by flake-pilot");
    }
    ok
}

pub fn connect(
    caller: &Caller, config_file: &str, instance: Option<&str>,
    uid: Option<u32>, prefer: Option<Ipv4Addr>, exclude: &[Ipv4Addr]
) -> bool {
    /*!
    Connect the given flake instance to the network of the VMs

    The instance gets an address of the recorded network, which is
    allocated in the network state along with the TAP device and
    the user it belongs to. The TAP device is created for the user
    who runs the VM, only that user can connect a VM to it, and a
    host route connects the address with the device. The VM is
    allowed to use the services of the host the setup was recorded
    for, see firewall::get_guest_service_rules(). The address is
    written to stdout, the caller writes it to the flake
    configuration
    !*/
    let vm = match get_vm_instance(caller, config_file, instance, uid) {
        Ok(vm) => vm,
        Err(error) => {
            error!("{error}");
            return false
        }
    };
    let _lock = match lock_network_state(true) {
        Some(lock) => lock,
        None => return false
    };
    let mut state = match read_network_state() {
        Some(state) => state,
        None => return false
    };
    let network = match state.get_network() {
        Some(network) => network,
        None => {
            error!("The network setup of the host is not recorded");
            error!("Please run 'flake-ctl firecracker network init' first");
            return false
        }
    };
    let services = state.get_services();
    if ! firewall_is_active() {
        error!("The firewall setup of the host is not active");
        error!(
            "Please run 'flake-ctl firecracker network init' first, \
            e.g after a reboot of the host"
        );
        return false
    }

    // 1. the address allocation
    let mut used: Vec<Ipv4Addr> = state.allocations.iter()
        .filter(|allocation| allocation.tap != vm.tap)
        .map(|allocation| allocation.address)
        .collect();
    used.extend(exclude.iter().filter(|address| network.contains(address)));
    let address = match select_address(&vm, prefer, &network, &state, &used) {
        Some(address) => address,
        None => return false
    };
    state.allocations.retain(|allocation| allocation.tap != vm.tap);
    state.allocations.push(Allocation {
        address,
        uid: vm.uid,
        tap: vm.tap.clone(),
        config_file: vm.config_file.to_string(),
        instance: vm.instance.clone()
    });
    if ! write_network_state(&state) {
        return false
    }

    // 2. the TAP device of the instance, owned by its user
    if ! create_tap(&vm.tap, vm.uid) {
        return false
    }

    // 3. the gateway on the TAP device
    if ! connect_tap(&vm.tap, &network) {
        return false
    }

    // 4. the host route to the address of the instance
    if ! route_address(&address, &vm.tap) {
        return false
    }

    // 5. the services of the host the instance may use
    if ! allow_guest_services(&vm.tap, &address, &network, services) {
        return false
    }
    println!("{address}");
    true
}

pub fn disconnect(
    caller: &Caller, config_file: &str, instance: Option<&str>,
    uid: Option<u32>
) -> bool {
    /*!
    Disconnect the given flake instance from the network of the VMs

    The firewall rules of the instance, its TAP device and its
    address allocation are deleted. The address becomes available
    again for another application
    !*/
    let vm = match get_vm_instance(caller, config_file, instance, uid) {
        Ok(vm) => vm,
        Err(error) => {
            error!("{error}");
            return false
        }
    };
    let _lock = match lock_network_state(true) {
        Some(lock) => lock,
        None => return false
    };
    let mut state = match read_network_state() {
        Some(state) => state,
        None => return false
    };

    // 1. the services of the host the instance was allowed to use
    if ! revoke_guest_services(&vm.tap) {
        return false
    }

    // 2. the TAP device of the instance
    if ! delete_tap(&vm.tap) {
        return false
    }

    // 3. the address allocation
    let allocations = state.allocations.len();
    state.allocations.retain(|allocation| allocation.tap != vm.tap);
    state.allocations.len() == allocations || write_network_state(&state)
}

fn get_vm_instance<'a>(
    caller: &Caller, config_file: &'a str, instance: Option<&str>,
    uid: Option<u32>
) -> Result<VmInstance<'a>, String> {
    /*!
    Provide the VM of the given flake instance after checking that
    the caller may act on it, see validate::flake_config() and
    validate::vm_user(). The TAP device is named after the user who
    runs the VM, the application and the instance selector
    !*/
    let flake = validate::flake_config(caller, config_file)?;
    let instance = instance.map(validate::instance_selector).transpose()?;
    let uid = validate::vm_user(caller, &flake, uid)?;
    let tap = get_tap_name(uid, &flake.meta_name(instance.as_deref()));
    Ok(VmInstance { config_file, instance, tap, uid })
}

fn validate_network(network: &str) -> Result<Ipv4Network, String> {
    /*!
    Check the given network of the VMs, see init()
    !*/
    let parsed = Ipv4Network::parse(network).ok_or(
        format!("Invalid network {network:?}")
    )?;
    if ! get_network_candidates().any(|candidate| candidate == parsed)
        || parsed.to_string() != network
    {
        return Err(format!(
            "{network} is not one of the private /{} networks flake-pilot \
            selects from", defaults::NETWORK_PREFIX_LEN
        ))
    }
    if network_collides(&parsed) {
        return Err(format!(
            "{network} collides with a network the host is connected to"
        ))
    }
    Ok(parsed)
}

fn validate_outgoing_interface(outgoing_interface: &str) -> Result<(), String> {
    /*!
    Check the given outgoing interface, see init()
    !*/
    validate::interface_name(outgoing_interface)?;
    if is_flake_tap_device(outgoing_interface) {
        return Err(format!(
            "{outgoing_interface} is the TAP device of a VM"
        ))
    }
    if ! Path::new(SYS_CLASS_NET).join(outgoing_interface).exists() {
        return Err(format!(
            "Interface {outgoing_interface} does not exist"
        ))
    }
    Ok(())
}

pub fn replace_file(path: &str, content: &str, mode: u32) -> bool {
    /*!
    Write the given content to the file at the given path

    The content is written to a temporary file in the same
    directory, which then takes the place of the file. A reader
    sees either the old or the new content. The directory is
    created if it does not exist yet
    !*/
    let directory = match Path::new(path).parent() {
        Some(directory) => directory,
        None => {
            error!("{path} has no parent directory");
            return false
        }
    };
    if let Err(error) = fs::create_dir_all(directory) {
        error!("Failed to create {}: {error}", directory.display());
        return false
    }
    let mut temp = match NamedTempFile::new_in(directory) {
        Ok(temp) => temp,
        Err(error) => {
            error!(
                "Failed to create a temporary file in {}: {error}",
                directory.display()
            );
            return false
        }
    };
    let written = temp.write_all(content.as_bytes())
        .and_then(|_| temp.as_file().sync_all())
        .and_then(|_| temp.as_file().set_permissions(
            fs::Permissions::from_mode(mode)
        ));
    if let Err(error) = written {
        error!("Failed to write {path}: {error}");
        return false
    }
    if let Err(error) = temp.persist(path) {
        error!("Failed to replace {path}: {error}");
        return false
    }
    true
}

pub fn select_address(
    flake: &VmInstance, configured: Option<Ipv4Addr>,
    network: &Ipv4Network, state: &NetworkState, used: &[Ipv4Addr]
) -> Option<Ipv4Addr> {
    /*!
    Select the address of the given flake instance

    The address allocated to the TAP device of the instance is
    kept. An address which is configured for the instance, which
    is the address the caller prefers, is kept as long as it is not
    allocated to another device. In all other cases a free one is
    taken, used lists the addresses which are not free. Only the
    address of a VM in the network is kept, never the network, the
    gateway or the broadcast address
    !*/
    let allocated = state.allocations.iter()
        .find(|allocation| allocation.tap == flake.tap)
        .map(|allocation| allocation.address)
        .filter(|address| is_vm_address(network, address));
    if let Some(address) = allocated {
        info!("Keeping allocated address {address}");
        return Some(address)
    }
    if let Some(address) = configured.filter(
        |address| is_vm_address(network, address)
    ) {
        match state.allocations.iter().find(
            |allocation| allocation.address == address
        ) {
            Some(allocation) if allocation.config_file == flake.config_file
                && allocation.instance == flake.instance =>
            {
                // The address of an instance is part of its flake
                // configuration, the instance of a system wide
                // flake can therefore only be connected for one
                // user at a time
                error!(
                    "The instance is already connected for the user with \
                    the ID {} through {}", allocation.uid, allocation.tap
                );
                error!(
                    "Please use an @NAME instance of its own for every user"
                );
                return None
            },
            Some(_) => { },
            None => {
                info!("Keeping configured address {address}");
                return Some(address)
            }
        }
    }
    let address = get_free_address(network, used)?;
    info!("Assigning address {address}");
    Some(address)
}

pub fn write_network_state(state: &NetworkState) -> bool {
    write_network_state_to(state, defaults::NETWORK_STATE)
}

fn write_network_state_to(state: &NetworkState, state_file: &str) -> bool {
    /*!
    Write the host wide network state

    The state is written to a temporary file next to the state
    file which then takes the place of the state file. A reader
    therefore never sees a partially written state. The file
    belongs to root and is readable for everybody, flake-ctl reads
    it with the privileges of the calling user
    !*/
    let yaml_data = match serde_yaml::to_string(state) {
        Ok(yaml_data) => yaml_data,
        Err(error) => {
            error!("Failed to serialize the network state: {error}");
            return false
        }
    };
    replace_file(state_file, &yaml_data, 0o644)
}

fn ip_forward_is_enabled() -> Option<bool> {
    /*!
    Check if IPv4 forwarding is switched on, None if unknown
    !*/
    fs::read_to_string(defaults::PROC_IP_FORWARD).ok()
        .map(|value| value.trim() == "1")
}

pub fn set_ip_forward(enable: bool) -> bool {
    /*!
    Switch IPv4 forwarding on the host on or off
    !*/
    let ip_forward = format!(
        "{}={}", defaults::SYSCTL_IP_FORWARD, if enable { 1 } else { 0 }
    );
    info!(
        "{} IP forwarding...", if enable { "Enabling" } else { "Disabling" }
    );
    let mut call = Command::new(defaults::SYSCTL_TOOL);
    call.arg("-w").arg(&ip_forward);
    run_ok(&mut call, &format!("run sysctl -w {ip_forward}"))
}

pub fn create_tap(tap: &str, uid: u32) -> bool {
    /*!
    Create the TAP device of a VM instance

    The device is the host local endpoint of the VM. Its name is
    the one the pilot passes to firecracker when it starts the
    instance of the application. firecracker runs as the calling
    user, the device is therefore created for the user with the
    given ID, who is the only one allowed to connect a VM to it.
    IPv6 is switched off on the device, the setup and its firewall
    rules are IPv4 only
    !*/
    if tap_exists(tap) {
        info!("Keeping existing TAP device {tap}");
    } else {
        info!("Creating TAP device {tap} for the user ID {uid}...");
        let mut call = Command::new(defaults::IP_TOOL);
        call.arg("tuntap").arg("add").arg(tap).arg("mode").arg("tap")
            .arg("user").arg(uid.to_string());
        if ! run_ok(&mut call, &format!("create TAP device {tap}")) {
            return false
        }
    }
    disable_ipv6(tap)
}

fn disable_ipv6(device: &str) -> bool {
    /*!
    Switch IPv6 off on the given network device

    Without IPv6 support in the kernel there is nothing to do
    !*/
    if ! Path::new(defaults::PROC_IPV6_CONF).join(device).exists() {
        return true
    }
    let disable_ipv6 = format!("net.ipv6.conf.{device}.disable_ipv6=1");
    let mut call = Command::new(defaults::SYSCTL_TOOL);
    call.arg("-w").arg(&disable_ipv6).stdout(Stdio::null());
    run_ok(&mut call, &format!("run sysctl -w {disable_ipv6}"))
}

pub fn delete_tap(tap: &str) -> bool {
    /*!
    Delete the TAP device of a VM instance

    The address of the device, its link state and the host route
    to the instance behind it are deleted along with it
    !*/
    if ! tap_exists(tap) {
        info!("No TAP device {tap} to delete");
        return true
    }
    info!("Deleting TAP device {tap}...");
    let mut call = Command::new(defaults::IP_TOOL);
    call.arg("tuntap").arg("del").arg(tap).arg("mode").arg("tap");
    run_ok(&mut call, &format!("delete TAP device {tap}"))
}

pub fn connect_tap(tap: &str, network: &Ipv4Network) -> bool {
    /*!
    Make the given TAP device the gateway of the VM behind it

    The traffic of the device to the outgoing interface and the
    way back are covered by the firewall setup created by init()
    !*/
    let gateway = network.gateway_route();
    if address_exists(tap, &gateway) {
        info!("Keeping existing address {gateway} on {tap}");
    } else {
        info!("Adding address {gateway} to {tap}...");
        let mut call = Command::new(defaults::IP_TOOL);
        call.arg("addr").arg("add").arg(&gateway).arg("dev").arg(tap);
        if ! run_ok(&mut call, &format!("add address {gateway} to {tap}")) {
            return false
        }
    }
    info!("Bringing up {tap}...");
    let mut call = Command::new(defaults::IP_TOOL);
    call.arg("link").arg("set").arg(tap).arg("up");
    run_ok(&mut call, &format!("bring up {tap}"))
}

pub fn route_address(address: &Ipv4Addr, tap: &str) -> bool {
    /*!
    Route the address of a VM instance to its TAP device

    Every TAP device provides the same gateway address of the
    private network. The route to that network which comes with
    it therefore points to one of the devices only and the
    traffic to all other instances would be sent to the wrong
    device. A host route for the address of the instance makes
    sure it is reached through the device it is connected to
    !*/
    let route = format!("{address}/{}", defaults::NETWORK_HOST_PREFIX_LEN);
    if route_exists(address, tap) {
        info!("Keeping existing route {route} dev {tap}");
        return true
    }
    info!("Adding route {route} dev {tap}...");
    let mut call = Command::new(defaults::IP_TOOL);
    call.arg("route").arg("add").arg(&route).arg("dev").arg(tap);
    run_ok(&mut call, &format!("add route {route} dev {tap}"))
}

fn route_exists(address: &Ipv4Addr, tap: &str) -> bool {
    /*!
    Check if the given device already routes the given address

    A host route is reported by iproute2 with the plain address,
    the prefix length of a single host is not shown
    !*/
    let mut call = Command::new(defaults::IP_TOOL);
    call.arg("-4").arg("-oneline").arg("route").arg("show").arg("dev")
        .arg(tap);
    let host_address = address.to_string();
    let host_route = format!(
        "{host_address}/{}", defaults::NETWORK_HOST_PREFIX_LEN
    );
    match call.output() {
        Ok(output) => String::from_utf8_lossy(&output.stdout).lines().any(
            |line| {
                let destination = line.split_whitespace().next()
                    .unwrap_or_default();
                destination == host_address || destination == host_route
            }
        ),
        Err(error) => {
            error!("Failed to execute {}: {error:?}", defaults::IP_TOOL);
            false
        }
    }
}

fn address_exists(device: &str, address: &str) -> bool {
    /*!
    Check if the given device already provides the given address
    !*/
    let mut call = Command::new(defaults::IP_TOOL);
    call.arg("-4").arg("-oneline").arg("addr").arg("show").arg("dev")
        .arg(device);
    match call.output() {
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .split_whitespace().any(|field| field == address),
        Err(error) => {
            error!("Failed to execute {}: {error:?}", defaults::IP_TOOL);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use flakes::network::{Allocation, Ipv4Network, NetworkState};

    use super::{select_address, VmInstance};

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
    fn test_select_address() {
        let private = network("172.16.0.0/24");
        let flake = VmInstance {
            config_file: "/usr/share/flakes/myapp.yaml",
            instance: None,
            tap: "tap-1000_myapp".to_string(),
            uid: 1000
        };
        let mut state = NetworkState::default();
        // no allocation and no configured address: the lowest free one
        assert_eq!(
            Some(address("172.16.0.3")),
            select_address(
                &flake, None, &private, &state, &[address("172.16.0.2")]
            )
        );
        // a configured address which is not allocated is kept
        assert_eq!(
            Some(address("172.16.0.9")),
            select_address(
                &flake, Some(address("172.16.0.9")), &private, &state, &[]
            )
        );
        // a configured address outside of the network is not kept
        assert_eq!(
            Some(address("172.16.0.2")),
            select_address(
                &flake, Some(address("10.0.0.2")), &private, &state, &[]
            )
        );
        // a configured address allocated to another flake is not
        // handed out twice
        state.allocations.push(allocation(
            "172.16.0.9", 1000, "tap-1000_other", "/other.yaml", None
        ));
        assert_eq!(
            Some(address("172.16.0.2")),
            select_address(
                &flake, Some(address("172.16.0.9")), &private, &state,
                &[address("172.16.0.9")]
            )
        );
        // the same instance connected for another user is refused,
        // its address is shared through the flake configuration
        state.allocations.push(allocation(
            "172.16.0.5", 1001, "tap-1001_myapp",
            "/usr/share/flakes/myapp.yaml", None
        ));
        assert_eq!(
            None,
            select_address(
                &flake, Some(address("172.16.0.5")), &private, &state, &[]
            )
        );
        // the address allocated to the device of the instance is kept
        state.allocations.push(allocation(
            "172.16.0.7", 1000, "tap-1000_myapp",
            "/usr/share/flakes/myapp.yaml", None
        ));
        assert_eq!(
            Some(address("172.16.0.7")),
            select_address(
                &flake, Some(address("172.16.0.5")), &private, &state, &[]
            )
        );
    }
}
