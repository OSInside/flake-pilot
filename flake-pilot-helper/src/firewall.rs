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
// which order the rules are created. The egress chain is the other
// way round: the drop rules of the egress policy are inserted at
// the top, in front of the rule which accepts everything else
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
    network: &str, outgoing_interface: &str, egress_deny: &[Ipv4Network]
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

    The rules of the egress chain are listed first, the chain is
    complete before traffic is passed to it
    !*/
    let tap_match = get_tap_match();
    let mut rules: Vec<(FirewallRule, RulePosition)> = get_egress_rules(
        egress_deny
    ).into_iter().map(|rule| (rule, RulePosition::Top)).collect();
    rules.extend([
        (
            FirewallRule::new("filter", FIREWALL_EGRESS_CHAIN, &[
                "-j", "ACCEPT"
            ]),
            RulePosition::Bottom
        ),
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
    ]);
    rules
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

pub fn get_egress_rules(egress_deny: &[Ipv4Network]) -> Vec<FirewallRule> {
    /*!
    Provide the rules of the egress policy

    The traffic of the VMs to the given networks is dropped, all
    other traffic which enters the egress chain is accepted by its
    last rule. The rules are given in the notation iptables lists
    them with, see delete_stale_egress_rules()
    !*/
    egress_deny.iter()
        .map(|network| FirewallRule::new("filter", FIREWALL_EGRESS_CHAIN, &[
            "-d", &network.to_string(), "-j", "DROP"
        ]))
        .collect()
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
    egress_deny: &[Ipv4Network]
) -> bool {
    /*!
    Create the chains and rules of the host firewall setup

    The chains are created first and are entered from the builtin
    chains last, traffic is therefore only passed to them once they
    are complete. Chains and rules which already exist are kept,
    this allows to call the setup more than once. The drop rules of
    a former egress policy are deleted once the rules of the given
    one are in place
    !*/
    for (table, chain) in get_firewall_chains() {
        if ! create_chain(table, chain) {
            return false
        }
    }
    for (rule, position) in get_firewall_rules(
        &network.to_string(), outgoing_interface, egress_deny
    ) {
        if ! add_rule(&rule, position) {
            return false
        }
    }
    if ! delete_stale_egress_rules(egress_deny)
        || ! delete_unfiltered_forward_rules()
    {
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
    egress_deny: &[Ipv4Network]
) -> bool {
    /*!
    Check if the host firewall setup is complete, the chains are
    present and are entered from the builtin chains

    With an egress policy, the traffic of the VMs has to be passed
    to the egress chain and the drop rules of the policy have to be
    present. A setup created before the egress policy existed has
    no egress chain, it is complete without a policy
    !*/
    let chains_exist = get_firewall_chains().iter()
        .filter(|(_, chain)| *chain != FIREWALL_EGRESS_CHAIN)
        .all(|(table, chain)| chain_exists(table, chain));
    if ! chains_exist || ! get_jump_rules().iter().all(rule_exists) {
        return false
    }
    egress_deny.is_empty() || (
        rule_exists(
            &get_egress_jump_rule(&network.to_string(), outgoing_interface)
        ) && get_egress_rules(egress_deny).iter().all(rule_exists)
    )
}

pub fn delete_stale_egress_rules(egress_deny: &[Ipv4Network]) -> bool {
    /*!
    Delete the drop rules of the egress chain which are not part of
    the given egress policy
    !*/
    let wanted: Vec<Vec<String>> = get_egress_rules(egress_deny).into_iter()
        .map(|rule| rule.spec)
        .collect();
    delete_listed_rules("filter", FIREWALL_EGRESS_CHAIN, |spec| {
        spec.last().is_some_and(|target| target == "DROP")
            && ! wanted.iter().any(|wanted_spec| wanted_spec == spec)
    })
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
        get_egress_rules, get_firewall_rules, get_guest_service_rules,
        get_listed_rules, RulePosition
    };

    fn network(network: &str) -> Ipv4Network {
        Ipv4Network::parse(network).unwrap()
    }

    fn address(address: &str) -> Ipv4Addr {
        address.parse().unwrap()
    }

    #[test]
    fn test_firewall_rules() {
        let rules = get_firewall_rules(
            "172.16.0.0/24", "eth0",
            &[network("169.254.0.0/16"), network("10.0.0.0/8")]
        );
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
        // the egress chain drops the traffic to the denied networks
        // in front of the rule which accepts everything else
        assert_eq!(
            vec![
                ("-d 169.254.0.0/16 -j DROP".to_string(), true),
                ("-d 10.0.0.0/8 -j DROP".to_string(), true),
                ("-j ACCEPT".to_string(), false)
            ],
            rule_args("FLAKE_PILOT_EGRESS")
        );
        // the egress chain is complete before traffic is passed to it
        let last_egress_rule = rules.iter()
            .rposition(|(rule, _)| rule.chain == "FLAKE_PILOT_EGRESS");
        let egress_jump_rule = rules.iter()
            .position(|(rule, _)| rule.spec.ends_with(&[
                "-j".to_string(), "FLAKE_PILOT_EGRESS".to_string()
            ]));
        assert!(last_egress_rule.unwrap() < egress_jump_rule.unwrap());
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
        // without an egress policy everything passes the egress chain
        let rules = get_firewall_rules("172.16.0.0/24", "eth0", &[]);
        assert_eq!(
            vec!["-j ACCEPT"],
            rules.iter()
                .filter(|(rule, _)| rule.chain == "FLAKE_PILOT_EGRESS")
                .map(|(rule, _)| rule.spec.join(" "))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_egress_rules_are_listed_as_created() {
        // stale drop rules are recognized by comparing the listed
        // rules of the egress chain with the created ones
        let rule_list = "-N FLAKE_PILOT_EGRESS\n\
            -A FLAKE_PILOT_EGRESS -d 169.254.0.0/16 -j DROP\n\
            -A FLAKE_PILOT_EGRESS -d 100.100.100.200/32 -j DROP\n\
            -A FLAKE_PILOT_EGRESS -j ACCEPT\n";
        assert_eq!(
            get_listed_rules(rule_list, "FLAKE_PILOT_EGRESS")[..2].to_vec(),
            get_egress_rules(&[
                network("169.254.0.0/16"), network("100.100.100.200")
            ]).into_iter().map(|rule| rule.spec).collect::<Vec<_>>()
        );
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
