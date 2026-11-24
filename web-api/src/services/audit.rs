// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Policy audit readiness: what would be denied if enforcement were on.
//!
//! With Cilium's policy audit mode (`policyAuditMode`, or a policy in audit
//! mode on an endpoint) traffic that a policy would drop is allowed and
//! reported with the `AUDIT` verdict. Rolling out default deny safely means
//! reading those flows first. This lists the edges and ports behind them,
//! worst first, so each can be turned into an allow rule or a bug report.
//! It reads stored flows only; it changes no policy and no Cilium setting.

use super::flow_agg::{endpoint_label, truncate, Granularity, PairCount};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const DEFAULT_MAX_ITEMS: usize = 50;
pub const MAX_MAX_ITEMS: usize = 1000;
const PORTS_LISTED: usize = 8;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AuditEdge {
    pub source: String,
    pub destination: String,
    pub protocol: String,
    /// Would-be-denied flows on this edge.
    pub flows: u64,
    /// Busiest ports first.
    pub ports: Vec<u16>,
    pub port_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Affected {
    pub namespace: String,
    pub flows: u64,
    pub edges: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AuditReport {
    pub granularity: &'static str,
    pub audit_flows: u64,
    pub edges_total: usize,
    pub edges: Vec<AuditEdge>,
    pub omitted_edges: usize,
    /// Destination namespaces that would see denials, most affected first.
    pub destination_namespaces: Vec<Affected>,
    pub truncated: bool,
    pub guidance: &'static str,
}

/// Build from pair counts that were read with the `AUDIT` verdict filter (every
/// counted flow in them is an audit flow; rows with none are skipped).
pub fn report(
    pairs: &[PairCount],
    g: Granularity,
    max_items: usize,
    truncated: bool,
) -> AuditReport {
    let max_items = max_items.clamp(1, MAX_MAX_ITEMS);
    #[derive(Default)]
    struct Acc {
        flows: u64,
        ports: HashMap<u16, u64>,
    }
    let mut edges: BTreeMap<(String, String, String), Acc> = BTreeMap::new();
    let mut ns: BTreeMap<String, (u64, BTreeSet<(String, String)>)> = BTreeMap::new();
    let mut total = 0u64;
    for p in pairs {
        let n = p.total();
        if n == 0 {
            continue;
        }
        total = total.saturating_add(n);
        let src = endpoint_label(&p.src_namespace, &p.src_pod, g);
        let dst = endpoint_label(&p.dst_namespace, &p.dst_pod, g);
        let proto: String = truncate(&p.protocol).chars().take(16).collect();
        let a = edges.entry((src.clone(), dst.clone(), proto)).or_default();
        a.flows = a.flows.saturating_add(n);
        let c = a.ports.entry(p.port).or_insert(0);
        *c = c.saturating_add(n);
        let dns = if p.dst_namespace.is_empty() {
            super::flow_agg::EXTERNAL.to_string()
        } else {
            truncate(&p.dst_namespace)
        };
        let e = ns.entry(dns).or_default();
        e.0 = e.0.saturating_add(n);
        e.1.insert((src, dst));
    }

    let edges_total = edges.len();
    let mut list: Vec<AuditEdge> = edges
        .into_iter()
        .map(|((source, destination, protocol), a)| {
            let mut ports: Vec<(u16, u64)> = a.ports.into_iter().collect();
            ports.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
            let port_count = ports.len();
            AuditEdge {
                source,
                destination,
                protocol,
                flows: a.flows,
                ports: ports
                    .into_iter()
                    .take(PORTS_LISTED)
                    .map(|(p, _)| p)
                    .collect(),
                port_count,
            }
        })
        .collect();
    list.sort_by(|a, b| {
        b.flows
            .cmp(&a.flows)
            .then_with(|| (&a.source, &a.destination).cmp(&(&b.source, &b.destination)))
    });
    let omitted_edges = list.len().saturating_sub(max_items);
    list.truncate(max_items);

    let mut affected: Vec<Affected> = ns
        .into_iter()
        .map(|(namespace, (flows, e))| Affected {
            namespace,
            flows,
            edges: e.len(),
        })
        .collect();
    affected.sort_by(|a, b| {
        b.flows
            .cmp(&a.flows)
            .then_with(|| a.namespace.cmp(&b.namespace))
    });
    affected.truncate(max_items);

    AuditReport {
        granularity: g.as_str(),
        audit_flows: total,
        edges_total,
        edges: list,
        omitted_edges,
        destination_namespaces: affected,
        truncated,
        guidance:
            "AUDIT flows were allowed only because policy audit mode is on; with enforcement \
                   they would be dropped. No AUDIT flows means audit mode is off, or nothing \
                   would be denied: check Cilium's policyAuditMode before reading it as safe. \
                   Review each edge, then allow it with a CiliumNetworkPolicy (the policy \
                   builder and simulator can draft and preview one) or fix the caller. Nothing is \
                   changed here.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audit(src: &str, dst_ns: &str, dst: &str, port: u16, n: u64) -> PairCount {
        PairCount {
            src_namespace: "app".into(),
            src_pod: src.into(),
            dst_namespace: dst_ns.into(),
            dst_pod: dst.into(),
            protocol: "TCP".into(),
            port,
            drop_reason: String::new(),
            forwarded: 0,
            dropped: 0,
            other: n,
        }
    }

    #[test]
    fn merges_ports_per_edge_and_ranks_edges() {
        let p = vec![
            audit("web-0", "db", "pg-0", 5432, 40),
            audit("web-0", "db", "pg-0", 5433, 5),
            audit("job", "db", "pg-0", 5432, 100),
            audit("web-0", "cache", "redis-0", 6379, 7),
        ];
        let r = report(&p, Granularity::Workload, 10, false);
        assert_eq!(r.audit_flows, 152);
        assert_eq!(r.edges_total, 3);
        assert_eq!(r.edges[0].source, "app/job");
        assert_eq!(r.edges[1].destination, "db/pg");
        assert_eq!(r.edges[1].ports, vec![5432, 5433]);
        assert_eq!(r.edges[1].flows, 45);
        assert_eq!(r.destination_namespaces[0].namespace, "db");
        assert_eq!(r.destination_namespaces[0].flows, 145);
        assert_eq!(r.destination_namespaces[0].edges, 2);
    }

    #[test]
    fn bounds_edges_and_ports_and_handles_empty() {
        let p: Vec<_> = (0..30)
            .map(|i| audit(&format!("c{i:02}"), "db", "pg", 1000 + i as u16, 1))
            .collect();
        let r = report(&p, Granularity::Pod, 5, true);
        assert_eq!((r.edges.len(), r.omitted_edges, r.edges_total), (5, 25, 30));
        assert!(r.truncated);
        let wide: Vec<_> = (0..20)
            .map(|i| audit("c", "db", "pg", 2000 + i, 1))
            .collect();
        let r = report(&wide, Granularity::Pod, 5, false);
        assert_eq!(r.edges[0].ports.len(), PORTS_LISTED);
        assert_eq!(r.edges[0].port_count, 20);
        let e = report(&[], Granularity::Pod, 5, false);
        assert_eq!((e.audit_flows, e.edges_total), (0, 0));
        assert!(e.guidance.contains("audit mode"));
    }

    #[test]
    fn zero_count_rows_and_external_destinations() {
        let mut z = audit("a", "x", "y", 1, 0);
        z.forwarded = 0;
        let ext = audit("a", "", "", 443, 3);
        let r = report(&[z, ext], Granularity::Pod, 5, false);
        assert_eq!(r.audit_flows, 3);
        assert_eq!(r.edges[0].destination, "external");
        assert_eq!(r.destination_namespaces[0].namespace, "external");
    }
}
