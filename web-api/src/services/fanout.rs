// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Fan-out: which sources reach for many destinations or many ports.
//!
//! A pod that suddenly tries dozens of peers, or dozens of ports on one peer,
//! and mostly gets dropped by Cilium is what lateral movement and port scans
//! look like in flow records. This ranks sources by how many distinct
//! destinations and distinct ports on one destination they touched, and marks
//! a source as a `suspect` when it also has a high drop ratio (probing
//! unauthorised targets is mostly denied).
//!
//! It is a heuristic over sampled L3/L4 flows: it notifies, it never blocks,
//! and a busy legitimate client (a scraper, a service mesh probe) can match,
//! which is why the evidence is listed. Pure and bounded.

use super::flow_agg::{endpoint_label, round, Granularity, PairCount, EXTERNAL};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

pub const DEFAULT_MIN_TARGETS: usize = 20;
pub const MIN_MIN_TARGETS: usize = 2;
pub const MAX_MIN_TARGETS: usize = 100_000;
pub const DEFAULT_MIN_DROP_RATIO: f64 = 0.3;
pub const DEFAULT_LIMIT: usize = 50;
pub const MAX_LIMIT: usize = 1000;
/// Destinations listed as evidence per source.
const SAMPLE: usize = 5;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Source {
    pub source: String,
    /// Distinct destinations reached.
    pub destinations: usize,
    /// Most distinct ports tried on a single destination.
    pub max_ports_on_one_destination: usize,
    pub flows: u64,
    pub dropped: u64,
    pub drop_ratio: Option<f64>,
    /// Many targets and mostly denied.
    pub suspect: bool,
    /// Why it was listed: `many_destinations`, `many_ports`.
    pub reasons: Vec<&'static str>,
    /// Destination with the most ports, and a few more destinations seen.
    pub widest_destination: Option<String>,
    pub sample_destinations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FanoutReport {
    pub granularity: &'static str,
    pub min_targets: usize,
    pub min_drop_ratio: f64,
    pub sources_seen: usize,
    pub sources_over_threshold: usize,
    pub suspects: usize,
    pub sources: Vec<Source>,
    pub truncated: bool,
    pub caveat: &'static str,
}

#[derive(Default)]
struct Acc {
    flows: u64,
    forwarded: u64,
    dropped: u64,
    dests: HashSet<String>,
    ports: BTreeMap<String, HashSet<u16>>,
}

/// Rank sources by fan-out. `min_targets` is clamped to
/// `MIN_MIN_TARGETS..=MAX_MIN_TARGETS`, `min_drop_ratio` to 0..=1 (NaN counts
/// as the default), `limit` to `1..=MAX_LIMIT`. Endpoints without a pod or
/// namespace (world, host) are one `external` node and are never a source:
/// their addresses are not kept, so their fan-out cannot be measured.
pub fn analyse(
    pairs: &[PairCount],
    g: Granularity,
    min_targets: usize,
    min_drop_ratio: f64,
    limit: usize,
    truncated: bool,
) -> FanoutReport {
    let min_targets = min_targets.clamp(MIN_MIN_TARGETS, MAX_MIN_TARGETS);
    let min_drop_ratio = if min_drop_ratio.is_nan() {
        DEFAULT_MIN_DROP_RATIO
    } else {
        min_drop_ratio.clamp(0.0, 1.0)
    };
    let limit = limit.clamp(1, MAX_LIMIT);

    let mut by_src: BTreeMap<String, Acc> = BTreeMap::new();
    for p in pairs {
        let src = endpoint_label(&p.src_namespace, &p.src_pod, g);
        if src == EXTERNAL {
            continue;
        }
        let dst = endpoint_label(&p.dst_namespace, &p.dst_pod, g);
        if dst == src {
            continue; // talking to its own workload is not fan-out
        }
        let a = by_src.entry(src).or_default();
        a.flows = a.flows.saturating_add(p.total());
        a.forwarded = a.forwarded.saturating_add(p.forwarded);
        a.dropped = a.dropped.saturating_add(p.dropped);
        a.ports.entry(dst.clone()).or_default().insert(p.port);
        a.dests.insert(dst);
    }

    let sources_seen = by_src.len();
    let mut out: Vec<Source> = Vec::new();
    for (source, a) in by_src {
        // BTreeMap order makes the tie-break (first name) deterministic.
        let (widest, max_ports) =
            a.ports
                .iter()
                .fold((None::<&String>, 0usize), |best, (d, ps)| {
                    if ps.len() > best.1 {
                        (Some(d), ps.len())
                    } else {
                        best
                    }
                });
        let mut reasons = Vec::new();
        if a.dests.len() >= min_targets {
            reasons.push("many_destinations");
        }
        if max_ports >= min_targets {
            reasons.push("many_ports");
        }
        if reasons.is_empty() {
            continue;
        }
        let judged = a.forwarded.saturating_add(a.dropped);
        let ratio = (judged > 0).then(|| a.dropped as f64 / judged as f64);
        // First names in order: deterministic evidence, bounded.
        let sample: Vec<String> = a.ports.keys().take(SAMPLE).cloned().collect();
        out.push(Source {
            source,
            destinations: a.dests.len(),
            max_ports_on_one_destination: max_ports,
            flows: a.flows,
            dropped: a.dropped,
            drop_ratio: ratio.map(|r| round(r, 4)),
            suspect: ratio.is_some_and(|r| r >= min_drop_ratio) && a.dropped > 0,
            reasons,
            widest_destination: widest.cloned(),
            sample_destinations: sample,
        });
    }
    out.sort_by(|a, b| {
        b.suspect
            .cmp(&a.suspect)
            .then(
                b.destinations
                    .max(b.max_ports_on_one_destination)
                    .cmp(&a.destinations.max(a.max_ports_on_one_destination)),
            )
            .then(b.flows.cmp(&a.flows))
            .then_with(|| a.source.cmp(&b.source))
    });
    let over = out.len();
    let suspects = out.iter().filter(|s| s.suspect).count();
    out.truncate(limit);

    FanoutReport {
        granularity: g.as_str(),
        min_targets,
        min_drop_ratio,
        sources_seen,
        sources_over_threshold: over,
        suspects,
        sources: out,
        truncated,
        caveat:
            "Heuristic over sampled L3/L4 flows. A source is a suspect when it reached at least \
                 min_targets destinations, or min_targets ports on one destination, and Cilium \
                 dropped at least min_drop_ratio of its judged flows. Scanners, health checkers \
                 and scrapers can match. Flows from outside the cluster are one external node and \
                 are not measured. Notifies only; nothing is blocked.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc(src: &str, dst: &str, port: u16, fwd: u64, drop: u64) -> PairCount {
        PairCount {
            src_namespace: "p".into(),
            src_pod: src.into(),
            dst_namespace: "p".into(),
            dst_pod: dst.into(),
            protocol: "TCP".into(),
            port,
            drop_reason: String::new(),
            forwarded: fwd,
            dropped: drop,
            other: 0,
        }
    }

    fn ana(p: &[PairCount]) -> FanoutReport {
        analyse(p, Granularity::Pod, 20, 0.3, 50, false)
    }

    #[test]
    fn host_sweep_with_denials_is_a_suspect() {
        let p: Vec<_> = (0..30)
            .map(|i| pc("evil", &format!("t-{i}"), 22, 1, 4))
            .collect();
        let r = ana(&p);
        assert_eq!(r.sources.len(), 1);
        let s = &r.sources[0];
        assert_eq!(s.source, "p/evil");
        assert_eq!((s.destinations, s.flows, s.dropped), (30, 150, 120));
        assert!(s.suspect);
        assert_eq!(s.reasons, vec!["many_destinations"]);
        assert_eq!(s.sample_destinations.len(), 5);
        assert_eq!((r.suspects, r.sources_seen), (1, 1));
    }

    #[test]
    fn port_scan_on_one_destination_is_found() {
        let p: Vec<_> = (1..=40)
            .map(|port| pc("evil", "victim", port, 0, 1))
            .collect();
        let r = ana(&p);
        let s = &r.sources[0];
        assert_eq!(s.destinations, 1);
        assert_eq!(s.max_ports_on_one_destination, 40);
        assert_eq!(s.widest_destination.as_deref(), Some("p/victim"));
        assert!(s.suspect && s.reasons == vec!["many_ports"]);
    }

    #[test]
    fn busy_healthy_client_is_listed_but_not_a_suspect() {
        let p: Vec<_> = (0..25)
            .map(|i| pc("scraper", &format!("t-{i}"), 9100, 100, 0))
            .collect();
        let r = ana(&p);
        assert_eq!(r.sources.len(), 1);
        assert!(!r.sources[0].suspect);
        assert_eq!(r.suspects, 0);
    }

    #[test]
    fn below_threshold_and_self_traffic_are_ignored() {
        let mut p: Vec<_> = (0..19)
            .map(|i| pc("a", &format!("t-{i}"), 80, 1, 9))
            .collect();
        for i in 0..50 {
            p.push(pc("b", "b", 1000 + i, 1, 0)); // same endpoint
        }
        let r = ana(&p);
        assert!(r.sources.is_empty());
        assert_eq!(r.sources_seen, 1);
    }

    #[test]
    fn external_sources_are_never_measured() {
        let mut p: Vec<PairCount> = Vec::new();
        for i in 0..100 {
            let mut x = pc("", &format!("t-{i}"), 22, 0, 1);
            x.src_namespace = String::new();
            p.push(x);
        }
        let r = ana(&p);
        assert!(r.sources.is_empty() && r.sources_seen == 0);
    }

    #[test]
    fn workload_granularity_merges_replicas_before_counting() {
        // Three replicas each reach 8 distinct targets: 24 together at workload level.
        let mut p = Vec::new();
        for (r, rep) in ["x2k4p", "z9c4d", "m5n6p"].iter().enumerate() {
            for i in 0..8 {
                p.push(pc(
                    &format!("job-7c9d8f6b5-{rep}"),
                    &format!("target{}", r * 8 + i),
                    80,
                    1,
                    0,
                ));
            }
        }
        assert!(ana(&p).sources.is_empty());
        let r = analyse(&p, Granularity::Workload, 20, 0.3, 50, false);
        assert_eq!(r.sources[0].destinations, 24);
    }

    #[test]
    fn ordering_limit_and_clamps() {
        let mut p = Vec::new();
        for s in 0..10 {
            for i in 0..(20 + s) {
                p.push(pc(&format!("s{s}"), &format!("t-{i}"), 22, 0, 1));
            }
        }
        let r = analyse(&p, Granularity::Pod, 20, 0.3, 3, false);
        assert_eq!(r.sources_over_threshold, 10);
        assert_eq!(r.sources.len(), 3);
        assert_eq!(r.sources[0].source, "p/s9"); // widest first
                                                 // Hostile knobs do not panic or widen the output.
        let r = analyse(&p, Granularity::Pod, 0, f64::NAN, 0, true);
        assert_eq!(
            (r.min_targets, r.min_drop_ratio, r.sources.len()),
            (2, 0.3, 1)
        );
        assert!(r.truncated);
        let r = analyse(&p, Granularity::Pod, usize::MAX, 5.0, usize::MAX, false);
        assert_eq!((r.min_drop_ratio, r.sources.len()), (1.0, 0));
    }

    #[test]
    fn large_scan_is_bounded() {
        let p: Vec<_> = (0..20_000)
            .map(|i| pc("evil", &format!("t-{i}"), 22, 0, 1))
            .collect();
        let r = ana(&p);
        assert_eq!(r.sources[0].destinations, 20_000);
        assert_eq!(r.sources[0].sample_destinations.len(), 5);
    }
}
