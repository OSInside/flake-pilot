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
use flakes::defaults::{
    TAP_DEVICE_PREFIX, FIREWALL_NAT_CHAIN, FIREWALL_FORWARD_CHAIN,
    FIREWALL_INPUT_CHAIN
};
use flakes::network::{
    get_network_config_file, get_tap_name, read_network_config, NetworkConfig,
    FirewallRule, get_tap_match, get_jump_rules, get_masquerade_rule
};
use flakes::registration;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{ErrorKind, Write};
use std::iter;
use std::net::Ipv4Addr;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::{Command, Stdio};
use tempfile::NamedTempFile;
use uzers::{get_current_uid, get_user_by_name, get_user_by_uid};

use crate::app_config::{
    AppConfig, AppFireCrackerEngine, AppFireCrackerInstance
};
use crate::defaults;
use crate::firecracker::run_as;

pub fn init(outgoing_interface: Option<&str>, usermode: bool) -> bool {
    /*!
    Prepare the host for NAT based VM networking

    Firecracker connects a VM to the host through a TUN/TAP device
    which by itself has no connection to the outside world. Routing
    the traffic of that device requires IP forwarding on the host
    and a NAT rule which lets the traffic appear as if it would
    originate from the given outgoing interface. Without a given
    outgoing interface the interface of the IPv4 default route of
    the host is used.

    The netfilter rules of the setup live in chains of their own,
    see setup_firewall(). Only traffic of the private network of
    the VMs leaving through the outgoing interface is masqueraded
    and forwarded. Traffic between the VMs, from the outside world
    to the VMs and from the VMs to the host is dropped, except for
    the services of the host a VM is allowed to use, see add().

    If IP forwarding was switched off, the state file records that
    flake-pilot switched it on, cleanup() switches it off again.

    The setup changes the network configuration of the host and is
    therefore performed via sudo. It is not persistent and has to
    be created again after a reboot of the host.

    The outgoing interface and the private network between the host
    and the VMs are stored such that the setup of the single flakes,
    done by add(), uses the same network and knows where to route
    its traffic to.

    Please note, the setup assumes there is no other firewall
    software active on the host. If the host firewall is managed
    by another tool, the rules have to be created with that tool
    instead
    !*/
    let outgoing_interface = match select_outgoing_interface(
        outgoing_interface
    ) {
        Some(outgoing_interface) => outgoing_interface,
        None => return false
    };
    let network = match select_network(usermode) {
        Some(network) => network,
        None => return false
    };
    let mut state = match read_network_state() {
        Some(state) => state,
        None => return false
    };
    match ip_forward_is_enabled() {
        Some(true) => info!("IP forwarding is already enabled"),
        _ => {
            if ! set_ip_forward(true) {
                return false
            }
            state.ip_forward_enabled = true;
        }
    }
    if ! write_network_state(&state) {
        return false
    }
    if ! setup_firewall(&network, &outgoing_interface) {
        return false
    }
    delete_legacy_rules(&outgoing_interface);
    if ! write_network_config(&outgoing_interface, &network, usermode) {
        return false
    }
    info!("Host is prepared for VM networking:");
    info!("  network: {network}");
    info!("  gateway: {}", network.gateway());
    info!("  outgoing interface: {outgoing_interface}");
    true
}

pub fn add(
    app: &str, instance: Option<&String>, user: Option<&String>,
    usermode: bool
) -> bool {
    /*!
    Connect the given flake application to the host network

    The flake gets an address of the private network between the
    host and the VMs written to its configuration. The address is
    allocated in the host wide state file, which keeps the addresses
    of all users apart. The TAP device it expects is created for the
    user who runs the VM, given by user, the calling user if not
    given, and a host route connects that address with that device.
    The VM is allowed to use the DHCP, DNS and NFS service of the
    host through its gateway, any other traffic to the host is
    dropped by the firewall setup created by init().

    Each instance of an application needs its own address and its
    own TAP device. Therefore the command has to be called for
    every @NAME instance selector the application is called with
    !*/
    let uid = match get_tap_uid(user, usermode) {
        Some(uid) => uid,
        None => return false
    };
    let flake = match get_flake_network(app, instance, uid, usermode) {
        Some(flake) => flake,
        None => return false
    };
    let outgoing_interface = match get_outgoing_interface(usermode) {
        Some(outgoing_interface) => outgoing_interface,
        None => return false
    };
    let network = match get_network(usermode) {
        Some(network) => network,
        None => return false
    };
    if ! firewall_is_active() {
        error!("The firewall setup of the host is not active");
        error!(
            "Please run 'flake-ctl firecracker network init' first, \
            e.g after a reboot of the host"
        );
        return false
    }
    let mut state = match read_network_state() {
        Some(state) => state,
        None => return false
    };

    // 1. the address allocation and the flake configuration
    let address = match configure_flake(
        &flake, uid, &network, &mut state, usermode
    ) {
        Some(address) => address,
        None => return false
    };
    if ! write_network_state(&state) {
        return false
    }

    // 2. the TAP device of the instance, owned by its user
    if ! create_tap(&flake.tap, uid) {
        return false
    }

    // 3. the gateway on the TAP device
    if ! connect_tap(&flake.tap, &network) {
        return false
    }

    // 4. the host route to the address of the instance
    if ! route_address(&address, &flake.tap) {
        return false
    }

    // 5. the services of the host the instance may use
    if ! allow_guest_services(&flake.tap, &address, &network) {
        return false
    }

    info!("Application {app} is connected:");
    info!("  address: {address}");
    info!("  gateway: {}", network.gateway());
    info!("  tap device: {} (user ID {uid})", flake.tap);
    info!("  outgoing interface: {outgoing_interface}");
    if let Some(instance) = flake.instance {
        info!("Call the application as: {app} {instance}");
    }
    true
}

pub fn remove(
    app: &str, instance: Option<&String>, user: Option<&String>,
    usermode: bool
) -> bool {
    /*!
    Disconnect the given flake application from the host network

    All of the setup created by add() is deleted. This is the
    firewall rules of the instance, its TAP device, the network
    setup in the flake configuration and the address allocation.
    The address becomes available again for another application.

    Called with an instance selector only the setup of that
    instance is deleted. The setup which is shared with the other
    instances of the application is kept as long as one of them
    is still connected
    !*/
    let uid = match get_tap_uid(user, usermode) {
        Some(uid) => uid,
        None => return false
    };
    let flake = match get_flake_network(app, instance, uid, usermode) {
        Some(flake) => flake,
        None => return false
    };
    let mut state = match read_network_state() {
        Some(state) => state,
        None => return false
    };

    // 1. the services of the host the instance was allowed to use
    if ! revoke_guest_services(&flake.tap) {
        return false
    }

    // 2. the TAP device of the instance
    if ! delete_tap(&flake.tap) {
        return false
    }

    // 3. the flake configuration
    if ! unconfigure_flake(&flake.config_file, flake.instance.as_deref()) {
        return false
    }

    // 4. the address allocation
    let allocations = state.allocations.len();
    state.allocations.retain(|allocation| allocation.tap != flake.tap);
    if state.allocations.len() != allocations
        && ! write_network_state(&state)
    {
        return false
    }

    info!("Application {app} is disconnected");
    true
}

