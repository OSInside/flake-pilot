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
//! The host firewall setup of the VM network
//!
//! All netfilter rules of the setup live in chains of their own,
//! see flakes::network::get_jump_rules(). The rules are created
//! from the network and the interface recorded in the network
//! state and from the address allocations, never from a rule
//! specification of the caller.
//!
use std::io::Write;
use std::net::Ipv4Addr;
use std::process::{Command, Stdio};

use flakes::defaults::{
    self, TAP_DEVICE_PREFIX, FIREWALL_NAT_CHAIN, FIREWALL_FORWARD_CHAIN,
    FIREWALL_INPUT_CHAIN, FIREWALL_EGRESS_CHAIN
};
use flakes::network::{
    FirewallRule, HostServices, Ipv4Network, get_tap_match, get_jump_rules,
    get_masquerade_rule
};

use crate::run::run_ok;

// Position a rule is created at in its chain. Accepting rules are
// inserted at the top, dropping rules are appended. This keeps
// every accepting rule in front of the drop rules, no matter in
// which order the rules are created. The egress chain is not built
// this way, it is written as a whole, see write_egress_chain()
pub enum RulePosition {
    Top,
    Bottom
}

pub fn get_firewall_chains() -> [(&'static str, &'static str); 4] {
    /*!
    Provide the table and the name of the chains of the host
    firewall setup

    The egress chain is entered from the forward chain and is
    therefore listed after it, the forward chain has to be deleted
    first
    !*/
    [
        ("nat", FIREWALL_NAT_CHAIN),
        ("filter", FIREWALL_FORWARD_CHAIN),
        ("filter", FIREWALL_INPUT_CHAIN),
        ("filter", FIREWALL_EGRESS_CHAIN)
    ]
}

