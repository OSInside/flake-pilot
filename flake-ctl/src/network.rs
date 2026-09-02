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
use flakes::config::get_flakes_dir;
use flakes::network::get_tap_name;
use glob::glob;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::fs;
use std::net::Ipv4Addr;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::app_config::{
    AppConfig, AppFireCrackerEngine, AppFireCrackerInstance
};
use crate::defaults;
use crate::firecracker::run_as;
use crate::setup::user_home;

// NetworkConfig is the record of the host network setup
#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub outgoing_interface: String,
}

pub fn init(outgoing_interface: &str, usermode: bool) -> bool {
    /*!
    Prepare the host for NAT based VM networking

    Firecracker connects a VM to the host through a TUN/TAP device
    which by itself has no connection to the outside world. Routing
    the traffic of that device requires IP forwarding on the host
    and a NAT rule which lets the traffic appear as if it would
    originate from the given outgoing interface.

    The setup changes the network configuration of the host and is
    therefore performed via sudo. It is not persistent and has to
    be created again after a reboot of the host.

    The outgoing interface is stored such that the setup of the
    single flakes, done by add(), knows where to route their
    traffic to.

    Please note, the setup assumes there is no other firewall
    software active on the host. If the host firewall is managed
    by another tool, the rules have to be created with that tool
    instead
    !*/
    if ! enable_ip_forward() {
        return false
    }
    if ! setup_nat(outgoing_interface) {
        return false
    }
    write_network_config(outgoing_interface, usermode)
}

pub fn add(app: &str, instance: Option<&String>, usermode: bool) -> bool {
    /*!
    Connect the given flake application to the host network

    The flake gets a free address of the private network between
    the host and the VMs written to its configuration and the TAP
    device it expects is created and routed to the outgoing
    interface of the host setup created by init(). A host route
    connects that address with that device.

    Each instance of an application needs its own address and its
    own TAP device. Therefore the command has to be called for
    every @NAME instance selector the application is called with
    !*/
    let flake = match get_flake_network(app, instance, usermode) {
        Some(flake) => flake,
        None => return false
    };
    let outgoing_interface = match get_outgoing_interface(usermode) {
        Some(outgoing_interface) => outgoing_interface,
        None => return false
    };

    // 1. the flake configuration
    let address = match configure_flake(
        &flake.config_file, flake.instance.as_deref(), usermode
    ) {
        Some(address) => address,
        None => return false
    };

    // 2. the TAP device of the instance
    if ! create_tap(&flake.tap) {
        return false
    }

    // 3. the route from the TAP device to the outside world
    if ! connect_tap(&flake.tap, &outgoing_interface) {
        return false
    }

    // 4. the host route to the address of the instance
    if ! route_address(&address, &flake.tap) {
        return false
    }

    info!("Application {app} is connected:");
    info!("  address: {address}");
    info!("  gateway: {}", defaults::NETWORK_GATEWAY);
    info!("  tap device: {}", flake.tap);
    info!("  outgoing interface: {outgoing_interface}");
    if let Some(instance) = flake.instance {
        info!("Call the application as: {app} {instance}");
    }
    true
}

pub fn remove(app: &str, instance: Option<&String>, usermode: bool) -> bool {
    /*!
    Disconnect the given flake application from the host network

    All of the setup created by add() is deleted. This is the
    route of the TAP device to the outside world, the TAP device
    itself and the network setup in the flake configuration. The
    address of the flake becomes available again for another
    application.

    Called with an instance selector only the setup of that
    instance is deleted. The setup which is shared with the other
    instances of the application is kept as long as one of them
    is still connected
    !*/
    let flake = match get_flake_network(app, instance, usermode) {
        Some(flake) => flake,
        None => return false
    };

    // 1. the route from the TAP device to the outside world
    match find_outgoing_interface(usermode) {
        Some(outgoing_interface) => {
            if ! disconnect_tap(&flake.tap, &outgoing_interface) {
                return false
            }
        },
        None => {
            // Without the interface the rule was created for there
            // is nothing to match it against. Deleting the device
            // stops the traffic anyway, only the rule stays behind
            warn!("Failed to detect the outgoing interface");
            warn!("Keeping the FORWARD rule of {}", flake.tap);
        }
    }

    // 2. the TAP device of the instance
    if ! delete_tap(&flake.tap) {
        return false
    }

    // 3. the flake configuration
    if ! unconfigure_flake(&flake.config_file, flake.instance.as_deref()) {
        return false
    }

    info!("Application {app} is disconnected");
    true
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
    app: &str, instance: Option<&String>, usermode: bool
) -> Option<FlakeNetwork> {
    /*!
    Provide the network identity of the given flake application

    The flake configuration to work on is looked up from the
    application path and the TAP device is the one the pilot
    connects the instance of the application to
    !*/
    if ! app.starts_with('/') {
        error!("Application {app:?} must be specified with an absolute path");
        return None
    }
    let app_basename = match Path::new(app).file_name() {
        Some(app_basename) => app_basename.to_string_lossy().to_string(),
        None => {
            error!("Failed to read the application name from {app}");
            return None
        }
    };
    let config_file = format!(
        "{}/{}.yaml", get_flakes_dir(usermode), app_basename
    );
    if ! Path::new(&config_file).exists() {
        error!("No flake configuration found at {config_file}");
        error!("Please register the application first");
        return None
    }
    let instance = instance.map(|instance| get_instance_name(instance));

    // The pilot connects the VM to the TAP device named after the
    // application and the instance selector it was called with
    let mut meta_name = app_basename;
    if let Some(ref instance) = instance {
        meta_name.push_str(instance);
    }
    let tap = get_tap_name(&meta_name);

    Some(FlakeNetwork { config_file, instance, tap })
}