pub fn cleanup() -> bool {
    /*!
    Delete the network setup of the host

    This is the counterpart of init(): the firewall setup is deleted
    as a whole, all TAP devices created by add() are deleted and IP
    forwarding is switched off again if flake-pilot switched it on.
    The address allocations and the network setup in the flake
    configurations are kept. The host is left in the state it has
    after a reboot, calling init() and add() again connects the
    applications with the addresses they had before
    !*/
    let mut state = match read_network_state() {
        Some(state) => state,
        None => return false
    };
    let mut ok = true;
    for allocation in &state.allocations {
        ok &= delete_tap(&allocation.tap);
    }
    ok &= delete_firewall();
    for usermode in [false, true] {
        if let Some(network_config) = read_network_config(usermode) {
            delete_legacy_rules(&network_config.outgoing_interface);
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
    if ok {
        info!("Network setup of the host is deleted");
    }
    ok
}

// ActiveTap is a TAP device of a flake application which is
// present on the host
pub struct ActiveTap {
    /// Name of the TAP device
    pub name: String,
    /// The '@NAME' instance selector the device was created for,
    /// None for the device of the application itself
    pub instance: Option<String>,
    /// ID of the user the device belongs to
    pub uid: u32
}

pub fn get_active_taps(app: &str, usermode: bool) -> Vec<ActiveTap> {
    /*!
    Provide the TAP devices of the given flake application which
    are present on the host

    An application is connected to the host network per instance
    and per user, each of them through a TAP device of its own.
    The devices are looked up in the address allocations of the
    host wide state, which record the device, the user and the
    instance of every connection add() has created
    !*/
    let app_basename = match get_app_basename(app) {
        Some(app_basename) => app_basename,
        None => return Vec::new()
    };
    let config_file = registration::config_file(&app_basename, usermode);
    let state = match read_network_state() {
        Some(state) => state,
        None => return Vec::new()
    };
    get_flake_taps(&state, &config_file)
        .into_iter()
        .filter(|active_tap| tap_exists(&active_tap.name))
        .collect()
}

fn get_flake_taps(state: &NetworkState, config_file: &str) -> Vec<ActiveTap> {
    /*!
    Provide the TAP devices the address allocations of the given
    state record for the flake of the given configuration file
    !*/
    state.allocations.iter()
        .filter(|allocation| allocation.config_file == config_file)
        .map(|allocation| ActiveTap {
            name: allocation.tap.clone(),
            instance: allocation.instance.clone(),
            uid: allocation.uid
        })
        .collect()
}

pub fn get_remove_command(app: &str, instance: Option<&str>, uid: u32) -> String {
    /*!
    Construct the 'flake-ctl firecracker network remove' call
    which deletes the network setup of the given instance

    The setup of an instance which was connected for another user
    than the caller is addressed by the name of that user
    !*/
    let mut command = format!(
        "flake-ctl firecracker network remove --app {app}"
    );
    if let Some(instance) = instance {
        command.push_str(&format!(" --instance {instance}"));
    }
    if uid != get_current_uid() {
        let user = get_user_by_uid(uid)
            .map(|user| user.name().to_string_lossy().to_string())
            .unwrap_or_else(|| uid.to_string());
        command.push_str(&format!(" --user {user}"));
    }
    command
}

// FlakeNetwork is the network identity of a flake application
struct FlakeNetwork {
    /// Path of the flake configuration file of the application
    config_file: String,
    /// The '@NAME' instance selector, None for the application itself
    instance: Option<String>,
    /// Name of the TAP device the instance is connected to
    tap: String
}

fn get_flake_network(
    app: &str, instance: Option<&String>, uid: u32, usermode: bool
) -> Option<FlakeNetwork> {
    /*!
    Provide the network identity of the given flake application

    The flake configuration to work on is looked up from the
    application path and the TAP device is the one the pilot
    connects the instance of the application to, if it is run by
    the user with the given ID
    !*/
    let app_basename = get_app_basename(app)?;
    let config_file = get_flake_config_file(app, usermode)?;
    let instance = instance.map(|instance| get_instance_name(instance));

    // The pilot connects the VM to the TAP device named after the
    // calling user, the application and the instance selector it
    // was called with
    let mut meta_name = app_basename;
    if let Some(ref instance) = instance {
        meta_name.push_str(instance);
    }
    let tap = get_tap_name(uid, &meta_name);

    Some(FlakeNetwork { config_file, instance, tap })
}

fn get_tap_uid(user: Option<&String>, usermode: bool) -> Option<u32> {
    /*!
    Provide the ID of the user the TAP device of an instance is
    created for

    The device belongs to the user who runs the VM, only that user
    can connect a VM to it. Without a given user this is the
    calling user. A flake registered by a user is only run by that
    user, naming another user is therefore only allowed for system
    wide flakes, which are set up by root and run by any user
    !*/
    let calling_uid = get_current_uid();
    let user = match user {
        Some(user) => user,
        None => return Some(calling_uid)
    };
    let uid = match get_user_by_name(user) {
        Some(user) => user.uid(),
        None => {
            error!("User {user} does not exist");
            return None
        }
    };
    if usermode && uid != calling_uid {
        error!(
            "A flake registered by a user is only run by that user, \
            --user can only name another user for system wide flakes"
        );
        return None
    }
    Some(uid)
}

pub fn get_app_basename(app: &str) -> Option<String> {
    /*!
    Provide the name the flake configuration of the given
    application is stored under
    !*/
    if ! app.starts_with('/') {
        error!("Application {app:?} must be specified with an absolute path");
        return None
    }
    match Path::new(app).file_name() {
        Some(app_basename) => Some(app_basename.to_string_lossy().to_string()),
        None => {
            error!("Failed to read the application name from {app}");
            None
        }
    }
}

pub fn get_flake_config_file(app: &str, usermode: bool) -> Option<String> {
    /*!
    Provide the path of the flake configuration of the given
    application

    The configuration is looked up from the application path in
    the flake registry the caller operates on
    !*/
    let app_basename = get_app_basename(app)?;
    let config_file = registration::config_file(&app_basename, usermode);
    if ! Path::new(&config_file).exists() {
        error!("No flake configuration found at {config_file}");
        error!("Please register the application first");
        return None
    }
    Some(config_file)
}

pub fn get_instance_name(instance: &str) -> String {
    /*!
    Provide the instance selector in its '@NAME' notation

    The selector is passed to the application with a leading '@'.
    For convenience the plain name is accepted as well
    !*/
    if instance.starts_with('@') {
        instance.to_string()
    } else {
        format!("@{instance}")
    }
}

fn configure_flake(
    flake: &FlakeNetwork, uid: u32, network: &Ipv4Network,
    state: &mut NetworkState, usermode: bool
) -> Option<Ipv4Addr> {
    /*!
    Allocate the address of the given flake instance and write the
    static network setup to its flake configuration

    The address is taken from the private network between the host
    and the VMs and is recorded in the address allocations of the
    given host wide state, along with the TAP device and the user
    it belongs to. An address which is allocated to another device
    or used by another flake registration is not handed out twice.
    An instance which is already connected keeps its address, this
    allows to call the setup again, e.g after a reboot of the host
    !*/
    let config_file = flake.config_file.as_str();
    let instance = flake.instance.as_deref();
    let mut yaml_config = read_flake_config(config_file)?;
    let engine_section = get_engine_section(
        &mut yaml_config, config_file, "network"
    )?;

    let configured = get_configured_address(engine_section, instance);
    let address = select_address(
        flake, configured, network, state,
        &get_used_addresses(state, &flake.tap, usermode)
    )?;
    state.allocations.retain(|allocation| allocation.tap != flake.tap);
    state.allocations.push(Allocation {
        address,
        uid,
        tap: flake.tap.clone(),
        config_file: config_file.to_string(),
        instance: flake.instance.clone()
    });

    let ip_option = format!(
        "ip={}::{}:{}::{}:off",
        address,
        network.gateway(),
        network.netmask(),
        defaults::NETWORK_GUEST_INTERFACE
    );
    let route_option = format!(
        "rd.route={}::{}",
        network.gateway_route(),
        defaults::NETWORK_GUEST_INTERFACE
    );
    let dns_option = format!("nameserver={}", defaults::NETWORK_DNS);

    // The route to the gateway and the name server are the same
    // for all instances and are therefore always set globally
    let boot_args = engine_section.boot_args.get_or_insert_with(Vec::new);
    set_boot_arg(boot_args, route_option);
    set_boot_arg(boot_args, dns_option);

    match instance {
        Some(instance) => {
            // Only the address is specific to the instance. The
            // global ip= setting stays in effect for a call of
            // the application without an instance selector
            let instances = engine_section.instance
                .get_or_insert_with(HashMap::new);
            let instance_key = get_instance_key(instances, instance);
            let instance_section = instances.entry(instance_key)
                .or_insert(AppFireCrackerInstance { boot_args: None });
            set_boot_arg(
                instance_section.boot_args.get_or_insert_with(Vec::new),
                ip_option
            );
        },
        None => set_boot_arg(boot_args, ip_option)
    }

    if ! write_flake_config(config_file, &yaml_config) {
        return None
    }
    info!("Updated {config_file}");
    Some(address)
}

pub fn get_instance_key(
    instances: &HashMap<String, AppFireCrackerInstance>, instance: &str
) -> String {
    /*!
    Provide the key of the instance section to write to

    A section for the instance which already exists is used no
    matter if it is keyed with or without the '@' prefix. A new
    section is created in the '@NAME' notation
    !*/
    let plain_name = instance.trim_start_matches('@');
    if ! instances.contains_key(instance) && instances.contains_key(plain_name)
    {
        return plain_name.to_string()
    }
    instance.to_string()
}

fn select_address(
    flake: &FlakeNetwork, configured: Option<Ipv4Addr>,
    network: &Ipv4Network, state: &NetworkState, used: &[Ipv4Addr]
) -> Option<Ipv4Addr> {
    /*!
    Select the address of the given flake instance

    The address allocated to the TAP device of the instance is
    kept. An address which is configured for the instance is kept
    as long as it is not allocated to another device, in all other
    cases a free one is taken, used lists the addresses which are
    not free. Addresses outside of the network are not kept
    !*/
    let allocated = state.allocations.iter()
        .find(|allocation| allocation.tap == flake.tap)
        .map(|allocation| allocation.address)
        .filter(|address| network.contains(address));
    if let Some(address) = allocated {
        info!("Keeping allocated address {address}");
        return Some(address)
    }
    if let Some(address) = configured.filter(
        |address| network.contains(address)
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

fn unconfigure_flake(config_file: &str, instance: Option<&str>) -> bool {
    /*!
    Delete the static network setup from the flake configuration

    Only the address is specific to an instance. The route to the
    gateway and the name server are shared and are therefore only
    deleted if no instance of the application is connected anymore
    !*/
    let mut yaml_config = match read_flake_config(config_file) {
        Some(yaml_config) => yaml_config,
        None => return false
    };
    let engine_section = match get_engine_section(
        &mut yaml_config, config_file, "network"
    ) {
        Some(engine_section) => engine_section,
        None => return false
    };

    match instance {
        Some(instance) => delete_instance_address(engine_section, instance),
        None => {
            if let Some(boot_args) = engine_section.boot_args.as_mut() {
                unset_boot_arg(boot_args, "ip");
            }
        }
    }
    if ! has_configured_address(engine_section) {
        if let Some(boot_args) = engine_section.boot_args.as_mut() {
            unset_boot_arg(boot_args, "rd.route");
            unset_boot_arg(boot_args, "nameserver");
        }
    }

    // Sections which became empty are dropped to leave the
    // configuration as it was before the setup was created
    if let Some(true) = engine_section.boot_args.as_ref().map(Vec::is_empty) {
        engine_section.boot_args = None;
    }
    if let Some(true) = engine_section.instance.as_ref().map(HashMap::is_empty)
    {
        engine_section.instance = None;
    }

    if ! write_flake_config(config_file, &yaml_config) {
        return false
    }
    info!("Updated {config_file}");
    true
}

fn delete_instance_address(
    engine_section: &mut AppFireCrackerEngine, instance: &str
) {
    /*!
    Delete the address of the given instance

    An instance section which provides nothing else is deleted
    along with the address
    !*/
    let instances = match engine_section.instance.as_mut() {
        Some(instances) => instances,
        None => return
    };
    let instance_key = if instances.contains_key(instance) {
        instance.to_string()
    } else {
        instance.trim_start_matches('@').to_string()
    };
    let instance_section = match instances.get_mut(&instance_key) {
        Some(instance_section) => instance_section,
        None => return
    };
    let is_empty = match instance_section.boot_args.as_mut() {
        Some(boot_args) => {
            unset_boot_arg(boot_args, "ip");
            boot_args.is_empty()
        },
        None => true
    };
    if is_empty {
        instances.remove(&instance_key);
    }
}

fn has_configured_address(engine_section: &AppFireCrackerEngine) -> bool {
    /*!
    Check if the flake still provides an address for itself or
    for one of its instances
    !*/
    if get_configured_address(engine_section, None).is_some() {
        return true
    }
    match engine_section.instance.as_ref() {
        Some(instances) => instances.values().any(
            |instance_section| instance_section.boot_args.as_deref()
                .and_then(get_boot_args_address).is_some()
        ),
        None => false
    }
}

pub fn get_engine_section<'a>(
    yaml_config: &'a mut AppConfig, config_file: &str, setup: &str
) -> Option<&'a mut AppFireCrackerEngine> {
    /*!
    Provide the firecracker runtime section of the given flake
    configuration
    !*/
    let engine_section = yaml_config.vm.as_mut()
        .and_then(|vm_config| vm_config.runtime.as_mut())
        .and_then(|runtime_section| runtime_section.firecracker.as_mut());
    if engine_section.is_none() {
        error!("{config_file} provides no firecracker runtime section");
        error!("Only firecracker flakes provide a {setup} setup");
    }
    engine_section
}

fn get_configured_address(
    engine_section: &AppFireCrackerEngine, instance: Option<&str>
) -> Option<Ipv4Addr> {
    /*!
    Provide the address which is configured for the given instance
    !*/
    let boot_args = match instance {
        Some(instance) => {
            let instances = engine_section.instance.as_ref()?;
            let instance_section = instances.get(instance).or_else(
                || instances.get(instance.trim_start_matches('@'))
            )?;
            instance_section.boot_args.as_ref()?
        },
        None => engine_section.boot_args.as_ref()?
    };
    get_boot_args_address(boot_args)
}

fn get_boot_args_address(boot_args: &[String]) -> Option<Ipv4Addr> {
    /*!
    Read the static address from the ip= option of the given
    boot_args. A dynamic setup, e.g 'ip=dhcp', provides none
    !*/
    boot_args.iter()
        .filter_map(|boot_arg| boot_arg.strip_prefix("ip="))
        .filter_map(|ip_option| ip_option.split(':').next())
        .find_map(|address| address.parse().ok())
}

// NetworkInfo is the network a VM instance is connected to
#[derive(Debug, Serialize)]
pub struct NetworkInfo {
    /// Address of the VM in the private network between the host
    /// and the VMs. A dynamic setup, e.g 'ip=dhcp', provides none
    pub address: Option<Ipv4Addr>,
    /// Name of the TAP device the VM is connected to
    pub tap: String
}

pub fn get_network_info(
    boot_args: &[String], uid: u32, meta_name: &str
) -> Option<NetworkInfo> {
    /*!
    Provide the network of the instance with the given meta name,
    run by the user with the given ID

    The network is read from the kernel commandline the pilot
    creates the VM with. An instance which is not connected to
    a network provides none
    !*/
    if ! has_network_setup(boot_args) {
        return None
    }
    Some(
        NetworkInfo {
            address: get_boot_args_address(boot_args),
            tap: get_tap_name(uid, meta_name)
        }
    )
}

fn has_network_setup(boot_args: &[String]) -> bool {
    /*!
    Check if the given kernel commandline configures a network

    The interface of a VM is only of use if the guest kernel is
    told to bring it up. This is done with the 'ip=' option which
    is written by add(). The values 'off' and 'none' explicitly
    switch the network of the guest off and are therefore treated
    like a missing option, the same way the pilot does
    !*/
    boot_args.iter()
        .filter_map(|boot_arg| boot_arg.strip_prefix("ip="))
        .any(|setup| setup != "off" && setup != "none")
}

pub fn get_effective_boot_args(
    engine_section: &AppFireCrackerEngine, instance: Option<&str>
) -> Vec<String> {
    /*!
    Provide the kernel commandline of the given instance

    The boot_args of the engine section are the base. An option
    which is also set in the section of the instance takes the
    place of the global setting of the same option. Options which
    are not set globally are appended. This is the same merge the
    pilot performs when it creates the VM
    !*/
    let boot_args = engine_section.boot_args.as_deref().unwrap_or_default();
    let instance_boot_args = instance
        .and_then(|instance| get_instance_boot_args(engine_section, instance))
        .unwrap_or_default();
    if instance_boot_args.is_empty() {
        return boot_args.to_vec()
    }
    let mut effective_boot_args: Vec<String> = Vec::new();
    let mut applied: Vec<&str> = Vec::new();
    for boot_arg in boot_args {
        let name = boot_arg_name(boot_arg);
        if ! instance_boot_args.iter().any(
            |instance_boot_arg| boot_arg_name(instance_boot_arg) == name
        ) {
            effective_boot_args.push(boot_arg.to_string());
            continue
        }
        // the instance setting(s) of this option take the place
        // of the global setting
        if ! applied.contains(&name) {
            applied.push(name);
            effective_boot_args.extend(
                instance_boot_args.iter()
                    .filter(
                        |instance_boot_arg|
                            boot_arg_name(instance_boot_arg) == name
                    )
                    .cloned()
            );
        }
    }
    // options which are not set globally are appended
    for boot_arg in instance_boot_args {
        if ! applied.contains(&boot_arg_name(boot_arg)) {
            effective_boot_args.push(boot_arg.to_string());
        }
    }
    effective_boot_args
}

pub fn get_instance_boot_args<'a>(
    engine_section: &'a AppFireCrackerEngine, instance: &str
) -> Option<&'a [String]> {
    /*!
    Provide the boot_args of the given instance

    A section which is keyed with the plain NAME is accepted as
    well, the same way the pilot reads it
    !*/
    let instances = engine_section.instance.as_ref()?;
    let instance_section = instances.get(instance).or_else(
        || instances.get(instance.trim_start_matches('@'))
    )?;
    instance_section.boot_args.as_deref()
}

