// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! What changed between two windows of stored flows.
//!
//! "What is different since yesterday / since the deploy?" is the first
//! question in most incidents. This compares a baseline window with a current
//! one, edge by edge (source, destination, protocol), and reports edges that
//! appeared, edges that vanished, edges whose drop ratio got worse and edges
//! whose traffic rate moved by a large factor.
//!
//! Pure and bounded: every list is capped and the remainder is counted, so a
//! scan cannot blow up the answer. Rates are per second so windows of
//! different length compare fairly. Thresholds are fixed and reported.

use super::flow_agg::{endpoint_label, round, Granularity, PairCount};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

pub const DEFAULT_MAX_ITEMS: usize = 100;
pub const MAX_MAX_ITEMS: usize = 1000;
/// A drop ratio must rise by at least this much (absolute) to be a regression.
pub const MIN_RATIO_RISE: f64 = 0.05;
/// And the current window must hold at least this many drops.
pub const MIN_DROPS: u64 = 5;
/// A traffic rate change needs this factor (up or down) ...
pub const MIN_RATE_FACTOR: f64 = 3.0;
/// ... and this many flows in both windows, or it is noise.
pub const MIN_FLOWS: u64 = 20;

#[derive(Debug, Clone, Default, PartialEq)]
struct Acc {
    forwarded: u64,
    dropped: u64,
    total: u64,
    ports: HashSet<u16>,
}

impl Acc {
    fn ratio(&self) -> Option<f64> {
        let judged = self.forwarded.saturating_add(self.dropped);
        (judged > 0).then(|| self.dropped as f64 / judged as f64)
    }
}

type Key = (String, String, String);