fn get_instance_name(instance: &str) -> String {
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
    config_file: &str, instance: Option<&str>, usermode: bool
) -> Option<Ipv4Addr> {
    /*!
    Write the static network setup to the flake configuration

    The address is taken from the private network between the host
    and the VMs. An address which is already used by another flake
    registration is not handed out twice. A flake which is already
    configured keeps its address, this allows to call the setup
    again, e.g after a reboot of the host
    !*/
    let mut yaml_config = read_flake_config(config_file)?;
    let engine_section = get_engine_section(&mut yaml_config, config_file)?;

    // An address which is already configured for this flake is
    // kept, in all other cases a free one is taken
    let address = match get_configured_address(engine_section, instance) {
        Some(address) => {
            info!("Keeping configured address {address}");
            address
        },
        None => {
            let address = get_free_address(&get_used_addresses(usermode))?;
            info!("Assigning address {address}");
            address
        }
    };

    let ip_option = format!(
        "ip={}::{}:{}::{}:off",
        address,
        defaults::NETWORK_GATEWAY,
        defaults::NETWORK_NETMASK,
        defaults::NETWORK_GUEST_INTERFACE
    );
    let route_option = format!(
        "rd.route={}/{}::{}",
        defaults::NETWORK_GATEWAY,
        defaults::NETWORK_PREFIX_LEN,
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

fn get_instance_key(
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
    deleted if no instance of the application is connected anymore
    !*/
    let mut yaml_config = match read_flake_config(config_file) {
        Some(yaml_config) => yaml_config,
        None => return false
    };
    let engine_section = match get_engine_section(
        &mut yaml_config, config_file
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

fn get_engine_section<'a>(
    yaml_config: &'a mut AppConfig, config_file: &str
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
        error!("Only firecracker flakes provide a network setup");
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

fn get_used_addresses(usermode: bool) -> Vec<Ipv4Addr> {
    /*!
    Provide the addresses which are in use by flake registrations

    All registrations that can be read are taken into account.
    In user mode this includes the system wide registrations
    because their instances share the network with the ones of
    the calling user
    !*/
    let mut used: Vec<Ipv4Addr> = Vec::new();
    let mut flakes_dirs = vec![get_flakes_dir(usermode)];
    if usermode {
        let system_flakes_dir = get_flakes_dir(false);
        if Path::new(&system_flakes_dir).is_dir() {
            flakes_dirs.push(system_flakes_dir);
        }
    }
    for flakes_dir in flakes_dirs {
        let glob_pattern = format!("{flakes_dir}/*.yaml");
        for config_file in glob(&glob_pattern).unwrap().flatten() {
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
                    if let Some(boot_args) = instance_section.boot_args.as_ref()
                    {
                        used.extend(get_boot_args_address(boot_args));
                    }
                }
            }
        }
    }
    used
}

fn get_free_address(used: &[Ipv4Addr]) -> Option<Ipv4Addr> {
    /*!
    Provide the lowest address of the private network which is
    not in use. The network and broadcast address as well as the
    address of the gateway are never handed out
    !*/
    let gateway = get_gateway();
    let network = gateway.octets();
    for host in 1..=254 {
        let address = Ipv4Addr::new(network[0], network[1], network[2], host);
        if address != gateway && ! used.contains(&address) {
            return Some(address)
        }
    }
    error!("No free address left in {}/{}",
        gateway, defaults::NETWORK_PREFIX_LEN
    );
    None
}

fn get_gateway() -> Ipv4Addr {
    /*!
    Provide the address of the host side of the private network
    !*/
    defaults::NETWORK_GATEWAY.parse().unwrap_or_else(
        |_| panic!("Invalid gateway default: {}", defaults::NETWORK_GATEWAY)
    )
}

fn set_boot_arg(boot_args: &mut Vec<String>, boot_arg: String) {
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

fn unset_boot_arg(boot_args: &mut Vec<String>, name: &str) {
    /*!
    Delete the kernel commandline option of the given name
    !*/
    boot_args.retain(|boot_arg| boot_arg_name(boot_arg) != name);
}

fn boot_arg_name(boot_arg: &str) -> &str {
    /*!
    Provide the name of a kernel commandline option
    !*/
    boot_arg.split('=').next().unwrap_or(boot_arg)
}

fn read_flake_config(config_file: &str) -> Option<AppConfig> {
    /*!
    Read the given flake configuration

    Only the configuration file itself is read. The optional
    drop-in files from the '.d' directory next to it are not
    merged in because the result is written back to this file
    !*/
    let yaml_data = match fs::read_to_string(config_file) {
        Ok(yaml_data) => yaml_data,
        Err(error) => {
            error!("Failed to read {config_file}: {error:?}");
            return None
        }
    };
    match serde_yaml::from_str(&yaml_data) {
        Ok(yaml_config) => Some(yaml_config),
        Err(error) => {
            error!("Failed to parse {config_file}: {error:?}");
            None
        }
    }
}

fn write_flake_config(config_file: &str, yaml_config: &AppConfig) -> bool {
    /*!
    Write back the given flake configuration
    !*/
    let config = match fs::File::create(config_file) {
        Ok(config) => config,
        Err(error) => {
            error!("Failed to open {config_file}: {error:?}");
            return false
        }
    };
    match serde_yaml::to_writer(config, yaml_config) {
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
    let routes = String::from_utf8_lossy(&output.stdout);
    let mut fields = routes.split_whitespace();
    while let Some(field) = fields.next() {
        if field == "dev" {
            return fields.next().map(ToOwned::to_owned)
        }
    }
    None
}

fn get_network_config_file(usermode: bool) -> Option<String> {
    /*!
    Provide the path of the host network setup record

    The system wide setup is recorded next to the other flake
    configuration files, the setup a user created is recorded
    in the home directory of that user
    !*/
    if ! usermode {
        return Some(defaults::NETWORK_CONFIG.to_string())
    }
    match user_home() {
        Some(home) => Some(format!("{}/{}", home, defaults::NETWORK_CONFIG_USER)),
        None => {
            error!("Failed to lookup the home directory of the caller");
            None
        }
    }
}

fn read_network_config(usermode: bool) -> Option<NetworkConfig> {
    /*!
    Read the record of the host network setup
    !*/
    let config_file = get_network_config_file(usermode)?;
    let yaml_data = fs::read_to_string(&config_file).ok()?;
    match serde_yaml::from_str(&yaml_data) {
        Ok(network_config) => Some(network_config),
        Err(error) => {
            error!("Failed to parse {config_file}: {error:?}");
            None
        }
    }
}

fn write_network_config(outgoing_interface: &str, usermode: bool) -> bool {
    /*!
    Record the host network setup

    The record allows the setup of the single flakes to find the
    interface their traffic has to be routed to
    !*/
    let config_file = match get_network_config_file(usermode) {
        Some(config_file) => config_file,
        None => return false
    };
    let network_config = NetworkConfig {
        outgoing_interface: outgoing_interface.to_string()
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

fn enable_ip_forward() -> bool {
    /*!
    Turn on IPv4 forwarding on the host
    !*/
    let proc_file = defaults::PROC_IP_FORWARD;
    info!("Enabling IP forwarding...");
    let mut call = run_as("sh", "root");
    call.arg("-c").arg(format!("echo 1 > {proc_file}"));
    run_ok(&mut call, &format!("write 1 to {proc_file}"))
}

struct NatRule<'a> {
    /// Name of the iptables table, None for the default table
    table: Option<&'a str>,
    /// Name of the chain the rule belongs to
    chain: &'a str,
    /// Match and target of the rule
    spec: Vec<&'a str>
}

fn setup_nat(outgoing_interface: &str) -> bool {
    /*!
    Create the netfilter rules to masquerade the VM traffic
    !*/
    let rules = [
        // Rewrite the sender of all outgoing traffic to the
        // address of the outgoing interface
        NatRule {
            table: Some("nat"),
            chain: "POSTROUTING",
            spec: vec!["-o", outgoing_interface, "-j", "MASQUERADE"]
        },
        // Let the answers to that traffic pass back to the VM
        NatRule {
            table: None,
            chain: "FORWARD",
            spec: vec![
                "-m", "conntrack",
                "--ctstate", "RELATED,ESTABLISHED",
                "-j", "ACCEPT"
            ]
        }
    ];
    add_rules(&rules)
}

fn add_rules(rules: &[NatRule]) -> bool {
    /*!
    Create the given netfilter rules

    Rules which are already present are not created again. This
    allows to call the setup more than once without stacking up
    duplicates of the same rule
    !*/
    for rule in rules {
        if rule_exists(rule) {
            info!("Keeping existing {} rule", rule.chain);
            continue
        }
        info!("Setting up {} rule...", rule.chain);
        if ! run_ok(
            &mut iptables(rule, "-A"),
            &format!("add {} rule", rule.chain)
        ) {
            return false
        }
    }
    true
}

fn delete_rules(rules: &[NatRule]) -> bool {
    /*!
    Delete the given netfilter rules

    A rule which is not active is not deleted. This allows to
    call the cleanup more than once and also covers the case
    that the rules were already flushed, e.g by a reboot
    !*/
    for rule in rules {
        if ! rule_exists(rule) {
            info!("No {} rule to delete", rule.chain);
            continue
        }
        info!("Deleting {} rule...", rule.chain);
        if ! run_ok(
            &mut iptables(rule, "-D"),
            &format!("delete {} rule", rule.chain)
        ) {
            return false
        }
    }
    true
}

fn rule_exists(rule: &NatRule) -> bool {
    /*!
    Check if the given rule is already active

    The check itself reports a missing rule on stderr which is
    not an error in this context and therefore not shown
    !*/
    let mut call = iptables(rule, "-C");
    call.stdout(Stdio::null()).stderr(Stdio::null());
    match call.status() {
        Ok(status) => status.success(),
        Err(_) => false
    }
}

fn iptables(rule: &NatRule, command: &str) -> Command {
    /*!
    Create an iptables call applying the given command,
    e.g '-A' or '-C', to the given rule
    !*/
    let mut call = run_as(defaults::IPTABLES_TOOL, "root");
    if let Some(table) = rule.table {
        call.arg("-t").arg(table);
    }
    call.arg(command).arg(rule.chain).args(&rule.spec);
    call
}

fn create_tap(tap: &str) -> bool {
    /*!
    Create the TAP device of a VM instance

    The device is the host local endpoint of the VM. Its name is
    the one the pilot passes to firecracker when it starts the
    instance of the application
    !*/
    if tap_exists(tap) {
        info!("Keeping existing TAP device {tap}");
        return true
    }
    info!("Creating TAP device {tap}...");
    let mut call = run_as(defaults::IP_TOOL, "root");
    call.arg("tuntap").arg("add").arg(tap).arg("mode").arg("tap");
    run_ok(&mut call, &format!("create TAP device {tap}"))
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

fn connect_tap(tap: &str, outgoing_interface: &str) -> bool {
    /*!
    Connect the given TAP device to the outgoing interface

    The device becomes the gateway of the VM behind it and its
    traffic is allowed to pass to the outgoing interface. The
    way back is covered by the rules created with init()
    !*/
    let gateway = format!(
        "{}/{}", defaults::NETWORK_GATEWAY, defaults::NETWORK_PREFIX_LEN
    );
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
    if ! run_ok(&mut call, &format!("bring up {tap}")) {
        return false
    }
    add_rules(&[
        NatRule {
            table: None,
            chain: "FORWARD",
            spec: vec!["-i", tap, "-o", outgoing_interface, "-j", "ACCEPT"]
        }
    ])
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

fn disconnect_tap(tap: &str, outgoing_interface: &str) -> bool {
    /*!
    Delete the route from the given TAP device to the outside
    world

    Only the rule of this device is deleted. The NAT setup of
    the host is shared by all VM applications and stays in
    place, it can be flushed by other means, e.g a reboot
    !*/
    delete_rules(&[
        NatRule {
            table: None,
            chain: "FORWARD",
            spec: vec!["-i", tap, "-o", outgoing_interface, "-j", "ACCEPT"]
        }
    ])
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
