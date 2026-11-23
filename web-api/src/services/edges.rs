// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Service edges with golden signals, built from stored flows.
//!
//! A service map answers "who talks to whom". This one adds what Hubble flows
//! can honestly say about each edge: traffic (flows per second), errors (the
//! dropped share, with Cilium's drop reasons) and the ports in use. Latency and
//! saturation are not in L3/L4 flow records, so they are reported as absent
//! rather than guessed. Output size is bounded however hostile the input: the
//! edge list is capped, the long tail is folded into one `other` row, and
//! endpoints without a pod are never split by IP.

use super::flow_agg::{endpoint_label, round, Granularity, PairCount};
use serde::Serialize;
use std::collections::HashMap;

pub const DEFAULT_MAX_EDGES: usize = 200;
pub const MAX_MAX_EDGES: usize = 1000;
const TOP_REASONS: usize = 3;
const TOP_PORTS: usize = 5;
/// Protocol labels are short; anything longer is not a protocol name.
const MAX_PROTOCOL_CHARS: usize = 16;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Edge {
    pub source: String,
    pub destination: String,
    pub protocol: String,
    pub forwarded: u64,
    pub dropped: u64,
    pub other: u64,
    pub total: u64,
    /// Dropped / (forwarded + dropped); null when neither was seen.
    pub drop_ratio: Option<f64>,
    pub flows_per_sec: f64,
    pub drop_reasons: Vec<ReasonCount>,
    /// Busiest destination ports first.
    pub ports: Vec<PortCount>,
    /// Distinct destination ports seen, so a scan is visible even when only a
    /// few ports are listed.
    pub port_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReasonCount {
    pub reason: String,
    pub dropped: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PortCount {
    pub port: u16,
    pub flows: u64,
}

/// Golden signals for one service as the receiver and as the caller.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Service {
    pub service: String,
    pub inbound_flows: u64,
    pub inbound_dropped: u64,
    pub inbound_drop_ratio: Option<f64>,
    pub inbound_flows_per_sec: f64,
    pub outbound_flows: u64,
    pub outbound_dropped: u64,
    pub outbound_drop_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Folded {
    pub edges: usize,
    pub flows: u64,
    pub dropped: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EdgeReport {
    pub granularity: &'static str,
    pub edges: Vec<Edge>,
    pub services: Vec<Service>,
    /// Edges beyond `max_edges`, summed: they are counted, not listed.
    pub folded: Folded,
    pub total_flows: u64,
    /// True when the store had more groups than were read (the quietest were
    /// cut), so counts are lower bounds.
    pub truncated: bool,
    /// What flows cannot tell, so no reader mistakes absence for health.
    pub not_measured: &'static str,
}

fn ratio(dropped: u64, forwarded: u64) -> Option<f64> {
    let judged = dropped.saturating_add(forwarded);
    (judged > 0).then(|| round(dropped as f64 / judged as f64, 4))
}

#[derive(Default)]
struct EdgeAcc {
    forwarded: u64,
    dropped: u64,
    other: u64,
    reasons: HashMap<String, u64>,
    ports: HashMap<u16, u64>,
}

#[derive(Default)]
struct SvcAcc {
    in_fwd: u64,
    in_drop: u64,
    in_other: u64,
    out_fwd: u64,
    out_drop: u64,
    out_other: u64,
}

fn protocol_label(p: &str) -> String {
    let p: String = p
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(MAX_PROTOCOL_CHARS)
        .collect::<String>()
        .to_ascii_uppercase();
    if p.is_empty() {
        "UNKNOWN".into()
    } else {
        p
    }
}

/// Fold grouped rows into edges and services.
///
/// `range_secs` is the length of the window the rows cover, for the rate.
/// `max_edges` is clamped to `1..=MAX_MAX_EDGES`. Deterministic: ties on
/// flow count are broken by name.
pub fn build(
    pairs: &[PairCount],
    g: Granularity,
    range_secs: i64,
    max_edges: usize,
    truncated: bool,
) -> EdgeReport {
    let max_edges = max_edges.clamp(1, MAX_MAX_EDGES);
    let secs = range_secs.max(1) as f64;

    let mut edges: HashMap<(String, String, String), EdgeAcc> = HashMap::new();
    let mut svcs: HashMap<String, SvcAcc> = HashMap::new();
    let mut total_flows = 0u64;

    for p in pairs {
        let src = endpoint_label(&p.src_namespace, &p.src_pod, g);
        let dst = endpoint_label(&p.dst_namespace, &p.dst_pod, g);
        total_flows = total_flows.saturating_add(p.total());

        let s = svcs.entry(src.clone()).or_default();
        s.out_fwd = s.out_fwd.saturating_add(p.forwarded);
        s.out_drop = s.out_drop.saturating_add(p.dropped);
        s.out_other = s.out_other.saturating_add(p.other);
        let d = svcs.entry(dst.clone()).or_default();
        d.in_fwd = d.in_fwd.saturating_add(p.forwarded);
        d.in_drop = d.in_drop.saturating_add(p.dropped);
        d.in_other = d.in_other.saturating_add(p.other);

        let e = edges
            .entry((src, dst, protocol_label(&p.protocol)))
            .or_default();
        e.forwarded = e.forwarded.saturating_add(p.forwarded);
        e.dropped = e.dropped.saturating_add(p.dropped);
        e.other = e.other.saturating_add(p.other);
        if p.dropped > 0 && !p.drop_reason.is_empty() {
            let reason: String = super::flow_agg::truncate(&p.drop_reason);
            let r = e.reasons.entry(reason).or_default();
            *r = r.saturating_add(p.dropped);
        }
        let pc = e.ports.entry(p.port).or_default();
        *pc = pc.saturating_add(p.total());
    }

    let mut listed: Vec<Edge> = edges
        .into_iter()
        .map(|((source, destination, protocol), a)| {
            let total = a
                .forwarded
                .saturating_add(a.dropped)
                .saturating_add(a.other);
            let mut reasons: Vec<ReasonCount> = a
                .reasons
                .into_iter()
                .map(|(reason, dropped)| ReasonCount { reason, dropped })
                .collect();
            reasons.sort_by(|x, y| y.dropped.cmp(&x.dropped).then(x.reason.cmp(&y.reason)));
            reasons.truncate(TOP_REASONS);
            let port_count = a.ports.len();
            let mut ports: Vec<PortCount> = a
                .ports
                .into_iter()
                .map(|(port, flows)| PortCount { port, flows })
                .collect();
            ports.sort_by(|x, y| y.flows.cmp(&x.flows).then(x.port.cmp(&y.port)));
            ports.truncate(TOP_PORTS);
            Edge {
                source,
                destination,
                protocol,
                forwarded: a.forwarded,
                dropped: a.dropped,
                other: a.other,
                total,
                drop_ratio: ratio(a.dropped, a.forwarded),
                flows_per_sec: round(total as f64 / secs, 4),
                drop_reasons: reasons,
                ports,
                port_count,
            }
        })
        .collect();
    listed.sort_by(|a, b| {
        b.total.cmp(&a.total).then_with(|| {
            (&a.source, &a.destination, &a.protocol).cmp(&(&b.source, &b.destination, &b.protocol))
        })
    });

    let mut folded = Folded::default();
    if listed.len() > max_edges {
        for e in listed.drain(max_edges..) {
            folded.edges += 1;
            folded.flows = folded.flows.saturating_add(e.total);
            folded.dropped = folded.dropped.saturating_add(e.dropped);
        }
    }

    let mut services: Vec<Service> = svcs
        .into_iter()
        .map(|(service, a)| {
            let inbound = a
                .in_fwd
                .saturating_add(a.in_drop)
                .saturating_add(a.in_other);
            Service {
                service,
                inbound_flows: inbound,
                inbound_dropped: a.in_drop,
                inbound_drop_ratio: ratio(a.in_drop, a.in_fwd),
                inbound_flows_per_sec: round(inbound as f64 / secs, 4),
                outbound_flows: a
                    .out_fwd
                    .saturating_add(a.out_drop)
                    .saturating_add(a.out_other),
                outbound_dropped: a.out_drop,
                outbound_drop_ratio: ratio(a.out_drop, a.out_fwd),
            }
        })
        .collect();
    services.sort_by(|a, b| {
        b.inbound_flows
            .cmp(&a.inbound_flows)
            .then(b.outbound_flows.cmp(&a.outbound_flows))
            .then(a.service.cmp(&b.service))
    });
    services.truncate(max_edges);

    EdgeReport {
        granularity: g.as_str(),
        edges: listed,
        services,
        folded,
        total_flows,
        truncated,
        not_measured: "Latency and saturation are not in L3/L4 flow records. Errors here are flows \
                       Cilium dropped; an application error over a forwarded connection is not visible.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc(
        src: (&str, &str),
        dst: (&str, &str),
        port: u16,
        fwd: u64,
        drop: u64,
        reason: &str,
    ) -> PairCount {
        PairCount {
            src_namespace: src.0.into(),
            src_pod: src.1.into(),
            dst_namespace: dst.0.into(),
            dst_pod: dst.1.into(),
            protocol: "TCP".into(),
            port,
            drop_reason: reason.into(),
            forwarded: fwd,
            dropped: drop,
            other: 0,
        }
    }

    fn fixture() -> Vec<PairCount> {
        vec![
            // Two replicas of web call the same db: one edge at workload level.
            pc(
                ("prod", "web-7c9d8f6b5-x2k4p"),
                ("prod", "db-0"),
                5432,
                90,
                0,
                "",
            ),
            pc(
                ("prod", "web-7c9d8f6b5-z9c4d"),
                ("prod", "db-0"),
                5432,
                60,
                0,
                "",
            ),
            pc(
                ("prod", "web-7c9d8f6b5-z9c4d"),
                ("prod", "db-0"),
                5432,
                0,
                10,
                "Policy denied",
            ),
            pc(
                ("prod", "web-7c9d8f6b5-m5n6p"),
                ("prod", "db-0"),
                5432,
                0,
                5,
                "Policy denied",
            ),
            pc(
                ("prod", "web-7c9d8f6b5-m5n6p"),
                ("prod", "db-0"),
                5432,
                0,
                2,
                "TTL exceeded",
            ),
            pc(("", ""), ("prod", "web-7c9d8f6b5-x2k4p"), 8080, 30, 0, ""),
        ]
    }

    #[test]
    fn replicas_merge_into_one_workload_edge_with_golden_signals() {
        let r = build(&fixture(), Granularity::Workload, 100, 50, false);
        let e = r
            .edges
            .iter()
            .find(|e| e.source == "prod/web" && e.destination == "prod/db")
            .expect("web -> db");
        assert_eq!((e.forwarded, e.dropped, e.total), (150, 17, 167));
        assert_eq!(e.drop_ratio, Some(0.1018));
        assert_eq!(e.flows_per_sec, 1.67);
        assert_eq!(
            e.drop_reasons[0],
            ReasonCount {
                reason: "Policy denied".into(),
                dropped: 15
            }
        );
        assert_eq!(e.drop_reasons[1].reason, "TTL exceeded");
        assert_eq!(
            e.ports,
            vec![PortCount {
                port: 5432,
                flows: 167
            }]
        );
        assert_eq!(r.total_flows, 197);
        let db = r.services.iter().find(|s| s.service == "prod/db").unwrap();
        assert_eq!((db.inbound_flows, db.inbound_dropped), (167, 17));
        assert_eq!(db.inbound_drop_ratio, Some(0.1018));
        // The world caller is one node, not one per IP.
        assert!(r.services.iter().any(|s| s.service == "external"));
        assert!(!r.truncated);
    }

    #[test]
    fn pod_granularity_keeps_replicas_apart() {
        let r = build(&fixture(), Granularity::Pod, 100, 50, false);
        assert_eq!(r.edges.len(), 4);
        let ns = build(&fixture(), Granularity::Namespace, 100, 50, false);
        assert_eq!(ns.edges.len(), 2); // prod->prod and external->prod
    }

    #[test]
    fn quiet_edge_has_no_ratio_and_zero_window_is_safe() {
        let mut p = pc(("a", "x"), ("b", "y"), 1, 0, 0, "");
        p.other = 4;
        let r = build(&[p], Granularity::Pod, 0, 10, false);
        assert_eq!(r.edges[0].drop_ratio, None);
        assert!(r.edges[0].flows_per_sec.is_finite());
    }

    #[test]
    fn cardinality_bomb_is_capped_and_folded() {
        // 5000 distinct one-flow peers, all hitting one service, plus a heavy edge.
        let mut pairs = vec![pc(("p", "a-0"), ("p", "b-0"), 80, 1000, 0, "")];
        for i in 0..5000 {
            pairs.push(pc(
                ("p", &format!("scan-{i}")),
                ("p", "b-0"),
                80,
                1,
                1,
                "Policy denied",
            ));
        }
        let r = build(&pairs, Granularity::Pod, 60, 50, true);
        assert_eq!(r.edges.len(), 50);
        assert_eq!(r.services.len(), 50);
        assert_eq!(r.edges[0].source, "p/a-0");
        assert_eq!(r.folded.edges, 5001 - 50);
        // Nothing lost: listed + folded flows add up to the total.
        let listed: u64 = r.edges.iter().map(|e| e.total).sum();
        assert_eq!(listed + r.folded.flows, r.total_flows);
        assert!(r.truncated);
        // Even the heaviest service numbers are intact.
        let b = r.services.iter().find(|s| s.service == "p/b-0").unwrap();
        assert_eq!(b.inbound_flows, 1000 + 5000 * 2);
    }

    #[test]
    fn port_scan_on_one_edge_stays_small() {
        let pairs: Vec<PairCount> = (1..=60_000u32)
            .step_by(7)
            .map(|p| pc(("a", "x"), ("b", "y"), p as u16, 1, 0, ""))
            .collect();
        let r = build(&pairs, Granularity::Pod, 60, 10, false);
        assert_eq!(r.edges.len(), 1);
        assert_eq!(r.edges[0].ports.len(), TOP_PORTS);
        assert_eq!(r.edges[0].port_count, pairs.len());
    }

    #[test]
    fn hostile_strings_are_sanitised_and_limits_clamped() {
        let mut p = pc(("n\u{0}s", "p\nod"), ("b", "y"), 1, 1, 1, &"R".repeat(5000));
        p.protocol = "\u{1b}[2Jtcp; DROP TABLE".into();
        let r = build(&[p], Granularity::Pod, 10, 0, false);
        assert_eq!(r.edges.len(), 1); // max_edges 0 clamps to 1
        let e = &r.edges[0];
        assert!(!e.source.chars().any(char::is_control));
        assert!(e.protocol.len() <= MAX_PROTOCOL_CHARS);
        assert_eq!(
            e.drop_reasons[0].reason.chars().count(),
            super::super::flow_agg::MAX_LABEL_CHARS
        );
        let huge = build(&[], Granularity::Pod, 10, usize::MAX, false);
        assert!(huge.edges.is_empty() && huge.services.is_empty());
        assert_eq!(huge.total_flows, 0);
    }

    #[test]
    fn counters_saturate_instead_of_overflowing() {
        let a = pc(("a", "x"), ("b", "y"), 1, u64::MAX, 0, "");
        let b = pc(("a", "x"), ("b", "y"), 1, u64::MAX, u64::MAX, "r");
        let r = build(&[a, b], Granularity::Pod, 10, 5, false);
        assert_eq!(r.edges[0].total, u64::MAX);
    }

    #[test]
    fn output_is_deterministic() {
        let mut shuffled = fixture();
        shuffled.reverse();
        let a = serde_json::to_string(&build(&fixture(), Granularity::Workload, 100, 50, false))
            .unwrap();
        let b = serde_json::to_string(&build(&shuffled, Granularity::Workload, 100, 50, false))
            .unwrap();
        assert_eq!(a, b);
    }
}
