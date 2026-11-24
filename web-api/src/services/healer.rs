// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Network healer: detects problems in live flows and builds fixes.
//
// Problem IDs are derived from the problem kind and namespace (for example
// `heal-dns-team-a`), so an ID means the same thing on every request. Only
// problems with a concrete, safe remediation are auto-fixable; today that is
// DNS egress being blocked. Policy denials need a human to decide what to allow.

use crate::models::flow::Flow;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const FIXES_PREFIX: &str = "cv:healer_fixes:";
/// Name of the CiliumNetworkPolicy created by the DNS fix. Fixed per namespace,
/// so applying twice updates the same object instead of creating duplicates.
pub const DNS_POLICY_NAME: &str = "paqtra-allow-dns";
const UNKNOWN_NS: &str = "unknown";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemKind {
    DnsBlocked,
    PolicyDenial,
}

#[derive(Debug, Clone)]
pub struct Problem {
    pub id: String,
    pub kind: ProblemKind,
    pub namespace: String,
    pub dropped_flows: usize,
    pub pods: Vec<String>,
}

impl Problem {
    pub fn kind_str(&self) -> &'static str {
        match self.kind {
            ProblemKind::DnsBlocked => "dns_blocked",
            ProblemKind::PolicyDenial => "policy_denial",
        }
    }

    /// Whether Paqtra can remediate this automatically. Requires a known
    /// namespace: a fix must never be applied to a guessed one.
    pub fn auto_fixable(&self) -> bool {
        self.kind == ProblemKind::DnsBlocked && self.namespace != UNKNOWN_NS
    }

    pub fn to_json(&self, fix_applied: bool) -> Value {
        let (severity, description, proposed_fix) = match self.kind {
            ProblemKind::DnsBlocked => (
                "high",
                format!(
                    "DNS traffic (port 53) blocked for {} flows in namespace '{}'",
                    self.dropped_flows, self.namespace
                ),
                format!(
                    "Add CiliumNetworkPolicy '{}' allowing UDP/TCP port 53 egress to kube-dns in namespace '{}'",
                    DNS_POLICY_NAME, self.namespace
                ),
            ),
            ProblemKind::PolicyDenial => (
                "medium",
                format!(
                    "{} dropped flows in namespace '{}'",
                    self.dropped_flows, self.namespace
                ),
                format!(
                    "Review CiliumNetworkPolicies in namespace '{}' for missing allow rules",
                    self.namespace
                ),
            ),
        };
        json!({
            "id": self.id,
            "type": self.kind_str(),
            "severity": severity,
            "description": description,
            "affected_pods": self.pods,
            "namespace": self.namespace,
            "status": if fix_applied { "fix_applied" } else { "open" },
            "proposed_fix": proposed_fix,
            "auto_fixable": self.auto_fixable(),
        })
    }
}

/// Group dropped flows into problems, one per (kind, source namespace),
/// ordered by ID so listings are stable.
pub fn detect(flows: &[Flow]) -> Vec<Problem> {
    // (is_dns, namespace) -> (flow count, distinct pods)
    let mut groups: BTreeMap<(bool, String), (usize, BTreeSet<String>)> = BTreeMap::new();
    for flow in flows.iter().filter(|f| f.verdict == "DROPPED") {
        let ns = if flow.source.namespace.is_empty() {
            UNKNOWN_NS.to_string()
        } else {
            flow.source.namespace.clone()
        };
        let entry = groups.entry((flow.port == 53, ns)).or_default();
        entry.0 += 1;
        if !flow.source.pod.is_empty() {
            entry.1.insert(flow.source.pod.clone());
        }
    }

    let mut problems: Vec<Problem> = groups
        .into_iter()
        .map(|((is_dns, namespace), (dropped_flows, pods))| Problem {
            id: format!("heal-{}-{}", if is_dns { "dns" } else { "pol" }, namespace),
            kind: if is_dns {
                ProblemKind::DnsBlocked
            } else {
                ProblemKind::PolicyDenial
            },
            namespace,
            dropped_flows,
            pods: pods.into_iter().take(5).collect(),
        })
        .collect();
    problems.sort_by(|a, b| a.id.cmp(&b.id));
    problems
}