fn fold(pairs: &[PairCount], g: Granularity) -> BTreeMap<Key, Acc> {
    let mut m: BTreeMap<Key, Acc> = BTreeMap::new();
    for p in pairs {
        let proto: String = p
            .protocol
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(16)
            .collect::<String>()
            .to_ascii_uppercase();
        let key = (
            endpoint_label(&p.src_namespace, &p.src_pod, g),
            endpoint_label(&p.dst_namespace, &p.dst_pod, g),
            if proto.is_empty() {
                "UNKNOWN".into()
            } else {
                proto
            },
        );
        let a = m.entry(key).or_default();
        a.forwarded = a.forwarded.saturating_add(p.forwarded);
        a.dropped = a.dropped.saturating_add(p.dropped);
        a.total = a.total.saturating_add(p.total());
        a.ports.insert(p.port);
    }
    m
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EdgeRef {
    pub source: String,
    pub destination: String,
    pub protocol: String,
}

/// An edge present in only one of the windows.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Appeared {
    #[serde(flatten)]
    pub edge: EdgeRef,
    pub flows: u64,
    pub dropped: u64,
    pub drop_ratio: Option<f64>,
    pub flows_per_sec: f64,
    pub port_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DropRegression {
    #[serde(flatten)]
    pub edge: EdgeRef,
    pub baseline_drop_ratio: Option<f64>,
    pub current_drop_ratio: f64,
    pub current_dropped: u64,
    pub current_flows: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RateChange {
    #[serde(flatten)]
    pub edge: EdgeRef,
    pub baseline_flows_per_sec: f64,
    pub current_flows_per_sec: f64,
    /// Current rate over baseline rate (above 1 is more traffic).
    pub factor: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Counts {
    pub baseline_edges: usize,
    pub current_edges: usize,
    pub new_edges: usize,
    pub gone_edges: usize,
    pub drop_regressions: usize,
    pub rate_changes: usize,
    /// Items found beyond the per-list cap, all lists together.
    pub omitted: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DiffReport {
    pub granularity: &'static str,
    pub counts: Counts,
    pub new_edges: Vec<Appeared>,
    pub gone_edges: Vec<Appeared>,
    pub drop_regressions: Vec<DropRegression>,
    pub rate_changes: Vec<RateChange>,
    pub thresholds: Thresholds,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Thresholds {
    pub min_drop_ratio_rise: f64,
    pub min_drops: u64,
    pub min_rate_factor: f64,
    pub min_flows_both_windows: u64,
}

fn edge_ref(k: &Key) -> EdgeRef {
    EdgeRef {
        source: k.0.clone(),
        destination: k.1.clone(),
        protocol: k.2.clone(),
    }
}

fn appeared(k: &Key, a: &Acc, secs: f64) -> Appeared {
    Appeared {
        edge: edge_ref(k),
        flows: a.total,
        dropped: a.dropped,
        drop_ratio: a.ratio().map(|r| round(r, 4)),
        flows_per_sec: round(a.total as f64 / secs, 4),
        port_count: a.ports.len(),
    }
}

/// Compare `current` against `baseline`. `*_secs` are the window lengths.
/// `max_items` caps each list and is clamped to `1..=MAX_MAX_ITEMS`.
pub fn diff(
    baseline: &[PairCount],
    current: &[PairCount],
    g: Granularity,
    baseline_secs: i64,
    current_secs: i64,
    max_items: usize,
) -> DiffReport {
    let max_items = max_items.clamp(1, MAX_MAX_ITEMS);
    let (bs, cs) = (baseline_secs.max(1) as f64, current_secs.max(1) as f64);
    let base = fold(baseline, g);
    let cur = fold(current, g);

    let mut news: Vec<Appeared> = Vec::new();
    let mut gones: Vec<Appeared> = Vec::new();
    let mut regs: Vec<DropRegression> = Vec::new();
    let mut rates: Vec<RateChange> = Vec::new();

    for (k, c) in &cur {
        match base.get(k) {
            None => news.push(appeared(k, c, cs)),
            Some(b) => {
                if let Some(cr) = c.ratio() {
                    let br = b.ratio();
                    if c.dropped >= MIN_DROPS && cr - br.unwrap_or(0.0) >= MIN_RATIO_RISE {
                        regs.push(DropRegression {
                            edge: edge_ref(k),
                            baseline_drop_ratio: br.map(|r| round(r, 4)),
                            current_drop_ratio: round(cr, 4),
                            current_dropped: c.dropped,
                            current_flows: c.total,
                        });
                    }
                }
                if b.total >= MIN_FLOWS && c.total >= MIN_FLOWS {
                    let (br, cr) = (b.total as f64 / bs, c.total as f64 / cs);
                    let factor = cr / br;
                    if factor.is_finite()
                        && (factor >= MIN_RATE_FACTOR || factor <= 1.0 / MIN_RATE_FACTOR)
                    {
                        rates.push(RateChange {
                            edge: edge_ref(k),
                            baseline_flows_per_sec: round(br, 4),
                            current_flows_per_sec: round(cr, 4),
                            factor: round(factor, 2),
                        });
                    }
                }
            }
        }
    }
    for (k, b) in &base {
        if !cur.contains_key(k) {
            gones.push(appeared(k, b, bs));
        }
    }

    let by_flows = |a: &Appeared, b: &Appeared| {
        b.flows
            .cmp(&a.flows)
            .then_with(|| a.edge.source.cmp(&b.edge.source))
            .then_with(|| a.edge.destination.cmp(&b.edge.destination))
            .then_with(|| a.edge.protocol.cmp(&b.edge.protocol))
    };
    news.sort_by(by_flows);
    gones.sort_by(by_flows);
    regs.sort_by(|a, b| {
        b.current_dropped
            .cmp(&a.current_dropped)
            .then_with(|| a.edge.source.cmp(&b.edge.source))
            .then_with(|| a.edge.destination.cmp(&b.edge.destination))
    });
    // Biggest movement first, in either direction.
    let mag = |f: f64| if f >= 1.0 { f } else { 1.0 / f };
    rates.sort_by(|a, b| {
        mag(b.factor)
            .partial_cmp(&mag(a.factor))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.edge.source.cmp(&b.edge.source))
            .then_with(|| a.edge.destination.cmp(&b.edge.destination))
    });

    let mut counts = Counts {
        baseline_edges: base.len(),
        current_edges: cur.len(),
        new_edges: news.len(),
        gone_edges: gones.len(),
        drop_regressions: regs.len(),
        rate_changes: rates.len(),
        omitted: 0,
    };
    let cut = |len: usize| len.saturating_sub(max_items);
    counts.omitted = cut(news.len()) + cut(gones.len()) + cut(regs.len()) + cut(rates.len());
    news.truncate(max_items);
    gones.truncate(max_items);
    regs.truncate(max_items);
    rates.truncate(max_items);

    DiffReport {
        granularity: g.as_str(),
        counts,
        new_edges: news,
        gone_edges: gones,
        drop_regressions: regs,
        rate_changes: rates,
        thresholds: Thresholds {
            min_drop_ratio_rise: MIN_RATIO_RISE,
            min_drops: MIN_DROPS,
            min_rate_factor: MIN_RATE_FACTOR,
            min_flows_both_windows: MIN_FLOWS,
        },
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
            drop_reason: if drop > 0 {
                "Policy denied".into()
            } else {
                String::new()
            },
            forwarded: fwd,
            dropped: drop,
            other: 0,
        }
    }

    fn run(b: &[PairCount], c: &[PairCount]) -> DiffReport {
        diff(b, c, Granularity::Pod, 3600, 3600, 100)
    }

    #[test]
    fn identical_windows_report_nothing() {
        let w = vec![pc("a", "b", 80, 100, 0), pc("a", "c", 53, 50, 0)];
        let r = run(&w, &w);
        assert_eq!(r.counts.new_edges + r.counts.gone_edges, 0);
        assert!(r.drop_regressions.is_empty() && r.rate_changes.is_empty());
        assert_eq!((r.counts.baseline_edges, r.counts.current_edges), (2, 2));
    }

    #[test]
    fn new_and_gone_edges_are_found_with_their_traffic() {
        let b = vec![pc("a", "old", 80, 40, 0), pc("a", "b", 80, 100, 0)];
        let c = vec![pc("a", "b", 80, 100, 0), pc("a", "new", 22, 5, 15)];
        let r = run(&b, &c);
        assert_eq!(r.new_edges.len(), 1);
        assert_eq!(r.new_edges[0].edge.destination, "p/new");
        assert_eq!((r.new_edges[0].flows, r.new_edges[0].dropped), (20, 15));
        assert_eq!(r.new_edges[0].drop_ratio, Some(0.75));
        assert_eq!(r.gone_edges.len(), 1);
        assert_eq!(r.gone_edges[0].edge.destination, "p/old");
    }

    #[test]
    fn drop_regression_needs_a_real_rise_and_enough_drops() {
        let b = vec![
            pc("a", "b", 80, 100, 0),
            pc("a", "c", 80, 100, 0),
            pc("a", "d", 80, 100, 0),
        ];
        let c = vec![
            pc("a", "b", 80, 80, 20), // 0% -> 20%: regression
            pc("a", "c", 80, 98, 2),  // only 2 drops: noise
            pc("a", "d", 80, 99, 1),
        ];
        let r = run(&b, &c);
        assert_eq!(r.drop_regressions.len(), 1);
        let d = &r.drop_regressions[0];
        assert_eq!(d.edge.destination, "p/b");
        assert_eq!(d.baseline_drop_ratio, Some(0.0));
        assert_eq!(d.current_drop_ratio, 0.2);
        // Boundary: a 5 point rise with 5 drops qualifies; 5 drops among 201 flows is a 2.5 point rise and does not.
        let r = run(&[pc("a", "b", 1, 100, 0)], &[pc("a", "b", 1, 95, 5)]);
        assert_eq!(r.drop_regressions.len(), 1);
        let r = run(&[pc("a", "b", 1, 100, 0)], &[pc("a", "b", 1, 196, 5)]);
        assert!(r.drop_regressions.is_empty());
    }

    #[test]
    fn already_bad_edges_are_not_regressions() {
        let w = vec![pc("a", "b", 80, 50, 50)];
        assert!(run(&w, &w).drop_regressions.is_empty());
    }

    #[test]
    fn rates_compare_per_second_across_unequal_windows() {
        // Same flow count, but the current window is 4x shorter: 4x the rate.
        let b = vec![pc("a", "b", 80, 100, 0)];
        let c = vec![pc("a", "b", 80, 100, 0)];
        let r = diff(&b, &c, Granularity::Pod, 4000, 1000, 10);
        assert_eq!(r.rate_changes.len(), 1);
        assert_eq!(r.rate_changes[0].factor, 4.0);
        // A 2x change is below the threshold; a fall to a tenth is reported.
        let r = diff(
            &b,
            &[pc("a", "b", 80, 200, 0)],
            Granularity::Pod,
            1000,
            1000,
            10,
        );
        assert!(r.rate_changes.is_empty());
        let r = diff(
            &[pc("a", "b", 80, 1000, 0)],
            &b,
            Granularity::Pod,
            1000,
            1000,
            10,
        );
        assert_eq!(r.rate_changes[0].factor, 0.1);
    }

    #[test]
    fn tiny_samples_are_not_a_rate_change() {
        let r = run(&[pc("a", "b", 80, 2, 0)], &[pc("a", "b", 80, 19, 0)]);
        assert!(r.rate_changes.is_empty());
    }

    #[test]
    fn replicas_at_workload_granularity_do_not_look_new() {
        let b = vec![pc("web-7c9d8f6b5-x2k4p", "db-0", 5432, 50, 0)];
        let c = vec![pc("web-7c9d8f6b5-z9c4d", "db-0", 5432, 50, 0)];
        assert_eq!(run(&b, &c).new_edges.len(), 1); // pod level: a new pod
        let r = diff(&b, &c, Granularity::Workload, 3600, 3600, 10);
        assert_eq!(r.counts.new_edges + r.counts.gone_edges, 0);
    }

    #[test]
    fn scan_floods_are_capped_and_counted() {
        let b = vec![pc("a", "b", 80, 100, 0)];
        let mut c = vec![pc("a", "b", 80, 100, 0)];
        for i in 0..5000 {
            c.push(pc("scanner", &format!("t-{i}"), 22, 0, 1));
        }
        let r = diff(&b, &c, Granularity::Pod, 3600, 3600, 50);
        assert_eq!(r.counts.new_edges, 5000);
        assert_eq!(r.new_edges.len(), 50);
        assert_eq!(r.counts.omitted, 4950);
        // max_items is clamped, not trusted.
        assert_eq!(diff(&b, &c, Granularity::Pod, 1, 1, 0).new_edges.len(), 1);
        assert_eq!(
            diff(&b, &c, Granularity::Pod, 1, 1, usize::MAX)
                .new_edges
                .len(),
            1000
        );
    }

    #[test]
    fn empty_inputs_and_zero_windows_are_safe() {
        let r = diff(&[], &[], Granularity::Namespace, 0, -5, 10);
        assert_eq!(r.counts, Counts::default());
        let r = diff(
            &[],
            &[pc("a", "b", 1, 1, 0)],
            Granularity::Namespace,
            0,
            0,
            10,
        );
        assert!(r.new_edges[0].flows_per_sec.is_finite());
    }

    #[test]
    fn hostile_labels_are_bounded() {
        let long = "x".repeat(5000);
        let r = run(&[], &[pc(&long, "b\u{7}ell", 1, 1, 0)]);
        assert!(r.new_edges[0].edge.source.chars().count() <= 253 + 2);
        assert!(!r.new_edges[0].edge.destination.contains('\u{7}'));
    }
}
