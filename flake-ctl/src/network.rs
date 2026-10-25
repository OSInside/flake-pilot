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
use flakes::helper::helper_call;
use flakes::network::{
    get_default_route_interface, get_egress_networks, get_free_network,
    get_nameserver, get_network_config_file, get_preferred_network,
    get_tap_name,
    lock_network_state, network_collides, read_network_config,
    read_network_state, tap_exists, HostServices, Ipv4Network, NetworkConfig,
    NetworkState
};
use flakes::registration;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::net::Ipv4Addr;
use std::path::Path;
use std::process::{Command, Stdio};
use uzers::{get_current_uid, get_user_by_name, get_user_by_uid};

use crate::app_config::{
    AppConfig, AppFireCrackerEngine, AppFireCrackerInstance
};
use crate::defaults;

pub fn init(
    outgoing_interface: Option<&str>, services: HostServices,
    egress_deny: &[String], usermode: bool
) -> bool {
    /*!
    Prepare the host for NAT based VM networking

    Firecracker connects a VM to the host through a TUN/TAP device
    which by itself has no connection to the outside world. Routing
    the traffic of that device requires IP forwarding on the host
    and a NAT rule which lets the traffic appear as if it would
    originate from the given outgoing interface. Without a given
    outgoing interface the interface of the IPv4 default route of
    the host is used.

    The private network of the VMs is selected here, the setup
    itself is created by the helper, see its network-init command.
    The helper checks the network and the interface again and
    records them along with the given services of the host and the
    networks of the given egress policy in the network state. The
    setup of the single flakes, done by add(), uses what is
    recorded there.

    The setup is also recorded in the network configuration file,
    which the pilot reads to check the firewall setup.

    The setup is not persistent and has to be created again after
    a reboot of the host. Please note, the setup assumes there is
    no other firewall software active on the host. If the host
    firewall is managed by another tool, the rules have to be
    created with that tool instead
    !*/
    let egress_networks = match get_egress_networks(egress_deny) {
        Ok(egress_networks) => egress_networks,
        Err(error) => {
            error!("{error}");
            return false
        }
    };
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
    let mut call = helper_call("network-init");
    call.arg("--network").arg(network.to_string())
        .arg("--outgoing-interface").arg(&outgoing_interface);
    if services.resolver {
        call.arg("--host-resolver");
    }
    if services.dhcp {
        call.arg("--host-dhcp");
    }
    for egress_network in &egress_networks {
        call.arg("--egress-deny").arg(egress_network.to_string());
    }
    if ! run_helper(&mut call, "prepare the host network") {
        return false
    }
    if ! write_network_config(
        &outgoing_interface, &network, services, usermode
    ) {
        return false
    }
    info!("Host is prepared for VM networking:");
    info!("  network: {network}");
    info!("  gateway: {}", network.gateway());
    info!("  outgoing interface: {outgoing_interface}");
    info!("  name server: {}", get_nameserver(&network, services));
    info!(
        "  DHCP server of the host: {}",
        if services.dhcp { "yes" } else { "no" }
    );
    if egress_networks.is_empty() {
        info!("  egress denied: none");
    } else {
        info!(
            "  egress denied: {}",
            egress_networks.iter()
                .map(ToString::to_string).collect::<Vec<_>>().join(", ")
        );
    }
    true
}

