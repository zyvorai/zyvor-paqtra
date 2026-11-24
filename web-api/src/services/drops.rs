// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Drop report: why Cilium dropped flows, split by cause.
//!
//! "Dropped" is not one thing. A policy denial is a decision somebody made; a
//! conntrack or NAT map insertion failure is a datapath that ran out of room;
//! an unroutable address is a routing or service problem. Reading them as one
//! number sends an operator to edit policy when the fix is a map size. This
//! groups the drop reasons Hubble reports into classes, ranks them and lists
//! the edges behind each. The class comes from the reason text, so it is a
//! reading aid, not a verdict. Pure and bounded.

use super::flow_agg::{endpoint_label, round, truncate, Granularity, PairCount};
use serde::Serialize;
use std::collections::HashMap;

pub const DEFAULT_MAX_REASONS: usize = 20;
pub const MAX_MAX_REASONS: usize = 200;
const TOP_EDGES: usize = 3;

/// source, destination, protocol, port
type EdgeKey = (String, String, String, u16);

/// Cause of a drop, from Cilium's `drop_reason_desc` text.
pub fn classify(reason: &str) -> &'static str {
    let r = reason.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| r.contains(n));
    if r.trim().is_empty() {
        "unspecified"
    } else if has(&["policy denied", "denylist", "authentication required"]) {
        "policy"
    } else if has(&["map insertion", "nat mapping", "no mapping for nat"]) {
        "datapath_capacity"
    } else if has(&[
        "invalid",
        "unsupported",
        "unknown l",
        "unknown icmp",
        "truncated",
        "parsing",
    ]) {
        "invalid_packet"
    } else if has(&[
        "unroutable",
        "no tunnel",
        "no node id",
        "backend not found",
        "ttl",
        "fragmentation",
        "not ready",
        "unencrypted",
    ]) {
        "routing_or_service"
    } else {
        "other"
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EdgeDrops {
    pub source: String,
    pub destination: String,
    pub protocol: String,
    pub port: u16,
    pub dropped: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReasonRow {
    pub reason: String,
    pub class: &'static str,
    pub dropped: u64,
    pub share: f64,
    /// Distinct source/destination/port combinations hit.
    pub edges: usize,
    pub top_edges: Vec<EdgeDrops>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ClassRow {
    pub class: &'static str,
    pub dropped: u64,
    pub share: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DropReport {
    pub granularity: &'static str,
    pub total_flows: u64,
    pub total_dropped: u64,
    pub drop_ratio: Option<f64>,
    pub classes: Vec<ClassRow>,
    pub reasons: Vec<ReasonRow>,
    /// Reasons beyond `max_reasons`, summed.
    pub omitted_reasons: usize,
    pub omitted_dropped: u64,
    pub truncated: bool,
    pub caveat: &'static str,
}

/// Build the report. `max_reasons` is clamped to `1..=MAX_MAX_REASONS`.
pub fn report(
    pairs: &[PairCount],
    g: Granularity,
    max_reasons: usize,
    truncated: bool,
) -> DropReport {
    let max_reasons = max_reasons.clamp(1, MAX_MAX_REASONS);
    let mut total_flows = 0u64;
    let mut judged_fwd = 0u64;
    let mut total_dropped = 0u64;
    let mut by_reason: HashMap<String, HashMap<EdgeKey, u64>> = HashMap::new();
    for p in pairs {
        total_flows = total_flows.saturating_add(p.total());
        judged_fwd = judged_fwd.saturating_add(p.forwarded);
        total_dropped = total_dropped.saturating_add(p.dropped);
        if p.dropped == 0 {
            continue;
        }
        let key = (
            endpoint_label(&p.src_namespace, &p.src_pod, g),
            endpoint_label(&p.dst_namespace, &p.dst_pod, g),
            super::flow_agg::truncate(&p.protocol)
                .chars()
                .take(16)
                .collect(),
            p.port,
        );
        let e = by_reason
            .entry(truncate(&p.drop_reason))
            .or_default()
            .entry(key)
            .or_insert(0);
        *e = e.saturating_add(p.dropped);
    }

    let share = |n: u64| {
        if total_dropped == 0 {
            0.0
        } else {
            round(n as f64 / total_dropped as f64, 4)
        }
    };

    let mut rows: Vec<ReasonRow> = by_reason
        .into_iter()
        .map(|(reason, edges)| {
            let dropped: u64 = edges.values().fold(0, |a, v| a.saturating_add(*v));
            let n = edges.len();
            let mut list: Vec<EdgeDrops> = edges
                .into_iter()
                .map(
                    |((source, destination, protocol, port), dropped)| EdgeDrops {
                        source,
                        destination,
                        protocol,
                        port,
                        dropped,
                    },
                )
                .collect();
            list.sort_by(|a, b| {
                b.dropped.cmp(&a.dropped).then_with(|| {
                    (&a.source, &a.destination, a.port).cmp(&(&b.source, &b.destination, b.port))
                })
            });
            list.truncate(TOP_EDGES);
            ReasonRow {
                class: classify(&reason),
                reason: if reason.is_empty() {
                    "(no reason reported)".into()
                } else {
                    reason
                },
                dropped,
                share: share(dropped),
                edges: n,
                top_edges: list,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.dropped
            .cmp(&a.dropped)
            .then_with(|| a.reason.cmp(&b.reason))
    });

    let mut classes: HashMap<&'static str, u64> = HashMap::new();
    for r in &rows {
        let c = classes.entry(r.class).or_insert(0);
        *c = c.saturating_add(r.dropped);
    }
    let mut classes: Vec<ClassRow> = classes
        .into_iter()
        .map(|(class, dropped)| ClassRow {
            class,
            dropped,
            share: share(dropped),
        })
        .collect();
    classes.sort_by(|a, b| b.dropped.cmp(&a.dropped).then(a.class.cmp(b.class)));

    let omitted_reasons = rows.len().saturating_sub(max_reasons);
    let omitted_dropped = rows
        .iter()
        .skip(max_reasons)
        .fold(0u64, |a, r| a.saturating_add(r.dropped));
    rows.truncate(max_reasons);

    let judged = judged_fwd.saturating_add(total_dropped);
    DropReport {
        granularity: g.as_str(),
        total_flows,
        total_dropped,
        drop_ratio: (judged > 0).then(|| round(total_dropped as f64 / judged as f64, 4)),
        classes,
        reasons: rows,
        omitted_reasons,
        omitted_dropped,
        truncated,
        caveat: "Classes are read from Cilium's drop reason text: policy is a decision, \
                 datapath_capacity (map insertion or NAT mapping failures) means the datapath ran \
                 out of room and policy edits will not help, invalid_packet and routing_or_service \
                 point at the network or Service, not policy. Counts are over sampled flows.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc(src: &str, dst: &str, port: u16, reason: &str, fwd: u64, drop: u64) -> PairCount {
        PairCount {
            src_namespace: "shop".into(),
            src_pod: src.into(),
            dst_namespace: "shop".into(),
            dst_pod: dst.into(),
            protocol: "TCP".into(),
            port,
            drop_reason: reason.into(),
            forwarded: fwd,
            dropped: drop,
            other: 0,
        }
    }

    #[test]
    fn classifies_cilium_reasons() {
        for (r, c) in [
            ("Policy denied", "policy"),
            ("POLICY_DENIED", "other"),
            ("Policy denied by denylist", "policy"),
            ("Authentication required", "policy"),
            ("CT: Map insertion failed", "datapath_capacity"),
            ("No mapping for NAT masquerade", "datapath_capacity"),
            ("Invalid source ip", "invalid_packet"),
            ("Unknown L4 protocol", "invalid_packet"),
            ("CT: Truncated or invalid header", "invalid_packet"),
            ("Stale or unroutable IP", "routing_or_service"),
            (
                "No tunnel/encapsulation endpoint (datapath BUG!)",
                "routing_or_service",
            ),
            ("Service backend not found", "routing_or_service"),
            ("TTL exceeded", "routing_or_service"),
            ("", "unspecified"),
            ("something new", "other"),
        ] {
            assert_eq!(classify(r), c, "{r}");
        }
    }

    #[test]
    fn splits_and_ranks_reasons_with_their_edges() {
        let p = vec![
            pc("web", "db", 5432, "Policy denied", 90, 10),
            pc("web", "cache", 6379, "Policy denied", 0, 4),
            pc("web", "db", 5432, "CT: Map insertion failed", 0, 30),
            pc("web", "db", 5432, "", 60, 0),
        ];
        let r = report(&p, Granularity::Pod, 10, false);
        assert_eq!(r.total_flows, 194);
        assert_eq!(r.total_dropped, 44);
        assert_eq!(r.drop_ratio, Some(round(44.0 / 194.0, 4)));
        assert_eq!(r.reasons[0].reason, "CT: Map insertion failed");
        assert_eq!(r.reasons[0].class, "datapath_capacity");
        assert_eq!(r.reasons[1].edges, 2);
        assert_eq!(r.reasons[1].top_edges[0].destination, "shop/db");
        assert_eq!(r.classes[0].class, "datapath_capacity");
        assert_eq!(r.classes[0].share, round(30.0 / 44.0, 4));
        let sum: f64 = r.classes.iter().map(|c| c.share).sum();
        assert!((sum - 1.0).abs() < 0.001);
    }

    #[test]
    fn empty_and_all_forwarded_input_is_quiet() {
        let r = report(&[], Granularity::Pod, 5, false);
        assert_eq!((r.total_flows, r.total_dropped), (0, 0));
        assert!(r.drop_ratio.is_none() && r.reasons.is_empty() && r.classes.is_empty());
        let r = report(&[pc("a", "b", 1, "", 5, 0)], Granularity::Pod, 5, true);
        assert_eq!(r.drop_ratio, Some(0.0));
        assert!(r.truncated && r.reasons.is_empty());
    }

    #[test]
    fn a_drop_without_reason_is_labelled_and_long_tail_is_folded() {
        let mut p = vec![pc("a", "b", 1, "", 0, 3)];
        for i in 0..5 {
            p.push(pc("a", "b", 2, &format!("odd {i}"), 0, 1));
        }
        let r = report(&p, Granularity::Namespace, 2, false);
        assert_eq!(r.reasons[0].reason, "(no reason reported)");
        assert_eq!(r.reasons[0].class, "unspecified");
        assert_eq!(r.reasons.len(), 2);
        assert_eq!((r.omitted_reasons, r.omitted_dropped), (4, 4));
        assert_eq!(r.reasons[0].top_edges[0].source, "shop");
    }

    #[test]
    fn hostile_reason_text_is_bounded() {
        let long = "x\n".repeat(1000);
        let r = report(&[pc("a", "b", 1, &long, 0, 1)], Granularity::Pod, 5, false);
        assert!(r.reasons[0].reason.chars().count() <= 253);
        assert!(!r.reasons[0].reason.contains('\n'));
    }
}
