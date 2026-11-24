// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Top talkers: who sends, who receives and which ports carry the most flows.
//!
//! Flow records carry no byte counts, so "top" means flow count in the stored
//! sample, with the dropped share next to it. Three rankings over the same
//! window: sources, destinations and protocol/port. Each is cut to `limit`
//! and the rest is summed, so a scan cannot make the answer unbounded.

use super::flow_agg::{endpoint_label, round, truncate, Granularity, PairCount};
use serde::Serialize;
use std::collections::HashMap;

pub const DEFAULT_LIMIT: usize = 10;
pub const MAX_LIMIT: usize = 100;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Talker {
    pub name: String,
    pub flows: u64,
    /// Share of all flows in the window.
    pub share: f64,
    pub dropped: u64,
    pub drop_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Ranking {
    pub entries: Vec<Talker>,
    /// Distinct names seen, listed or not.
    pub distinct: usize,
    /// Flows from names beyond `limit`.
    pub other_flows: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TopReport {
    pub granularity: &'static str,
    pub total_flows: u64,
    pub sources: Ranking,
    pub destinations: Ranking,
    pub ports: Ranking,
    pub truncated: bool,
    pub note: &'static str,
}

#[derive(Default)]
struct Acc {
    flows: u64,
    forwarded: u64,
    dropped: u64,
}

fn rank(acc: HashMap<String, Acc>, total: u64, limit: usize) -> Ranking {
    let distinct = acc.len();
    let mut v: Vec<(String, Acc)> = acc.into_iter().collect();
    v.sort_by(|a, b| b.1.flows.cmp(&a.1.flows).then_with(|| a.0.cmp(&b.0)));
    let other_flows = v
        .iter()
        .skip(limit)
        .fold(0u64, |s, (_, a)| s.saturating_add(a.flows));
    let entries = v
        .into_iter()
        .take(limit)
        .map(|(name, a)| {
            let judged = a.forwarded.saturating_add(a.dropped);
            Talker {
                name,
                flows: a.flows,
                share: if total == 0 {
                    0.0
                } else {
                    round(a.flows as f64 / total as f64, 4)
                },
                dropped: a.dropped,
                drop_ratio: (judged > 0).then(|| round(a.dropped as f64 / judged as f64, 4)),
            }
        })
        .collect();
    Ranking {
        entries,
        distinct,
        other_flows,
    }
}

fn add(m: &mut HashMap<String, Acc>, key: String, p: &PairCount) {
    let a = m.entry(key).or_default();
    a.flows = a.flows.saturating_add(p.total());
    a.forwarded = a.forwarded.saturating_add(p.forwarded);
    a.dropped = a.dropped.saturating_add(p.dropped);
}

/// Rank sources, destinations and ports. `limit` is clamped to `1..=MAX_LIMIT`.
pub fn report(pairs: &[PairCount], g: Granularity, limit: usize, truncated: bool) -> TopReport {
    let limit = limit.clamp(1, MAX_LIMIT);
    let (mut src, mut dst, mut ports) = (HashMap::new(), HashMap::new(), HashMap::new());
    let mut total = 0u64;
    for p in pairs {
        total = total.saturating_add(p.total());
        add(&mut src, endpoint_label(&p.src_namespace, &p.src_pod, g), p);
        add(&mut dst, endpoint_label(&p.dst_namespace, &p.dst_pod, g), p);
        let proto: String = truncate(&p.protocol).chars().take(16).collect();
        let proto = if proto.is_empty() { "?".into() } else { proto };
        add(&mut ports, format!("{}/{}", proto, p.port), p);
    }
    TopReport {
        granularity: g.as_str(),
        total_flows: total,
        sources: rank(src, total, limit),
        destinations: rank(dst, total, limit),
        ports: rank(ports, total, limit),
        truncated,
        note: "Ranked by flow count in the stored sample: flow records carry no byte counts, so \
               this is not bandwidth. Endpoints without a namespace or pod (world, host) are one \
               external entry.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc(src: &str, dst: &str, port: u16, fwd: u64, drop: u64) -> PairCount {
        PairCount {
            src_namespace: "n".into(),
            src_pod: src.into(),
            dst_namespace: if dst == "ext" { "".into() } else { "n".into() },
            dst_pod: if dst == "ext" { "".into() } else { dst.into() },
            protocol: "TCP".into(),
            port,
            drop_reason: String::new(),
            forwarded: fwd,
            dropped: drop,
            other: 0,
        }
    }

    #[test]
    fn ranks_three_ways_with_shares() {
        let p = vec![
            pc("a", "b", 80, 50, 0),
            pc("a", "c", 443, 30, 10),
            pc("d", "b", 80, 10, 0),
        ];
        let r = report(&p, Granularity::Pod, 10, false);
        assert_eq!(r.total_flows, 100);
        assert_eq!(r.sources.entries[0].name, "n/a");
        assert_eq!(r.sources.entries[0].flows, 90);
        assert_eq!(r.sources.entries[0].share, 0.9);
        assert_eq!(r.sources.entries[0].dropped, 10);
        assert_eq!(r.destinations.entries[0].name, "n/b");
        assert_eq!(r.ports.entries[0].name, "TCP/80");
        assert_eq!(r.ports.entries[0].flows, 60);
        assert_eq!(r.ports.entries[1].drop_ratio, Some(0.25));
    }

    #[test]
    fn limit_folds_the_rest_and_counts_distinct() {
        let p: Vec<_> = (0..30)
            .map(|i| pc(&format!("s{i:02}"), "x", 80, 30 - i, 0))
            .collect();
        let r = report(&p, Granularity::Pod, 5, false);
        assert_eq!(r.sources.entries.len(), 5);
        assert_eq!(r.sources.distinct, 30);
        let listed: u64 = r.sources.entries.iter().map(|e| e.flows).sum();
        assert_eq!(listed + r.sources.other_flows, r.total_flows);
        // ties broken by name, so the output is stable
        assert_eq!(r.sources.entries[0].name, "n/s00");
        assert_eq!(
            report(&p, Granularity::Pod, 0, false).sources.entries.len(),
            1
        );
        assert_eq!(
            report(&p, Granularity::Pod, 9999, false)
                .sources
                .entries
                .len(),
            30
        );
    }

    #[test]
    fn external_endpoints_are_one_entry_and_empty_input_is_fine() {
        let p = vec![pc("a", "ext", 53, 5, 0), pc("b", "ext", 53, 5, 0)];
        let r = report(&p, Granularity::Workload, 10, false);
        assert_eq!(r.destinations.entries.len(), 1);
        assert_eq!(r.destinations.entries[0].name, "external");
        assert_eq!(r.destinations.entries[0].flows, 10);
        let e = report(&[], Granularity::Pod, 10, false);
        assert_eq!(e.total_flows, 0);
        assert!(e.sources.entries.is_empty());
    }

    #[test]
    fn flows_without_a_verdict_have_no_ratio() {
        let mut x = pc("a", "b", 1, 0, 0);
        x.other = 4;
        let r = report(&[x], Granularity::Pod, 3, false);
        assert_eq!(r.sources.entries[0].flows, 4);
        assert!(r.sources.entries[0].drop_ratio.is_none());
    }
}