pub fn add(
    app: &str, instance: Option<&String>, user: Option<&String>,
    usermode: bool
) -> bool {
    /*!
    Connect the given flake application to the host network

    The helper allocates an address of the recorded network for the
    instance, creates the TAP device for the user who runs the VM,
    given by user, the calling user if not given, routes the address
    to it and opens the services of the host the VM may use, see its
    network-connect command. The address is written to the flake
    configuration here, along with the gateway and the name server.

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
    let state = match read_locked_network_state() {
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
    let mut yaml_config = match read_flake_config(&flake.config_file) {
        Some(yaml_config) => yaml_config,
        None => return false
    };
    let configured = match get_engine_section(
        &mut yaml_config, &flake.config_file, "network"
    ) {
        Some(engine_section) => get_configured_address(
            engine_section, flake.instance.as_deref()
        ),
        None => return false
    };

    // 1. the address allocation, the TAP device, the route and the
    //    firewall rules of the instance
    let mut call = helper_call("network-connect");
    call.arg("--config-file").arg(&flake.config_file);
    if let Some(instance) = &flake.instance {
        call.arg("--instance").arg(instance);
    }
    if user.is_some() {
        call.arg("--uid").arg(uid.to_string());
    }
    if let Some(configured) = configured {
        call.arg("--prefer").arg(configured.to_string());
    }
    for address in get_configured_addresses(usermode) {
        call.arg("--exclude").arg(address.to_string());
    }
    let address = match run_helper_output(&mut call, "connect the instance")
        .and_then(|output| output.trim().parse::<Ipv4Addr>().ok())
    {
        Some(address) => address,
        None => return false
    };

    // 2. the flake configuration
    if ! configure_flake(&flake, &address, &network, services) {
        return false
    }

    info!("Application {app} is connected:");
    info!("  address: {address}");
    info!("  gateway: {}", network.gateway());
    info!("  tap device: {} (user ID {uid})", flake.tap);
    if let Some(outgoing_interface) = &state.outgoing_interface {
        info!("  outgoing interface: {outgoing_interface}");
    }
    info!("  name server: {}", get_nameserver(&network, services));
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

    All of the setup created by add() is deleted. The helper deletes
    the firewall rules of the instance, its TAP device and its
    address allocation, see its network-disconnect command. The
    network setup is deleted from the flake configuration here. The
    address becomes available again for another application.

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

    // 1. the firewall rules, the TAP device and the address allocation
    let mut call = helper_call("network-disconnect");
    call.arg("--config-file").arg(&flake.config_file);
    if let Some(instance) = &flake.instance {
        call.arg("--instance").arg(instance);
    }
    if user.is_some() {
        call.arg("--uid").arg(uid.to_string());
    }
    if ! run_helper(&mut call, "disconnect the instance") {
        return false
    }

    // 2. the flake configuration
    if ! unconfigure_flake(&flake.config_file, flake.instance.as_deref()) {
        return false
    }

    info!("Application {app} is disconnected");
    true
}

pub fn cleanup() -> bool {
    /*!
    Delete the network setup of the host

    This is the counterpart of init(), performed by the helper, see
    its network-cleanup command: the firewall setup is deleted as a
    whole, all TAP devices of the flakes are deleted and IP
    forwarding is switched off again if flake-pilot switched it on.
    The address allocations and the network setup in the flake
    configurations are kept. The host is left in the state it has
    after a reboot, calling init() and add() again connects the
    applications with the addresses they had before
    !*/
    let mut call = helper_call("network-cleanup");
    if ! run_helper(&mut call, "delete the host network setup") {
        return false
    }
    info!("Network setup of the host is deleted");
    true
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
    let state = match read_locked_network_state() {
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
    flake: &FlakeNetwork, address: &Ipv4Addr, network: &Ipv4Network,
    services: HostServices
) -> bool {
    /*!
    Write the static network setup of the given flake instance to
    its flake configuration

    The address is the one the helper allocated to the instance.
    The configuration belongs to the caller and is written with the
    privileges of the caller
    !*/
    let config_file = flake.config_file.as_str();
    let instance = flake.instance.as_deref();
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
    let dns_option = format!(
        "nameserver={}", get_nameserver(network, services)
    );
    // The initrd has to wait for the network to come up. It is
    // requested along with the address, which is the setting
    // that tells the initrd to bring up the network
    let neednet_option = "rd.neednet=1".to_string();

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
            let instance_boot_args = instance_section.boot_args
                .get_or_insert_with(Vec::new);
            set_boot_arg(instance_boot_args, ip_option);
            set_boot_arg(instance_boot_args, neednet_option);
        },
        None => {
            set_boot_arg(boot_args, ip_option);
            set_boot_arg(boot_args, neednet_option);
        }
    }

    if ! write_flake_config(config_file, &yaml_config) {
        return false
    }
    info!("Updated {config_file}");
    true
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