fn get_used_addresses(
    state: &NetworkState, tap: &str, usermode: bool
) -> Vec<Ipv4Addr> {
    /*!
    Provide the addresses which are in use, except the one of the
    given TAP device

    These are the addresses allocated in the host wide state and,
    for flakes which were connected before the addresses got
    allocated there, the addresses configured in the flake
    registrations which can be read. In user mode this includes
    the system wide registrations because their instances share
    the network with the ones of the calling user
    !*/
    let mut used: Vec<Ipv4Addr> = state.allocations.iter()
        .filter(|allocation| allocation.tap != tap)
        .map(|allocation| allocation.address)
        .collect();
    let mut config_files = registration::config_files(usermode);
    if usermode {
        config_files.extend(registration::config_files(false));
    }
    for config_file in config_files {
        let config_file = config_file.to_string_lossy().to_string();
        let yaml_config = match read_flake_config(&config_file) {
            Some(yaml_config) => yaml_config,
            None => continue
        };
        let engine_section = match yaml_config.vm.as_ref()
            .and_then(|vm_config| vm_config.runtime.as_ref())
            .and_then(|runtime_section| runtime_section.firecracker.as_ref())
        {
            Some(engine_section) => engine_section,
            None => continue
        };
        if let Some(boot_args) = engine_section.boot_args.as_ref() {
            used.extend(get_boot_args_address(boot_args));
        }
        if let Some(instances) = engine_section.instance.as_ref() {
            for instance_section in instances.values() {
                if let Some(boot_args) = instance_section.boot_args.as_ref() {
                    used.extend(get_boot_args_address(boot_args));
                }
            }
        }
    }
    used
}