pub fn get_firewall_rules(
    network: &str, outgoing_interface: &str
) -> Vec<(FirewallRule, RulePosition)> {
    /*!
    Provide the rules of the host firewall setup and the position
    they are created at

    The traffic of the private network of the VMs is masqueraded
    and forwarded if it leaves through the outgoing interface and
    passes the egress policy, see get_egress_rules(). The answers
    to it may pass back. All other traffic which comes from or goes
    to a TAP device is dropped: traffic between two VMs, traffic
    from the outside world to a VM and traffic from a VM to another
    interface of the host. Traffic from a VM to the host itself is
    dropped as well, except for answers to traffic of the host and
    the services connect() allows for each instance

    The rules of the egress chain are not part of them, the chain
    is written before these rules are created, see setup_firewall()
    !*/
    let tap_match = get_tap_match();
    vec![
        (get_masquerade_rule(network, outgoing_interface), RulePosition::Bottom),
        (get_egress_jump_rule(network, outgoing_interface), RulePosition::Top),
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

pub fn get_egress_jump_rule(
    network: &str, outgoing_interface: &str
) -> FirewallRule {
    /*!
    Provide the rule which passes the traffic of the VMs to the
    outside world to the egress chain. It is the only way traffic
    of a TAP device gets forwarded
    !*/
    FirewallRule::new("filter", FIREWALL_FORWARD_CHAIN, &[
        "-i", &get_tap_match(), "-s", network, "-o", outgoing_interface,
        "-j", FIREWALL_EGRESS_CHAIN
    ])
}

pub fn get_egress_rules(
    egress_deny: &[Ipv4Network], egress_allow: &[Ipv4Network]
) -> Vec<FirewallRule> {
    /*!
    Provide the rules of the egress chain, in the order they have
    to be in

    The traffic of the VMs to the denied networks is dropped first.
    Without allowed networks all other traffic is accepted by the
    last rule of the chain. With allowed networks, only the traffic
    to them is accepted and the last rule drops everything else. A
    network which is denied therefore stays denied, even if it is
    part of an allowed one. The allow list can only narrow down what
    the VMs reach, never extend it.

    The rules are given in the notation iptables lists them with,
    see egress_chain_matches()
    !*/
    let rule = |network: &Ipv4Network, target: &str| FirewallRule::new(
        "filter", FIREWALL_EGRESS_CHAIN,
        &["-d", &network.to_string(), "-j", target]
    );
    let mut rules: Vec<FirewallRule> = egress_deny.iter()
        .map(|network| rule(network, "DROP"))
        .collect();
    rules.extend(egress_allow.iter().map(|network| rule(network, "ACCEPT")));
    rules.push(FirewallRule::new("filter", FIREWALL_EGRESS_CHAIN, &[
        "-j", if egress_allow.is_empty() { "ACCEPT" } else { "DROP" }
    ]));
    rules
}

pub fn get_egress_restore_input(rules: &[FirewallRule]) -> String {
    /*!
    Provide the input of iptables-restore which replaces the rules
    of the egress chain by the given ones

    With --noflush only the chains which are declared in the input
    are flushed, the egress chain is the only one. The rules come
    from validated networks only, see get_egress_rules()
    !*/
    let mut input = format!("*filter\n:{FIREWALL_EGRESS_CHAIN} - [0:0]\n");
    for rule in rules {
        input.push_str(
            &format!("-A {} {}\n", rule.chain, rule.spec.join(" "))
        );
    }
    input.push_str("COMMIT\n");
    input
}

pub fn write_egress_chain(
    egress_deny: &[Ipv4Network], egress_allow: &[Ipv4Network]
) -> bool {
    /*!
    Replace the rules of the egress chain by the ones of the given
    egress policy, see get_egress_rules()

    The rules decide by their order which traffic passes. Adding and
    deleting them one by one would let traffic pass, or drop it, by
    a mix of the former and the new policy while the chain is
    changed. iptables-restore replaces the chain in one transaction
    instead, the traffic passes either the former or the new policy
    !*/
    let input = get_egress_restore_input(
        &get_egress_rules(egress_deny, egress_allow)
    );
    info!(
        "Writing {FIREWALL_EGRESS_CHAIN} chain: {} denied, {} allowed \
        network(s)...", egress_deny.len(), egress_allow.len()
    );
    let mut call = Command::new(defaults::IPTABLES_RESTORE_TOOL);
    call.arg("--wait").arg("--noflush")
        .stdin(Stdio::piped()).stdout(Stdio::null());
    let mut child = match call.spawn() {
        Ok(child) => child,
        Err(error) => {
            error!("Failed to write {FIREWALL_EGRESS_CHAIN} chain: {error:?}");
            return false
        }
    };
    let written = match child.stdin.take() {
        Some(mut stdin) => stdin.write_all(input.as_bytes()),
        None => Err(std::io::Error::other("no stdin"))
    };
    // stdin is closed at this point, iptables-restore reads to its end
    match (written, child.wait()) {
        (Ok(_), Ok(status)) if status.success() => true,
        (Err(error), _) | (_, Err(error)) => {
            error!("Failed to write {FIREWALL_EGRESS_CHAIN} chain: {error:?}");
            false
        },
        (_, Ok(status)) => {
            error!("Failed to write {FIREWALL_EGRESS_CHAIN} chain: {status}");
            false
        }
    }
}

pub fn egress_chain_matches(
    listed: &[Vec<String>], egress_deny: &[Ipv4Network],
    egress_allow: &[Ipv4Network]
) -> bool {
    /*!
    Check if the listed rules of the egress chain are the ones of
    the given egress policy

    The drop rules of the denied networks come first, followed by
    the accept rules of the allowed networks and the last rule of
    the chain, see get_egress_rules(). Within the drop rules and
    within the accept rules the order does not matter. Former
    versions inserted the drop rules one by one at the top, which
    listed them in reverse order
    !*/
    let wanted: Vec<Vec<String>> = get_egress_rules(egress_deny, egress_allow)
        .into_iter().map(|rule| rule.spec).collect();
    if listed.len() != wanted.len() {
        return false
    }
    let same_rules = |listed: &[Vec<String>], wanted: &[Vec<String>]| {
        wanted.iter().all(|spec| listed.contains(spec))
    };
    let allow_start = egress_deny.len();
    let allow_end = allow_start + egress_allow.len();
    same_rules(&listed[..allow_start], &wanted[..allow_start])
        && same_rules(
            &listed[allow_start..allow_end], &wanted[allow_start..allow_end]
        )
        && listed[allow_end..] == wanted[allow_end..]
}

pub fn get_chain_rules(table: &str, chain: &str) -> Option<Vec<Vec<String>>> {
    /*!
    Provide the rules of the given chain in the notation iptables
    lists them with, see get_listed_rules()
    !*/
    let mut call = iptables(table);
    call.arg("-S").arg(chain).stderr(Stdio::null());
    match call.output() {
        Ok(output) if output.status.success() => Some(get_listed_rules(
            &String::from_utf8_lossy(&output.stdout), chain
        )),
        _ => None
    }
}

pub fn get_guest_service_rules(
    tap: &str, address: &Ipv4Addr, network: &Ipv4Network,
    services: HostServices
) -> Vec<FirewallRule> {
    /*!
    Provide the rules which let the VM behind the given TAP device
    use the services of the host

    The VM may reach the NFS service of the host through its
    gateway. The DNS and the DHCP service only if the host runs
    them for the VMs, otherwise the VMs do not use them and the
    services of the host stay closed to them. Apart from DHCP,
    which is used before the VM has an address, the traffic has to
    come from the address of the VM and from its own TAP device. A
    VM can therefore not use the services, e.g the NFS exports, of
    another VM
    !*/
    let address = address.to_string();
    let gateway = network.gateway().to_string();
    let mut rules = Vec::new();
    if services.dhcp {
        rules.push(
            FirewallRule::new("filter", FIREWALL_INPUT_CHAIN, &[
                "-i", tap, "-p", "udp",
                "--sport", defaults::NETWORK_DHCP_CLIENT_PORT,
                "--dport", defaults::NETWORK_DHCP_SERVER_PORT,
                "-j", "ACCEPT"
            ])
        )
    }
    let mut ports = Vec::new();
    if services.resolver {
        ports.push(("udp", defaults::NETWORK_DNS_PORT));
        ports.push(("tcp", defaults::NETWORK_DNS_PORT));
    }
    ports.push(("tcp", defaults::NETWORK_NFS_PORT));
    for (protocol, port) in ports {
        rules.push(
            FirewallRule::new("filter", FIREWALL_INPUT_CHAIN, &[
                "-i", tap, "-s", &address, "-d", &gateway,
                "-p", protocol, "--dport", port, "-j", "ACCEPT"
            ])
        )
    }
    rules
}

pub fn setup_firewall(
    network: &Ipv4Network, outgoing_interface: &str,
    egress_deny: &[Ipv4Network], egress_allow: &[Ipv4Network]
) -> bool {
    /*!
    Create the chains and rules of the host firewall setup

    The chains are created first and are entered from the builtin
    chains last, traffic is therefore only passed to them once they
    are complete. The egress chain is written as a whole before
    traffic is passed to it, a former egress policy is replaced in
    one step, see write_egress_chain(). All other chains and rules
    which already exist are kept, this allows to call the setup
    more than once
    !*/
    for (table, chain) in get_firewall_chains() {
        if ! create_chain(table, chain) {
            return false
        }
    }
    if ! write_egress_chain(egress_deny, egress_allow) {
        return false
    }
    for (rule, position) in get_firewall_rules(
        &network.to_string(), outgoing_interface
    ) {
        if ! add_rule(&rule, position) {
            return false
        }
    }
    if ! delete_unfiltered_forward_rules() {
        return false
    }
    for rule in get_jump_rules() {
        if ! add_rule(&rule, RulePosition::Top) {
            return false
        }
    }
    true
}

pub fn delete_firewall() -> bool {
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

pub fn firewall_is_active(
    network: &Ipv4Network, outgoing_interface: &str,
    egress_deny: &[Ipv4Network], egress_allow: &[Ipv4Network]
) -> bool {
    /*!
    Check if the host firewall setup is complete, the chains are
    present and are entered from the builtin chains

    With an egress policy, the traffic of the VMs has to be passed
    to the egress chain and the chain has to hold the rules of the
    policy, see egress_chain_matches(). The chain is read in one
    call, a policy may consist of many networks. A setup created
    before the egress policy existed has no egress chain, it is
    complete without a policy
    !*/
    let chains_exist = get_firewall_chains().iter()
        .filter(|(_, chain)| *chain != FIREWALL_EGRESS_CHAIN)
        .all(|(table, chain)| chain_exists(table, chain));
    if ! chains_exist || ! get_jump_rules().iter().all(rule_exists) {
        return false
    }
    if egress_deny.is_empty() && egress_allow.is_empty() {
        return true
    }
    rule_exists(
        &get_egress_jump_rule(&network.to_string(), outgoing_interface)
    ) && get_chain_rules("filter", FIREWALL_EGRESS_CHAIN).is_some_and(
        |listed| egress_chain_matches(&listed, egress_deny, egress_allow)
    )
}

pub fn delete_unfiltered_forward_rules() -> bool {
    /*!
    Delete the rules which forward the traffic of the VMs without
    passing it to the egress chain

    Former versions accepted the traffic of the TAP devices in the
    forward chain directly. Such a rule is recognized by its match
    of the TAP devices as the incoming interface and its target
    !*/
    let tap_match = get_tap_match();
    delete_listed_rules("filter", FIREWALL_FORWARD_CHAIN, |spec| {
        spec.ends_with(&["-j".to_string(), "ACCEPT".to_string()])
            && spec.windows(2).any(
                |pair| pair[0] == "-i" && pair[1] == tap_match
            )
    })
}

pub fn allow_guest_services(
    tap: &str, address: &Ipv4Addr, network: &Ipv4Network,
    services: HostServices
) -> bool {
    /*!
    Let the VM behind the given TAP device use the services of the
    host, see get_guest_service_rules()
    !*/
    get_guest_service_rules(tap, address, network, services).iter().all(
        |rule| add_rule(rule, RulePosition::Top)
    )
}

pub fn revoke_guest_services(tap: &str) -> bool {
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

pub fn delete_legacy_rules(outgoing_interface: &str) {
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

pub fn iptables(table: &str) -> Command {
    /*!
    Create an iptables call for the given table
    !*/
    let mut call = Command::new(defaults::IPTABLES_TOOL);
    call.arg("-t").arg(table);
    call
}

pub fn iptables_ok(call: &mut Command) -> bool {
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

pub fn chain_exists(table: &str, chain: &str) -> bool {
    /*!
    Check if the given chain exists
    !*/
    let mut call = iptables(table);
    call.arg("-S").arg(chain);
    iptables_ok(&mut call)
}

pub fn create_chain(table: &str, chain: &str) -> bool {
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

pub fn delete_chain(table: &str, chain: &str) -> bool {
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

pub fn rule_exists(rule: &FirewallRule) -> bool {
    /*!
    Check if the given rule is active
    !*/
    let mut call = iptables(rule.table);
    call.arg("-C").arg(&rule.chain).args(&rule.spec);
    iptables_ok(&mut call)
}

pub fn add_rule(rule: &FirewallRule, position: RulePosition) -> bool {
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

pub fn delete_rule(rule: &FirewallRule) -> bool {
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

pub fn delete_listed_rules(
    table: &str, chain: &str, matches: impl Fn(&[String]) -> bool
) -> bool {
    /*!
    Delete the rules of the given chain which match

    The rules are read in the notation iptables lists them with,
    each rule is matched by its specification without the leading
    '-A CHAIN'
    !*/
    let rules = match get_chain_rules(table, chain) {
        Some(rules) => rules,
        None => {
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

pub fn get_listed_rules(rule_list: &str, chain: &str) -> Vec<Vec<String>> {
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

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use flakes::network::{HostServices, Ipv4Network};

    use super::{
        egress_chain_matches, get_egress_restore_input, get_egress_rules,
        get_firewall_rules, get_guest_service_rules, get_listed_rules,
        RulePosition
    };

    fn network(network: &str) -> Ipv4Network {
        Ipv4Network::parse(network).unwrap()
    }

    fn address(address: &str) -> Ipv4Addr {
        address.parse().unwrap()
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
        // the egress chain is written as a whole, apart from these
        // rules
        assert!(rule_args("FLAKE_PILOT_EGRESS").is_empty());
        // accepting rules are created at the top, the drop rules
        // for everything else to and from a VM at the bottom. The
        // traffic to the outside world is only accepted after it
        // passed the egress chain
        assert_eq!(
            vec![
                (
                    "-i tap-+ -s 172.16.0.0/24 -o eth0 -j FLAKE_PILOT_EGRESS"
                        .to_string(),
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
    fn test_egress_rules() {
        let specs = |deny: &[Ipv4Network], allow: &[Ipv4Network]| {
            get_egress_rules(deny, allow).iter()
                .inspect(|rule| assert_eq!("FLAKE_PILOT_EGRESS", rule.chain))
                .map(|rule| rule.spec.join(" "))
                .collect::<Vec<_>>()
        };
        // without an egress policy everything passes the egress chain
        assert_eq!(vec!["-j ACCEPT"], specs(&[], &[]));
        // the traffic to the denied networks is dropped in front of
        // the rule which accepts everything else
        assert_eq!(
            vec![
                "-d 169.254.0.0/16 -j DROP",
                "-d 10.0.0.0/8 -j DROP",
                "-j ACCEPT"
            ],
            specs(&[network("169.254.0.0/16"), network("10.0.0.0/8")], &[])
        );
        // with an allow list only the traffic to the allowed
        // networks passes, the denied networks stay denied
        assert_eq!(
            vec![
                "-d 47.0.0.0/8 -j DROP",
                "-d 8.8.8.8/32 -j ACCEPT",
                "-d 0.0.0.0/0 -j ACCEPT",
                "-j DROP"
            ],
            specs(
                &[network("47.0.0.0/8")],
                &[network("8.8.8.8"), network("0.0.0.0/0")]
            )
        );
    }

    #[test]
    fn test_egress_restore_input() {
        // only the egress chain is declared and therefore replaced,
        // its rules are written in their order
        assert_eq!(
            "*filter\n\
            :FLAKE_PILOT_EGRESS - [0:0]\n\
            -A FLAKE_PILOT_EGRESS -d 169.254.0.0/16 -j DROP\n\
            -A FLAKE_PILOT_EGRESS -d 203.0.113.0/24 -j ACCEPT\n\
            -A FLAKE_PILOT_EGRESS -j DROP\n\
            COMMIT\n",
            get_egress_restore_input(&get_egress_rules(
                &[network("169.254.0.0/16")], &[network("203.0.113.0/24")]
            ))
        );
    }

    #[test]
    fn test_egress_chain_matches() {
        let listed = |rule_list: &str| get_listed_rules(
            &format!("-N FLAKE_PILOT_EGRESS\n{rule_list}"),
            "FLAKE_PILOT_EGRESS"
        );
        let deny = [network("169.254.0.0/16"), network("100.100.100.200")];
        let allow = [network("203.0.113.0/24")];
        // the rules are listed in the notation they are created with
        let denied = listed(
            "-A FLAKE_PILOT_EGRESS -d 169.254.0.0/16 -j DROP\n\
            -A FLAKE_PILOT_EGRESS -d 100.100.100.200/32 -j DROP\n\
            -A FLAKE_PILOT_EGRESS -j ACCEPT\n"
        );
        assert!(egress_chain_matches(&denied, &deny, &[]));
        // former versions listed the drop rules in reverse order
        let reversed = listed(
            "-A FLAKE_PILOT_EGRESS -d 100.100.100.200/32 -j DROP\n\
            -A FLAKE_PILOT_EGRESS -d 169.254.0.0/16 -j DROP\n\
            -A FLAKE_PILOT_EGRESS -j ACCEPT\n"
        );
        assert!(egress_chain_matches(&reversed, &deny, &[]));
        // a missing, stale or additional rule does not match
        assert!(! egress_chain_matches(&denied, &deny[..1], &[]));
        assert!(! egress_chain_matches(&denied, &deny, &allow));
        assert!(! egress_chain_matches(
            &listed("-A FLAKE_PILOT_EGRESS -j ACCEPT\n"), &deny, &[]
        ));
        let allowed = listed(
            "-A FLAKE_PILOT_EGRESS -d 169.254.0.0/16 -j DROP\n\
            -A FLAKE_PILOT_EGRESS -d 100.100.100.200/32 -j DROP\n\
            -A FLAKE_PILOT_EGRESS -d 203.0.113.0/24 -j ACCEPT\n\
            -A FLAKE_PILOT_EGRESS -j DROP\n"
        );
        assert!(egress_chain_matches(&allowed, &deny, &allow));
        assert!(! egress_chain_matches(&allowed, &deny, &[]));
        // an allow rule in front of a drop rule would let the
        // traffic to the denied network pass
        let allowed_first = listed(
            "-A FLAKE_PILOT_EGRESS -d 203.0.113.0/24 -j ACCEPT\n\
            -A FLAKE_PILOT_EGRESS -d 169.254.0.0/16 -j DROP\n\
            -A FLAKE_PILOT_EGRESS -d 100.100.100.200/32 -j DROP\n\
            -A FLAKE_PILOT_EGRESS -j DROP\n"
        );
        assert!(! egress_chain_matches(&allowed_first, &deny, &allow));
    }

    #[test]
    fn test_guest_service_rules() {
        let rules = |services| get_guest_service_rules(
            "tap-1000_myapp", &address("172.16.0.2"),
            &network("172.16.0.0/24"), services
        );
        let specs = |services| rules(services).iter()
            .map(|rule| rule.spec.join(" ")).collect::<Vec<_>>();
        let dhcp = "-i tap-1000_myapp -p udp --sport 68 --dport 67 -j ACCEPT";
        let dns_udp = "-i tap-1000_myapp -s 172.16.0.2 -d 172.16.0.1 -p udp \
            --dport 53 -j ACCEPT";
        let dns_tcp = "-i tap-1000_myapp -s 172.16.0.2 -d 172.16.0.1 -p tcp \
            --dport 53 -j ACCEPT";
        let nfs = "-i tap-1000_myapp -s 172.16.0.2 -d 172.16.0.1 -p tcp \
            --dport 2049 -j ACCEPT";

        // without services on the host only NFS is open
        assert_eq!(vec![nfs], specs(HostServices::default()));

        // each service of the host is opened on its own
        assert_eq!(
            vec![dns_udp, dns_tcp, nfs],
            specs(HostServices { resolver: true, dhcp: false })
        );
        assert_eq!(
            vec![dhcp, nfs],
            specs(HostServices { resolver: false, dhcp: true })
        );
        let all = HostServices { resolver: true, dhcp: true };
        assert_eq!(vec![dhcp, dns_udp, dns_tcp, nfs], specs(all));
        assert!(
            rules(all).iter().all(|rule| rule.chain == "FLAKE_PILOT_INPUT")
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
}