fn unconfigure_flake(config_file: &str, instance: Option<&str>) -> bool {
    /*!
    Delete the static network setup from the flake configuration

    Only the address is specific to an instance. The route to the
    gateway and the name server are shared and are therefore only
    deleted if no instance of the application is connected anymore.
    Without an address the initrd must not wait for the network,
    which is the same state a fresh registration creates
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
    if get_configured_address(engine_section, None).is_none() {
        set_boot_arg(
            engine_section.boot_args.get_or_insert_with(Vec::new),
            "rd.neednet=0".to_string()
        );
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
    Delete the address of the given instance along with the
    request to wait for the network

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
            unset_boot_arg(boot_args, "rd.neednet");
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

fn get_configured_addresses(usermode: bool) -> Vec<Ipv4Addr> {
    /*!
    Provide the addresses configured in the flake registrations
    which can be read

    The helper does not hand them out. They belong to flakes which
    were connected before the addresses got allocated in the host
    wide state. In user mode this includes the system wide
    registrations because their instances share the network with
    the ones of the calling user
    !*/
    let mut used: Vec<Ipv4Addr> = Vec::new();
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
    used.sort();
    used.dedup();
    used
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
    Read the private network of the recorded host setup

    The network is recorded in the network state by the helper. A
    setup created by a former version is only recorded in the
    network configuration file
    !*/
    if let Some(network) = read_locked_network_state()
        .and_then(|state| state.get_network())
    {
        return RecordedNetwork::Network(network)
    }
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

fn write_network_config(
    outgoing_interface: &str, network: &Ipv4Network, services: HostServices,
    usermode: bool
) -> bool {
    /*!
    Record the host network setup

    The record allows the setup of the single flakes to find the
    interface their traffic has to be routed to, the private
    network their addresses are taken from and the services of the
    host they use
    !*/
    let config_file = get_network_config_file(usermode);
    let network_config = NetworkConfig {
        outgoing_interface: outgoing_interface.to_string(),
        network: Some(network.to_string()),
        host_resolver: services.resolver,
        host_dhcp: services.dhcp
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

fn read_locked_network_state() -> Option<NetworkState> {
    /*!
    Read the host wide network state under a shared lock, it must
    not be read while the helper writes it
    !*/
    let _lock = lock_network_state(false)?;
    read_network_state()
}

fn run_helper(call: &mut Command, action: &str) -> bool {
    /*!
    Run the given helper call, its messages are passed through
    !*/
    match call.status() {
        Ok(status) if status.success() => true,
        Ok(status) => {
            error!("Failed to {action}: {status}");
            false
        },
        Err(error) => {
            error!("Failed to {action}: {error:?}");
            false
        }
    }
}

fn run_helper_output(call: &mut Command, action: &str) -> Option<String> {
    /*!
    Run the given helper call and provide its result, the standard
    output. Its messages are passed through
    !*/
    call.stderr(Stdio::inherit());
    match call.output() {
        Ok(output) if output.status.success() => {
            Some(String::from_utf8_lossy(&output.stdout).to_string())
        },
        Ok(output) => {
            error!("Failed to {action}: {}", output.status);
            None
        },
        Err(error) => {
            error!("Failed to {action}: {error:?}");
            None
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

    use flakes::network::{
        Allocation, HostServices, Ipv4Network, NetworkState
    };
    use std::fs;
    use tempfile::tempdir;

    use super::{
        configure_flake, get_effective_boot_args, get_flake_taps,
        get_network_info, get_remove_command, read_flake_config,
        select_outgoing_interface, unconfigure_flake, FlakeNetwork,
        NetworkConfig
    };

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


    fn read_boot_args(
        config_file: &str
    ) -> (Option<Vec<String>>, Option<Vec<String>>) {
        let yaml_config = read_flake_config(config_file).unwrap();
        let engine_section = yaml_config.vm.unwrap().runtime.unwrap()
            .firecracker.unwrap();
        let instance_boot_args = engine_section.instance
            .and_then(|mut instances| instances.remove("@one"))
            .and_then(|instance_section| instance_section.boot_args);
        (engine_section.boot_args, instance_boot_args)
    }

    #[test]
    fn test_configure_and_unconfigure_flake() {
        let config_dir = tempdir().unwrap();
        let config_file = config_dir.path().join("myapp.yaml")
            .display().to_string();
        fs::write(
            &config_file,
            "vm:\n  name: leap\n  target_app_path: /usr/bin/myapp\n  \
            host_app_path: /usr/bin/myapp\n  runtime:\n    firecracker:\n      \
            boot_args:\n        - root=/dev/vda\n        - rd.neednet=0\n"
        ).unwrap();
        let network = Ipv4Network::new(address("172.16.0.0"), 24);
        let services = HostServices { resolver: false, dhcp: false };
        let registered = boot_args(&["root=/dev/vda", "rd.neednet=0"]);

        // the instance waits for the network, the application
        // called without an instance selector does not
        let flake = FlakeNetwork {
            config_file: config_file.clone(),
            instance: Some("@one".to_string()),
            tap: "tap-1000_myapp_one".to_string()
        };
        assert!(configure_flake(
            &flake, &address("172.16.0.3"), &network, services
        ));
        let (global, instance) = read_boot_args(&config_file);
        assert_eq!(
            Some(boot_args(&[
                "root=/dev/vda", "rd.neednet=0",
                "rd.route=172.16.0.1/24::eth0", "nameserver=8.8.8.8"
            ])),
            global
        );
        assert_eq!(
            Some(boot_args(&[
                "ip=172.16.0.3::172.16.0.1:255.255.255.0::eth0:off",
                "rd.neednet=1"
            ])),
            instance
        );
        assert!(unconfigure_flake(&config_file, Some("@one")));
        assert_eq!(
            (Some(registered.clone()), None), read_boot_args(&config_file)
        );

        // the application called without an instance selector
        let flake = FlakeNetwork {
            config_file: config_file.clone(),
            instance: None,
            tap: "tap-1000_myapp".to_string()
        };
        assert!(configure_flake(
            &flake, &address("172.16.0.2"), &network, services
        ));
        let (global, _) = read_boot_args(&config_file);
        assert_eq!(
            Some(boot_args(&[
                "root=/dev/vda", "rd.neednet=1",
                "rd.route=172.16.0.1/24::eth0", "nameserver=8.8.8.8",
                "ip=172.16.0.2::172.16.0.1:255.255.255.0::eth0:off"
            ])),
            global
        );
        assert!(unconfigure_flake(&config_file, None));
        assert_eq!((Some(registered), None), read_boot_args(&config_file));
    }

    #[test]
    fn test_get_flake_taps() {
        let state = NetworkState {
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
            ],
            ..Default::default()
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
    fn test_network_config_record() {
        // the record is written here and read by the pilot,
        // both of them have to agree on its format
        let record = serde_yaml::to_string(&NetworkConfig {
            outgoing_interface: "eth0".to_string(),
            network: Some("172.16.0.0/24".to_string()),
            host_resolver: true,
            host_dhcp: true
        }).unwrap();
        assert!(record.contains("outgoing_interface: eth0"));
        assert!(record.contains("network: 172.16.0.0/24"));
        assert!(record.contains("host_resolver: true"));
        assert!(record.contains("host_dhcp: true"));
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

}