fn get_free_address(
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

pub fn set_boot_arg(boot_args: &mut Vec<String>, boot_arg: String) {
    /*!
    Set the given kernel commandline option

    An option of the same name which is already present is
    replaced, in all other cases the option is appended
    !*/
    let name = boot_arg_name(&boot_arg).to_string();
    match boot_args.iter().position(|arg| boot_arg_name(arg) == name) {
        Some(position) => boot_args[position] = boot_arg,
        None => boot_args.push(boot_arg)
    }
}

pub fn unset_boot_arg(boot_args: &mut Vec<String>, name: &str) {
    /*!
    Delete the kernel commandline option of the given name
    !*/
    boot_args.retain(|boot_arg| boot_arg_name(boot_arg) != name);
}

pub fn boot_arg_name(boot_arg: &str) -> &str {
    /*!
    Provide the name of a kernel commandline option
    !*/
    boot_arg.split('=').next().unwrap_or(boot_arg)
}

pub fn read_flake_config(config_file: &str) -> Option<AppConfig> {
    /*!
    Read the given flake configuration

    Only the configuration file itself is read. The optional
    drop-in files from the '.d' directory next to it are not
    merged in because the result is written back to this file
    !*/
    match AppConfig::from_file(Path::new(config_file)) {
        Ok(yaml_config) => Some(yaml_config),
        Err(error) => {
            error!("Failed to read {config_file}: {error:?}");
            None
        }
    }
}

pub fn write_flake_config(config_file: &str, yaml_config: &AppConfig) -> bool {
    /*!
    Write back the given flake configuration
    !*/
    match yaml_config.to_file(Path::new(config_file)) {
        Ok(_) => true,
        Err(error) => {
            error!("Failed to write {config_file}: {error:?}");
            false
        }
    }
}

fn get_outgoing_interface(usermode: bool) -> Option<String> {
    /*!
    Provide the interface the VM traffic is routed to

    This is the interface the host setup was created for. If there
    is no record of it, e.g because the setup was created manually,
    the interface of the default route is used
    !*/
    match find_outgoing_interface(usermode) {
        Some(interface) => Some(interface),
        None => {
            error!("Failed to detect the outgoing interface");
            error!(
                "Please run 'flake-ctl firecracker network init \
                --outgoing-interface <NAME>' first"
            );
            None
        }
    }
}

fn find_outgoing_interface(usermode: bool) -> Option<String> {
    /*!
    Look up the interface the VM traffic is routed to
    !*/
    if let Some(network_config) = read_network_config(usermode) {
        return Some(network_config.outgoing_interface)
    }
    let interface = get_default_route_interface()?;
    info!("No network setup record found");
    info!("Using interface of the default route: {interface}");
    Some(interface)
}

fn select_outgoing_interface(outgoing_interface: Option<&str>) -> Option<String> {
    /*!
    Provide the interface the VM traffic gets routed to

    A given interface is taken as it is, otherwise the interface
    of the IPv4 default route is used. That is the interface the
    host itself sends its traffic to the outside world through
    !*/
    if let Some(outgoing_interface) = outgoing_interface {
        return Some(outgoing_interface.to_string())
    }
    match get_default_route_interface() {
        Some(interface) => {
            info!("Using interface of the default route: {interface}");
            Some(interface)
        },
        None => {
            error!("Failed to detect the outgoing interface");
            error!("The host has no IPv4 default route");
            error!("Please specify it with --outgoing-interface");
            None
        }
    }
}

fn get_default_route_interface() -> Option<String> {
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

fn get_route_list_default_device(routes: &str) -> Option<String> {
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

// RecordedNetwork is the network of the host setup record
enum RecordedNetwork {
    /// The network the record provides
    Network(Ipv4Network),
    /// There is no record of a host setup
    Missing,
    /// The record exists but provides no valid network
    Invalid
}

fn read_recorded_network(usermode: bool) -> RecordedNetwork {
    /*!
    Read the private network from the host setup record
    !*/
    let network_config = match read_network_config(usermode) {
        Some(network_config) => network_config,
        None => return RecordedNetwork::Missing
    };
    // A record which predates the selection of the network
    // provides none, it was created for the preferred network
    let recorded_network = match network_config.network {
        Some(recorded_network) => recorded_network,
        None => return RecordedNetwork::Network(get_preferred_network())
    };
    match Ipv4Network::parse(&recorded_network) {
        Some(network) => RecordedNetwork::Network(network),
        None => {
            error!(
                "Invalid network {recorded_network:?} in the \
                network setup record"
            );
            RecordedNetwork::Invalid
        }
    }
}

fn select_network(usermode: bool) -> Option<Ipv4Network> {
    /*!
    Provide the private network to create the host setup for

    A network which is already recorded is kept as long as the
    host does not use it. This keeps the flakes which are
    connected to it reachable and allows to create the host
    setup again, e.g after a reboot. A recorded network which
    collides with a network of the host, e.g because the host
    was connected to it afterwards, is replaced
    !*/
    match read_recorded_network(usermode) {
        RecordedNetwork::Network(network) => {
            if ! network_collides(&network) {
                info!("Keeping recorded network {network}");
                return Some(network)
            }
            warn!("Recorded network {network} collides with a network \
                of the host"
            );
            warn!("The applications which are connected to it have to \
                be connected again"
            );
            let network = get_free_network()?;
            info!("Selecting network {network}");
            Some(network)
        },
        RecordedNetwork::Missing => {
            let network = get_free_network()?;
            info!("Selecting network {network}");
            Some(network)
        },
        RecordedNetwork::Invalid => None
    }
}

fn get_network(usermode: bool) -> Option<Ipv4Network> {
    /*!
    Provide the private network between the host and the VMs

    This is the network the host setup was created for. If there
    is no record of it, e.g because the setup was created
    manually, a network which does not collide with the networks
    of the host is used
    !*/
    match read_recorded_network(usermode) {
        RecordedNetwork::Network(network) => Some(network),
        RecordedNetwork::Missing => {
            let network = get_free_network()?;
            info!("No network setup record found");
            info!("Using free network: {network}");
            Some(network)
        },
        RecordedNetwork::Invalid => None
    }
}

pub fn get_guest_address(
    app: &str, instance: Option<&String>, usermode: bool
) -> Option<Ipv4Addr> {
    /*!
    Provide the address of the VM the given flake instance runs in

    The address is the static ip= option of the kernel commandline
    of the instance, as written by 'flake-ctl firecracker network
    add'. It is used to allow exactly this VM as a client, e.g in
    the NFS exports of the host. Only an address of the private
    network between the host and the VMs is accepted
    !*/
    let config_file = get_flake_config_file(app, usermode)?;
    let mut yaml_config = read_flake_config(&config_file)?;
    let engine_section = get_engine_section(
        &mut yaml_config, &config_file, "volume"
    )?;
    let instance = instance.map(|instance| get_instance_name(instance));
    let boot_args = get_effective_boot_args(
        engine_section, instance.as_deref()
    );
    let address = match get_boot_args_address(&boot_args) {
        Some(address) => address,
        None => {
            error!("No static guest address found in {config_file}");
            error!("The address is read from the ip= boot option");
            error!("{}", get_add_command_hint(app, instance.as_deref()));
            return None
        }
    };
    let network = get_network(usermode)?;
    if ! is_vm_address(&network, &address) {
        error!(
            "Guest address {address} of {config_file} is not a VM \
            address of the private network {network}"
        );
        return None
    }
    Some(address)
}

fn is_vm_address(network: &Ipv4Network, address: &Ipv4Addr) -> bool {
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

fn get_add_command_hint(app: &str, instance: Option<&str>) -> String {
    /*!
    Provide the command which connects the given flake instance
    to the private network
    !*/
    match instance {
        Some(instance) => format!(
            "Please run 'flake-ctl firecracker network add --app {app} \
            --instance {instance}' first"
        ),
        None => format!(
            "Please run 'flake-ctl firecracker network add --app {app}' first"
        )
    }
}

fn get_free_network() -> Option<Ipv4Network> {
    /*!
    Provide a private network which does not collide with the
    networks the host is connected to

    The preferred network is taken if the host does not use it,
    in all other cases the fallback ranges are searched for a
    free network
    !*/
    select_free_network(&get_host_networks()?)
}

fn select_free_network(host_networks: &[Ipv4Network]) -> Option<Ipv4Network> {
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

fn network_collides(network: &Ipv4Network) -> bool {
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

fn get_network_candidates() -> impl Iterator<Item = Ipv4Network> {
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

fn get_preferred_network() -> Ipv4Network {
    /*!
    Provide the network which is used if the host allows it
    !*/
    get_default_network(defaults::NETWORK_PREFERRED)
}

fn get_default_network(address: &str) -> Ipv4Network {
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

fn get_host_networks() -> Option<Vec<Ipv4Network>> {
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

fn get_interface_networks() -> Option<Vec<Ipv4Network>> {
    /*!
    Provide the networks of the addresses of the host interfaces
    !*/
    let mut call = Command::new(defaults::IP_TOOL);
    call.arg("-4").arg("-oneline").arg("addr").arg("show");
    Some(get_address_list_networks(&run_output(&mut call)?))
}

fn get_address_list_networks(addresses: &str) -> Vec<Ipv4Network> {
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

fn get_route_networks() -> Option<Vec<Ipv4Network>> {
    /*!
    Provide the networks the host has a route for
    !*/
    let mut call = Command::new(defaults::IP_TOOL);
    call.arg("-4").arg("-oneline").arg("route").arg("show");
    Some(get_route_list_networks(&run_output(&mut call)?))
}

fn get_route_list_networks(routes: &str) -> Vec<Ipv4Network> {
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

fn is_flake_tap_device(device: &str) -> bool {
    /*!
    Check if the given device is a TAP device of a flake

    Devices attached to a parent device are reported in the
    'NAME@PARENT' notation, in the address list the name of the
    device is followed by a colon
    !*/
    device
        .split('@').next().unwrap_or_default()
        .trim_end_matches(':')
        .starts_with(TAP_DEVICE_PREFIX)
}

fn get_field_value<'a>(line: &'a str, name: &str) -> Option<&'a str> {
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

fn run_output(call: &mut Command) -> Option<String> {
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
struct Ipv4Network {
    /// Address of the network itself, e.g 172.16.0.0
    address: Ipv4Addr,
    /// Number of leading bits which form the network part
    prefix_len: u8
}

impl Ipv4Network {
    fn new(address: Ipv4Addr, prefix_len: u8) -> Ipv4Network {
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

    fn parse(network: &str) -> Option<Ipv4Network> {
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

    fn netmask_bits(prefix_len: u8) -> u32 {
        /*!
        Provide the netmask of the given prefix length
        !*/
        match prefix_len {
            0 => 0,
            prefix_len => u32::MAX << (32 - prefix_len as u32)
        }
    }

    fn netmask(&self) -> Ipv4Addr {
        /*!
        Provide the netmask of the network, e.g 255.255.255.0
        !*/
        Ipv4Addr::from(Ipv4Network::netmask_bits(self.prefix_len))
    }

    fn gateway(&self) -> Ipv4Addr {
        /*!
        Provide the address of the host side of the network. It
        is the first address of the network and is configured on
        the TAP device of every instance
        !*/
        Ipv4Addr::from(u32::from(self.address) + 1)
    }

    fn gateway_route(&self) -> String {
        /*!
        Provide the route to the gateway as the guest expects it
        in its rd.route= option, e.g 172.16.0.1/24
        !*/
        format!("{}/{}", self.gateway(), self.prefix_len)
    }

    fn broadcast(&self) -> u32 {
        /*!
        Provide the last address of the network
        !*/
        u32::from(self.address) | ! Ipv4Network::netmask_bits(self.prefix_len)
    }

    fn hosts(&self) -> impl Iterator<Item = Ipv4Addr> {
        /*!
        Provide the addresses of the network which can be handed
        out. The network and the broadcast address are not part
        of them
        !*/
        (u32::from(self.address) + 1..self.broadcast()).map(Ipv4Addr::from)
    }

    fn contains(&self, address: &Ipv4Addr) -> bool {
        /*!
        Check if the given address belongs to the network
        !*/
        u32::from(*address) & Ipv4Network::netmask_bits(self.prefix_len)
            == u32::from(self.address)
    }

    fn overlaps(&self, other: &Ipv4Network) -> bool {
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

fn write_network_config(
    outgoing_interface: &str, network: &Ipv4Network, usermode: bool
) -> bool {
    /*!
    Record the host network setup

    The record allows the setup of the single flakes to find the
    interface their traffic has to be routed to and the private
    network their addresses are taken from
    !*/
    let config_file = get_network_config_file(usermode);
    let network_config = NetworkConfig {
        outgoing_interface: outgoing_interface.to_string(),
        network: Some(network.to_string())
    };
    let yaml_data = match serde_yaml::to_string(&network_config) {
        Ok(yaml_data) => yaml_data,
        Err(error) => {
            error!("Failed to serialize the network setup: {error:?}");
            return false
        }
    };
    if let Some(config_dir) = Path::new(&config_file).parent() {
        if let Err(error) = fs::create_dir_all(config_dir) {
            error!("Failed to create {}: {error:?}", config_dir.display());
            return false
        }
    }
    match fs::write(&config_file, yaml_data) {
        Ok(_) => {
            info!("Created {config_file}");
            true
        },
        Err(error) => {
            error!("Failed to write {config_file}: {error:?}");
            false
        }
    }
}

// NetworkState is the host wide record of the network setup,
// see defaults::NETWORK_STATE
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
struct NetworkState {
    /// IP forwarding was switched off and has been switched on by
    /// init(), cleanup() switches it off again
    #[serde(default)]
    ip_forward_enabled: bool,
    /// The addresses handed out to the VM instances of all users
    #[serde(default)]
    allocations: Vec<Allocation>
}

// Allocation is an address of the private network handed out to
// the instance of a flake application run by one user
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Allocation {
    /// Address of the VM instance
    address: Ipv4Addr,
    /// ID of the user who runs the instance and owns its TAP device
    uid: u32,
    /// Name of the TAP device the instance is connected to
    tap: String,
    /// Path of the flake configuration of the application
    config_file: String,
    /// The '@NAME' instance selector, None for the application itself
    #[serde(default)]
    instance: Option<String>
}

fn read_network_state() -> Option<NetworkState> {
    /*!
    Read the host wide network state

    There is no state before the first network setup, which is
    read as an empty one. The state is only trusted if it belongs
    to root, everybody else could hand out addresses with it
    !*/
    let state_file = defaults::NETWORK_STATE;
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

fn write_network_state(state: &NetworkState) -> bool {
    /*!
    Write the host wide network state

    The state is written through a temporary copy which is
    installed as root, the state file belongs to root
    !*/
    let state_file = defaults::NETWORK_STATE;
    let yaml_data = match serde_yaml::to_string(state) {
        Ok(yaml_data) => yaml_data,
        Err(error) => {
            error!("Failed to serialize the network state: {error}");
            return false
        }
    };
    let mut temp = match NamedTempFile::new_in(defaults::TEMP_DIR) {
        Ok(temp) => temp,
        Err(error) => {
            error!(
                "Failed to create temp file in {}: {error}",
                defaults::TEMP_DIR
            );
            return false
        }
    };
    if let Err(error) = temp.write_all(yaml_data.as_bytes())
        .and_then(|_| temp.flush())
    {
        error!("Failed to write temp network state: {error}");
        return false
    }
    let mut call = run_as(defaults::INSTALL_TOOL, "root");
    call.arg("-D").arg("-m").arg("644")
        .arg("-o").arg("root").arg("-g").arg("root")
        .arg(temp.path()).arg(state_file);
    run_ok(&mut call, &format!("install {state_file}"))
}

fn ip_forward_is_enabled() -> Option<bool> {
    /*!
    Check if IPv4 forwarding is switched on, None if unknown
    !*/
    fs::read_to_string(defaults::PROC_IP_FORWARD).ok()
        .map(|value| value.trim() == "1")
}

fn set_ip_forward(enable: bool) -> bool {
    /*!
    Switch IPv4 forwarding on the host on or off
    !*/
    let ip_forward = format!(
        "{}={}", defaults::SYSCTL_IP_FORWARD, if enable { 1 } else { 0 }
    );
    info!(
        "{} IP forwarding...", if enable { "Enabling" } else { "Disabling" }
    );
    let mut call = run_as(defaults::SYSCTL_TOOL, "root");
    call.arg("-w").arg(&ip_forward);
    run_ok(&mut call, &format!("run sysctl -w {ip_forward}"))
}

// Position a rule is created at in its chain. Accepting rules are
// inserted at the top, dropping rules are appended. This keeps
// every accepting rule in front of the drop rules, no matter in
// which order the rules are created
enum RulePosition {
    Top,
    Bottom
}

fn get_firewall_chains() -> [(&'static str, &'static str); 3] {
    /*!
    Provide the table and the name of the chains of the host
    firewall setup
    !*/
    [
        ("nat", FIREWALL_NAT_CHAIN),
        ("filter", FIREWALL_FORWARD_CHAIN),
        ("filter", FIREWALL_INPUT_CHAIN)
    ]
}

fn get_firewall_rules(
    network: &str, outgoing_interface: &str
) -> Vec<(FirewallRule, RulePosition)> {
    /*!
    Provide the rules of the host firewall setup and the position
    they are created at

    The traffic of the private network of the VMs is masqueraded
    and forwarded if it leaves through the outgoing interface. The
    answers to it may pass back. All other traffic which comes from
    or goes to a TAP device is dropped: traffic between two VMs,
    traffic from the outside world to a VM and traffic from a VM to
    another interface of the host. Traffic from a VM to the host
    itself is dropped as well, except for answers to traffic of the
    host and the services add() allows for each instance
    !*/
    let tap_match = get_tap_match();
    vec![
        (get_masquerade_rule(network, outgoing_interface), RulePosition::Bottom),
        (
            FirewallRule::new("filter", FIREWALL_FORWARD_CHAIN, &[
                "-i", &tap_match, "-s", network, "-o", outgoing_interface,
                "-j", "ACCEPT"
            ]),
            RulePosition::Top
        ),
        (
            FirewallRule::new("filter", FIREWALL_FORWARD_CHAIN, &[
                "-o", &tap_match, "-m", "conntrack",
                "--ctstate", "RELATED,ESTABLISHED", "-j", "ACCEPT"
            ]),
            RulePosition::Top
        ),
        (
            FirewallRule::new("filter", FIREWALL_FORWARD_CHAIN, &[
                "-i", &tap_match, "-j", "DROP"
            ]),
            RulePosition::Bottom
        ),
        (
            FirewallRule::new("filter", FIREWALL_FORWARD_CHAIN, &[
                "-o", &tap_match, "-j", "DROP"
            ]),
            RulePosition::Bottom
        ),
        (
            FirewallRule::new("filter", FIREWALL_INPUT_CHAIN, &[
                "-m", "conntrack",
                "--ctstate", "RELATED,ESTABLISHED", "-j", "ACCEPT"
            ]),
            RulePosition::Top
        ),
        (
            FirewallRule::new("filter", FIREWALL_INPUT_CHAIN, &["-j", "DROP"]),
            RulePosition::Bottom
        )
    ]
}

fn get_guest_service_rules(
    tap: &str, address: &Ipv4Addr, network: &Ipv4Network
) -> Vec<FirewallRule> {
    /*!
    Provide the rules which let the VM behind the given TAP device
    use the services of the host

    The VM may reach the DHCP, DNS and NFS service of the host
    through its gateway. Apart from DHCP, which is used before the
    VM has an address, the traffic has to come from the address of
    the VM and from its own TAP device. A VM can therefore not use
    the services, e.g the NFS exports, of another VM
    !*/
    let address = address.to_string();
    let gateway = network.gateway().to_string();
    let mut rules = vec![
        FirewallRule::new("filter", FIREWALL_INPUT_CHAIN, &[
            "-i", tap, "-p", "udp",
            "--sport", defaults::NETWORK_DHCP_CLIENT_PORT,
            "--dport", defaults::NETWORK_DHCP_SERVER_PORT,
            "-j", "ACCEPT"
        ])
    ];
    for (protocol, port) in [
        ("udp", defaults::NETWORK_DNS_PORT),
        ("tcp", defaults::NETWORK_DNS_PORT),
        ("tcp", defaults::NETWORK_NFS_PORT)
    ] {
        rules.push(
            FirewallRule::new("filter", FIREWALL_INPUT_CHAIN, &[
                "-i", tap, "-s", &address, "-d", &gateway,
                "-p", protocol, "--dport", port, "-j", "ACCEPT"
            ])
        )
    }
    rules
}

fn setup_firewall(network: &Ipv4Network, outgoing_interface: &str) -> bool {
    /*!
    Create the chains and rules of the host firewall setup

    The chains are created first and are entered from the builtin
    chains last, traffic is therefore only passed to them once they
    are complete. Chains and rules which already exist are kept,
    this allows to call the setup more than once
    !*/
    for (table, chain) in get_firewall_chains() {
        if ! create_chain(table, chain) {
            return false
        }
    }
    for (rule, position) in get_firewall_rules(
        &network.to_string(), outgoing_interface
    ) {
        if ! add_rule(&rule, position) {
            return false
        }
    }
    for rule in get_jump_rules() {
        if ! add_rule(&rule, RulePosition::Top) {
            return false
        }
    }
    true
}

fn delete_firewall() -> bool {
    /*!
    Delete the chains and rules of the host firewall setup

    The jump rules into the chains are deleted first, the chains
    and the rules in them afterwards
    !*/
    let mut ok = true;
    for rule in get_jump_rules() {
        ok &= delete_rule(&rule);
    }
    for (table, chain) in get_firewall_chains() {
        ok &= delete_chain(table, chain);
    }
    ok
}

fn firewall_is_active() -> bool {
    /*!
    Check if the host firewall setup is complete, the chains are
    present and are entered from the builtin chains
    !*/
    get_firewall_chains().iter().all(
        |(table, chain)| chain_exists(table, chain)
    ) && get_jump_rules().iter().all(rule_exists)
}

fn allow_guest_services(
    tap: &str, address: &Ipv4Addr, network: &Ipv4Network
) -> bool {
    /*!
    Let the VM behind the given TAP device use the services of the
    host, see get_guest_service_rules()
    !*/
    get_guest_service_rules(tap, address, network).iter().all(
        |rule| add_rule(rule, RulePosition::Top)
    )
}

fn revoke_guest_services(tap: &str) -> bool {
    /*!
    Delete the rules which let the VM behind the given TAP device
    use the services of the host

    All rules of the input chain which match the device are
    deleted, no matter which address they were created for
    !*/
    if ! chain_exists("filter", FIREWALL_INPUT_CHAIN) {
        info!("No {FIREWALL_INPUT_CHAIN} chain, no rules of {tap} to delete");
        return true
    }
    delete_listed_rules("filter", FIREWALL_INPUT_CHAIN, |spec| {
        spec.windows(2).any(|pair| pair[0] == "-i" && pair[1] == tap)
    })
}

fn delete_legacy_rules(outgoing_interface: &str) {
    /*!
    Delete the rules former versions of flake-pilot created

    Former versions created their rules in the builtin chains: a
    FORWARD rule per TAP device, which is recognized by the name of
    the device and is deleted, as well as a MASQUERADE rule for all
    traffic of the outgoing interface and a FORWARD rule for all
    answers. The latter two cannot be told apart from rules created
    by other means and are therefore only reported
    !*/
    delete_listed_rules("filter", "FORWARD", |spec| {
        spec.len() == 6
            && spec[0] == "-i" && spec[1].starts_with(TAP_DEVICE_PREFIX)
            && spec[2] == "-o" && spec[4] == "-j" && spec[5] == "ACCEPT"
    });
    for rule in [
        FirewallRule::new(
            "nat", "POSTROUTING",
            &["-o", outgoing_interface, "-j", "MASQUERADE"]
        ),
        FirewallRule::new(
            "filter", "FORWARD", &[
                "-m", "conntrack", "--ctstate", "RELATED,ESTABLISHED",
                "-j", "ACCEPT"
            ]
        )
    ] {
        if rule_exists(&rule) {
            warn!(
                "Found a {} rule which former versions of flake-pilot \
                created for all traffic:", rule.chain
            );
            warn!(
                "  iptables -t {} -A {} {}",
                rule.table, rule.chain, rule.spec.join(" ")
            );
            warn!(
                "If it was not created by other means, delete it with: \
                iptables -t {} -D {} {}",
                rule.table, rule.chain, rule.spec.join(" ")
            );
        }
    }
}

fn iptables(table: &str) -> Command {
    /*!
    Create an iptables call for the given table
    !*/
    let mut call = run_as(defaults::IPTABLES_TOOL, "root");
    call.arg("-t").arg(table);
    call
}

fn iptables_ok(call: &mut Command) -> bool {
    /*!
    Run the given iptables call quietly and tell whether it
    succeeded. A missing rule or chain is reported on stderr,
    which is not an error in this context
    !*/
    call.stdout(Stdio::null()).stderr(Stdio::null());
    match call.status() {
        Ok(status) => status.success(),
        Err(_) => false
    }
}

fn chain_exists(table: &str, chain: &str) -> bool {
    /*!
    Check if the given chain exists
    !*/
    let mut call = iptables(table);
    call.arg("-S").arg(chain);
    iptables_ok(&mut call)
}

fn create_chain(table: &str, chain: &str) -> bool {
    /*!
    Create the given chain if it does not exist
    !*/
    if chain_exists(table, chain) {
        info!("Keeping existing {chain} chain");
        return true
    }
    info!("Creating {chain} chain...");
    let mut call = iptables(table);
    call.arg("-N").arg(chain);
    run_ok(&mut call, &format!("create {chain} chain"))
}

fn delete_chain(table: &str, chain: &str) -> bool {
    /*!
    Delete the given chain along with its rules if it exists
    !*/
    if ! chain_exists(table, chain) {
        info!("No {chain} chain to delete");
        return true
    }
    info!("Deleting {chain} chain...");
    let mut flush = iptables(table);
    flush.arg("-F").arg(chain);
    let mut delete = iptables(table);
    delete.arg("-X").arg(chain);
    run_ok(&mut flush, &format!("flush {chain} chain"))
        && run_ok(&mut delete, &format!("delete {chain} chain"))
}

fn rule_exists(rule: &FirewallRule) -> bool {
    /*!
    Check if the given rule is active
    !*/
    let mut call = iptables(rule.table);
    call.arg("-C").arg(&rule.chain).args(&rule.spec);
    iptables_ok(&mut call)
}

fn add_rule(rule: &FirewallRule, position: RulePosition) -> bool {
    /*!
    Create the given rule at the given position of its chain

    A rule which is already active is not created again. This
    allows to call the setup more than once without stacking up
    duplicates of the same rule
    !*/
    if rule_exists(rule) {
        return true
    }
    let mut call = iptables(rule.table);
    match position {
        RulePosition::Top => call.arg("-I").arg(&rule.chain).arg("1"),
        RulePosition::Bottom => call.arg("-A").arg(&rule.chain)
    };
    call.args(&rule.spec);
    run_ok(
        &mut call,
        &format!("add {} rule {}", rule.chain, rule.spec.join(" "))
    )
}

fn delete_rule(rule: &FirewallRule) -> bool {
    /*!
    Delete the given rule if it is active
    !*/
    if ! rule_exists(rule) {
        return true
    }
    let mut call = iptables(rule.table);
    call.arg("-D").arg(&rule.chain).args(&rule.spec);
    run_ok(
        &mut call,
        &format!("delete {} rule {}", rule.chain, rule.spec.join(" "))
    )
}

fn delete_listed_rules(
    table: &str, chain: &str, matches: impl Fn(&[String]) -> bool
) -> bool {
    /*!
    Delete the rules of the given chain which match

    The rules are read in the notation iptables lists them with,
    each rule is matched by its specification without the leading
    '-A CHAIN'
    !*/
    let mut call = iptables(table);
    call.arg("-S").arg(chain).stderr(Stdio::null());
    let rules = match call.output() {
        Ok(output) if output.status.success() => {
            get_listed_rules(&String::from_utf8_lossy(&output.stdout), chain)
        },
        _ => {
            warn!("Failed to read the rules of the {chain} chain");
            return false
        }
    };
    let mut ok = true;
    for spec in rules.iter().filter(|spec| matches(spec)) {
        info!("Deleting {chain} rule {}...", spec.join(" "));
        let mut call = iptables(table);
        call.arg("-D").arg(chain).args(spec);
        ok &= run_ok(&mut call, &format!("delete {chain} rule"));
    }
    ok
}

fn get_listed_rules(rule_list: &str, chain: &str) -> Vec<Vec<String>> {
    /*!
    Read the rules of the given chain from the output of
    'iptables -S CHAIN'

    Each rule is listed as '-A CHAIN SPEC', the policy of a builtin
    chain and the creation of a custom chain are listed as '-P' and
    '-N' and are not rules
    !*/
    rule_list.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            if fields.next() != Some("-A") || fields.next() != Some(chain) {
                return None
            }
            Some(fields.map(ToOwned::to_owned).collect())
        })
        .collect()
}

fn create_tap(tap: &str, uid: u32) -> bool {
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
        let mut call = run_as(defaults::IP_TOOL, "root");
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
    let mut call = run_as(defaults::SYSCTL_TOOL, "root");
    call.arg("-w").arg(&disable_ipv6).stdout(Stdio::null());
    run_ok(&mut call, &format!("run sysctl -w {disable_ipv6}"))
}

fn delete_tap(tap: &str) -> bool {
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
    let mut call = run_as(defaults::IP_TOOL, "root");
    call.arg("tuntap").arg("del").arg(tap).arg("mode").arg("tap");
    run_ok(&mut call, &format!("delete TAP device {tap}"))
}

fn tap_exists(tap: &str) -> bool {
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

fn get_tuntap_device_name(tuntap_list_line: &str) -> &str {
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

fn connect_tap(tap: &str, network: &Ipv4Network) -> bool {
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
        let mut call = run_as(defaults::IP_TOOL, "root");
        call.arg("addr").arg("add").arg(&gateway).arg("dev").arg(tap);
        if ! run_ok(&mut call, &format!("add address {gateway} to {tap}")) {
            return false
        }
    }
    info!("Bringing up {tap}...");
    let mut call = run_as(defaults::IP_TOOL, "root");
    call.arg("link").arg("set").arg(tap).arg("up");
    run_ok(&mut call, &format!("bring up {tap}"))
}

fn route_address(address: &Ipv4Addr, tap: &str) -> bool {
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
    let mut call = run_as(defaults::IP_TOOL, "root");
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

fn run_ok(call: &mut Command, action: &str) -> bool {
    /*!
    Run the given call and tell whether it succeeded
    !*/
    match call.status() {
        Ok(status) => {
            if ! status.success() {
                error!("Failed to {action}: {status}");
                return false
            }
            true
        },
        Err(error) => {
            error!("Failed to {action}: {error:?}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::net::Ipv4Addr;

    use crate::app_config::{AppFireCrackerEngine, AppFireCrackerInstance};

    use flakes::network::get_tap_name;
    use uzers::get_current_uid;

    use super::{
        get_address_list_networks, get_effective_boot_args, get_flake_taps,
        get_firewall_rules, get_free_address, get_guest_service_rules,
        get_listed_rules, get_network_candidates, get_network_info,
        get_preferred_network, get_remove_command,
        get_route_list_default_device, get_route_list_networks,
        is_vm_address, select_address, select_free_network,
        select_outgoing_interface, Allocation, FlakeNetwork, Ipv4Network,
        NetworkConfig, NetworkState, RulePosition
    };

    fn network(network: &str) -> Ipv4Network {
        Ipv4Network::parse(network).unwrap()
    }

    fn address(address: &str) -> Ipv4Addr {
        address.parse().unwrap()
    }

    fn boot_args(boot_args: &[&str]) -> Vec<String> {
        boot_args.iter().map(ToString::to_string).collect()
    }

    fn engine_section(
        global: &[&str], instances: &[(&str, &[&str])]
    ) -> AppFireCrackerEngine {
        let mut instance_sections: HashMap<String, AppFireCrackerInstance> =
            HashMap::new();
        for (instance, instance_boot_args) in instances {
            instance_sections.insert(
                instance.to_string(),
                AppFireCrackerInstance {
                    boot_args: Some(boot_args(instance_boot_args))
                }
            );
        }
        AppFireCrackerEngine {
            boot_args: Some(boot_args(global)),
            overlay_size: None,
            rootfs_image_path: None,
            kernel_image_path: None,
            initrd_path: None,
            mem_size_mib: None,
            vcpu_count: None,
            cache_type: None,
            instance: if instance_sections.is_empty() {
                None
            } else {
                Some(instance_sections)
            }
        }
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
    fn test_get_flake_taps() {
        let state = NetworkState {
            ip_forward_enabled: false,
            allocations: vec![
                allocation(
                    "172.16.0.2", 1000, "tap-1000_myapp",
                    "/home/tux/.config/flakes/myapp.yaml", None
                ),
                allocation(
                    "172.16.0.3", 1000, "tap-1000_myapp_one",
                    "/home/tux/.config/flakes/myapp.yaml", Some("@one")
                ),
                allocation(
                    "172.16.0.4", 0, "tap-0_myapp",
                    "/usr/share/flakes/myapp.yaml", None
                )
            ]
        };
        // the devices of the flake of the given configuration,
        // another flake of the same name is not taken into account
        let flake_taps = get_flake_taps(
            &state, "/home/tux/.config/flakes/myapp.yaml"
        );
        assert_eq!(
            vec!["tap-1000_myapp", "tap-1000_myapp_one"],
            flake_taps.iter()
                .map(|active_tap| active_tap.name.as_str())
                .collect::<Vec<&str>>()
        );
        assert_eq!(
            vec![None, Some("@one")],
            flake_taps.iter()
                .map(|active_tap| active_tap.instance.as_deref())
                .collect::<Vec<Option<&str>>>()
        );
        assert!(flake_taps.iter().all(|active_tap| active_tap.uid == 1000));
        assert!(get_flake_taps(&state, "/other.yaml").is_empty());
    }

    #[test]
    fn test_network_state() {
        // a state file without the optional entries is read as empty
        let state: NetworkState = serde_yaml::from_str("{}").unwrap();
        assert_eq!(NetworkState::default(), state);
        let state = NetworkState {
            ip_forward_enabled: true,
            allocations: vec![
                allocation(
                    "172.16.0.2", 1000, "tap-1000_myapp",
                    "/home/tux/.config/flakes/myapp.yaml", Some("@one")
                )
            ]
        };
        let yaml_data = serde_yaml::to_string(&state).unwrap();
        assert!(yaml_data.contains("address: 172.16.0.2"));
        assert_eq!(
            state, serde_yaml::from_str::<NetworkState>(&yaml_data).unwrap()
        );
    }

    #[test]
    fn test_select_address() {
        let private = network("172.16.0.0/24");
        let flake = FlakeNetwork {
            config_file: "/usr/share/flakes/myapp.yaml".to_string(),
            instance: None,
            tap: "tap-1000_myapp".to_string()
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

    #[test]
    fn test_firewall_rules() {
        let rules = get_firewall_rules("172.16.0.0/24", "eth0");
        let rule_args = |chain: &str| -> Vec<(String, bool)> {
            rules.iter()
                .filter(|(rule, _)| rule.chain == chain)
                .map(|(rule, position)| (
                    rule.spec.join(" "),
                    matches!(position, RulePosition::Top)
                ))
                .collect()
        };
        // only the traffic of the VM network leaving through the
        // outgoing interface is masqueraded
        assert_eq!(
            vec![(
                "-s 172.16.0.0/24 -o eth0 -j MASQUERADE".to_string(), false
            )],
            rule_args("FLAKE_PILOT_NAT")
        );
        // accepting rules are created at the top, the drop rules
        // for everything else to and from a VM at the bottom
        assert_eq!(
            vec![
                (
                    "-i tap-+ -s 172.16.0.0/24 -o eth0 -j ACCEPT".to_string(),
                    true
                ),
                (
                    "-o tap-+ -m conntrack --ctstate RELATED,ESTABLISHED \
                    -j ACCEPT".to_string(),
                    true
                ),
                ("-i tap-+ -j DROP".to_string(), false),
                ("-o tap-+ -j DROP".to_string(), false)
            ],
            rule_args("FLAKE_PILOT_FORWARD")
        );
        assert_eq!(
            vec![
                (
                    "-m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT"
                        .to_string(),
                    true
                ),
                ("-j DROP".to_string(), false)
            ],
            rule_args("FLAKE_PILOT_INPUT")
        );
    }

    #[test]
    fn test_guest_service_rules() {
        let rules = get_guest_service_rules(
            "tap-1000_myapp", &address("172.16.0.2"), &network("172.16.0.0/24")
        );
        assert!(rules.iter().all(|rule| rule.chain == "FLAKE_PILOT_INPUT"));
        assert_eq!(
            vec![
                "-i tap-1000_myapp -p udp --sport 68 --dport 67 -j ACCEPT",
                "-i tap-1000_myapp -s 172.16.0.2 -d 172.16.0.1 -p udp \
                --dport 53 -j ACCEPT",
                "-i tap-1000_myapp -s 172.16.0.2 -d 172.16.0.1 -p tcp \
                --dport 53 -j ACCEPT",
                "-i tap-1000_myapp -s 172.16.0.2 -d 172.16.0.1 -p tcp \
                --dport 2049 -j ACCEPT"
            ],
            rules.iter().map(|rule| rule.spec.join(" ")).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_get_listed_rules() {
        let rule_list = "-N FLAKE_PILOT_INPUT\n\
            -A FLAKE_PILOT_INPUT -s 172.16.0.2/32 -d 172.16.0.1/32 \
            -i tap-1000_myapp -p tcp -m tcp --dport 2049 -j ACCEPT\n\
            -A FLAKE_PILOT_INPUT -j DROP\n\
            -A FORWARD -j DROP\n";
        assert_eq!(
            vec![
                vec![
                    "-s", "172.16.0.2/32", "-d", "172.16.0.1/32",
                    "-i", "tap-1000_myapp", "-p", "tcp", "-m", "tcp",
                    "--dport", "2049", "-j", "ACCEPT"
                ],
                vec!["-j", "DROP"]
            ],
            get_listed_rules(rule_list, "FLAKE_PILOT_INPUT")
        );
        // the policy of a builtin chain is not a rule
        assert!(get_listed_rules("-P FORWARD ACCEPT\n", "FORWARD").is_empty());
    }

    #[test]
    fn test_network_config_record() {
        // the record is written here and read by the pilot,
        // both of them have to agree on its format
        let record = serde_yaml::to_string(&NetworkConfig {
            outgoing_interface: "eth0".to_string(),
            network: Some("172.16.0.0/24".to_string())
        }).unwrap();
        assert!(record.contains("outgoing_interface: eth0"));
        assert!(record.contains("network: 172.16.0.0/24"));
    }

    #[test]
    fn test_remove_command() {
        let uid = get_current_uid();
        assert_eq!(
            "flake-ctl firecracker network remove --app /usr/bin/myapp",
            get_remove_command("/usr/bin/myapp", None, uid)
        );
        assert_eq!(
            "flake-ctl firecracker network remove --app /usr/bin/myapp \
            --instance @one",
            get_remove_command("/usr/bin/myapp", Some("@one"), uid)
        );
        // the setup of another user is addressed by its name
        if uid != 0 {
            assert_eq!(
                "flake-ctl firecracker network remove --app /usr/bin/myapp \
                --user root",
                get_remove_command("/usr/bin/myapp", None, 0)
            );
        }
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
    fn test_select_outgoing_interface() {
        // a given interface is taken without detecting one
        assert_eq!(
            Some("eth1".to_string()), select_outgoing_interface(Some("eth1"))
        );
    }

    #[test]
    fn test_get_effective_boot_args_without_instance() {
        let engine_section = engine_section(
            &["ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off"],
            &[("@one", &["ip=172.16.0.3::172.16.0.1:255.255.255.0::eth0:off"])]
        );
        // the setup of an instance is not in effect for the
        // application itself
        assert_eq!(
            boot_args(&["ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off"]),
            get_effective_boot_args(&engine_section, None)
        );
        // an instance without a section of its own uses the
        // global setup
        assert_eq!(
            boot_args(&["ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off"]),
            get_effective_boot_args(&engine_section, Some("@other"))
        );
    }

    #[test]
    fn test_get_effective_boot_args_of_instance() {
        let engine_section = engine_section(
            &[
                "ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off",
                "nfs=172.16.0.1:/host:/guest",
                "nameserver=8.8.8.8"
            ],
            &[(
                "@one",
                &[
                    "ip=172.16.0.3::172.16.0.1:255.255.255.0::eth0:off",
                    "quiet"
                ]
            )]
        );
        // the option of the instance takes the place of the
        // global one, options which are not set globally are
        // appended
        assert_eq!(
            boot_args(&[
                "ip=172.16.0.3::172.16.0.1:255.255.255.0::eth0:off",
                "nfs=172.16.0.1:/host:/guest",
                "nameserver=8.8.8.8",
                "quiet"
            ]),
            get_effective_boot_args(&engine_section, Some("@one"))
        );
    }

    #[test]
    fn test_get_effective_boot_args_of_instance_without_prefix() {
        let engine_section = engine_section(
            &["nfs=172.16.0.1:/host:/guest"],
            &[("one", &["nfs=172.16.0.1:/other:/mnt"])]
        );
        // a section keyed with the plain name is accepted as well
        assert_eq!(
            boot_args(&["nfs=172.16.0.1:/other:/mnt"]),
            get_effective_boot_args(&engine_section, Some("@one"))
        );
    }

    #[test]
    fn test_get_network_info() {
        let boot_args = boot_args(&[
            "ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off",
            "rd.route=172.16.0.1/24::eth0"
        ]);
        let network_info = get_network_info(
            &boot_args, 1000, "myapp@one"
        ).unwrap();
        assert_eq!(Some(address("172.16.0.2")), network_info.address);
        assert_eq!(get_tap_name(1000, "myapp@one"), network_info.tap);
        assert!(network_info.tap.starts_with("tap-1000_"));
    }

    #[test]
    fn test_get_network_info_without_setup() {
        // a flake which is not connected provides no network
        for setup in ["quiet", "ip=off", "ip=none"] {
            assert!(
                get_network_info(&boot_args(&[setup]), 1000, "myapp").is_none()
            );
        }
        // a dynamic setup provides no address
        let network_info = get_network_info(
            &boot_args(&["ip=dhcp"]), 1000, "myapp"
        ).unwrap();
        assert_eq!(None, network_info.address);
        assert_eq!("tap-1000_myapp", network_info.tap);
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