/// Policy spec that lets every pod in the namespace reach kube-dns on port 53.
/// Purely additive: pods that were already dropping DNS are already subject to
/// egress default-deny, so this only opens the one path they need.
pub fn dns_allow_spec() -> Value {
    json!({
        "endpointSelector": {},
        "egress": [{
            "toEndpoints": [{
                "matchLabels": {
                    "k8s:io.kubernetes.pod.namespace": "kube-system",
                    "k8s-app": "kube-dns"
                }
            }],
            "toPorts": [{
                "ports": [{ "port": "53", "protocol": "ANY" }]
            }]
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::flow::FlowEndpoint;

    fn flow(verdict: &str, ns: &str, pod: &str, port: u16) -> Flow {
        Flow {
            id: "f".into(),
            timestamp: String::new(),
            source: FlowEndpoint {
                namespace: ns.into(),
                pod: pod.into(),
                ip: String::new(),
            },
            destination: FlowEndpoint {
                namespace: String::new(),
                pod: String::new(),
                ip: String::new(),
            },
            verdict: verdict.into(),
            protocol: "UDP".into(),
            port,
            http_method: None,
            http_url: None,
            http_code: None,
            cluster: None,
            ..Default::default()
        }
    }

    #[test]
    fn ids_are_stable_and_input_order_independent() {
        let a = vec![
            flow("DROPPED", "team-b", "p1", 53),
            flow("DROPPED", "team-a", "p2", 53),
            flow("DROPPED", "team-a", "p3", 8080),
        ];
        let mut b = a.clone();
        b.reverse();
        let ids = |v: &[Flow]| detect(v).into_iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(&a), ids(&b));
        assert_eq!(
            ids(&a),
            vec!["heal-dns-team-a", "heal-dns-team-b", "heal-pol-team-a"]
        );
    }

    #[test]
    fn forwarded_flows_are_ignored_and_pods_deduplicated() {
        let flows = vec![
            flow("FORWARDED", "team-a", "p1", 53),
            flow("DROPPED", "team-a", "p1", 53),
            flow("DROPPED", "team-a", "p1", 53),
        ];
        let problems = detect(&flows);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].dropped_flows, 2);
        assert_eq!(problems[0].pods, vec!["p1"]);
    }

    #[test]
    fn only_dns_with_known_namespace_is_auto_fixable() {
        let flows = vec![
            flow("DROPPED", "team-a", "p", 53),
            flow("DROPPED", "team-a", "p", 443),
            flow("DROPPED", "", "p", 53),
        ];
        let by_id: BTreeMap<_, _> = detect(&flows)
            .into_iter()
            .map(|p| (p.id.clone(), p.auto_fixable()))
            .collect();
        assert!(by_id["heal-dns-team-a"]);
        assert!(!by_id["heal-pol-team-a"]);
        assert!(!by_id["heal-dns-unknown"]);
    }

    #[test]
    fn json_shape_and_applied_status() {
        let p = &detect(&[flow("DROPPED", "team-a", "p", 53)])[0];
        let open = p.to_json(false);
        assert_eq!(open["status"], "open");
        assert_eq!(open["type"], "dns_blocked");
        assert_eq!(open["auto_fixable"], true);
        assert_eq!(p.to_json(true)["status"], "fix_applied");
    }

    #[test]
    fn dns_spec_targets_kube_dns_port_53_only() {
        let spec = dns_allow_spec();
        let egress = &spec["egress"][0];
        assert_eq!(
            egress["toEndpoints"][0]["matchLabels"]["k8s-app"],
            "kube-dns"
        );
        assert_eq!(egress["toPorts"][0]["ports"][0]["port"], "53");
        assert_eq!(spec["egress"].as_array().unwrap().len(), 1);
    }
}
