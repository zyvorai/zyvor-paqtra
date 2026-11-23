// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Hubble metric cardinality advisor.
//!
//! Hubble's `flow` and `drop` metric handlers add one Prometheus series per
//! distinct label combination, and `labelsContext` decides how many labels
//! carry the source and destination. Choosing `source_pod` or `source_ip` on a
//! busy cluster is the usual way to run Prometheus out of memory. This module
//! counts, from the flows Paqtra already stored, how many series each context
//! would have produced over the window, so the choice is made on the cluster's
//! own traffic instead of a guess.
//!
//! The counts are what was observed, not a promise: Hubble also splits the
//! `flow` metric by `type` and `subtype`, which flow records do not carry, so
//! the figures are a lower bound for the `flow` handler. Nothing here changes
//! Hubble or Cilium configuration; it only reads stored flows and prints a
//! suggestion.

use super::flow_agg::{endpoint_label, Granularity, PairCount};
use serde::Serialize;
use std::collections::HashSet;

pub const DEFAULT_BUDGET: u64 = 20_000;
pub const MIN_BUDGET: u64 = 100;
pub const MAX_BUDGET: u64 = 10_000_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Observed {
    pub namespaces: usize,
    pub workloads: usize,
    pub pods: usize,
    pub ports: usize,
    pub protocols: usize,
    pub drop_reasons: usize,
    pub flows: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ContextEstimate {
    /// `none`, `namespace`, `workload` or `pod`.
    pub context: &'static str,
    /// Value for the `labelsContext` option; empty for `none`.
    pub labels_context: &'static str,
    pub flow_series: u64,
    pub drop_series: u64,
    pub total_series: u64,
    /// `low` (under a quarter of the budget), `ok`, `high` (over budget) or
    /// `extreme` (over ten times the budget).
    pub risk: &'static str,
    pub fits_budget: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CardinalityReport {
    pub budget: u64,
    pub observed: Observed,
    pub contexts: Vec<ContextEstimate>,
    /// The most detailed context that fits the budget, if any.
    pub recommended: Option<&'static str>,
    pub recommended_labels_context: Option<&'static str>,
    /// Pod and IP contexts also depend on pod churn; see `notes`.
    pub notes: Vec<String>,
    pub truncated: bool,
}

const PRESETS: [(Granularity, &str, &str); 3] = [
    (
        Granularity::Namespace,
        "namespace",
        "source_namespace,destination_namespace",
    ),
    (
        Granularity::Workload,
        "workload",
        "source_namespace,source_workload,destination_namespace,destination_workload",
    ),
    (
        Granularity::Pod,
        "pod",
        "source_namespace,source_pod,destination_namespace,destination_pod",
    ),
];

fn risk(total: u64, budget: u64) -> &'static str {
    if total > budget.saturating_mul(10) {
        "extreme"
    } else if total > budget {
        "high"
    } else if total.saturating_mul(4) <= budget {
        "low"
    } else {
        "ok"
    }
}

/// Distinct series the `flow` and `drop` handlers would have produced.
fn count_series(pairs: &[PairCount], g: Option<Granularity>) -> (u64, u64) {
    let mut flow: HashSet<(String, String, &str, &str)> = HashSet::new();
    let mut drop: HashSet<(String, String, &str, &str)> = HashSet::new();
    for p in pairs {
        let (s, d) = match g {
            Some(g) => (
                endpoint_label(&p.src_namespace, &p.src_pod, g),
                endpoint_label(&p.dst_namespace, &p.dst_pod, g),
            ),
            None => (String::new(), String::new()),
        };
        let proto = p.protocol.as_str();
        for (n, verdict) in [
            (p.forwarded, "FORWARDED"),
            (p.dropped, "DROPPED"),
            (p.other, "OTHER"),
        ] {
            if n > 0 {
                flow.insert((s.clone(), d.clone(), proto, verdict));
            }
        }
        if p.dropped > 0 {
            drop.insert((s, d, proto, p.drop_reason.as_str()));
        }
    }
    (flow.len() as u64, drop.len() as u64)
}

/// Estimate series counts per `labelsContext` choice. `budget` is clamped to
/// `MIN_BUDGET..=MAX_BUDGET`.
pub fn advise(pairs: &[PairCount], budget: u64, truncated: bool) -> CardinalityReport {
    let budget = budget.clamp(MIN_BUDGET, MAX_BUDGET);

    let mut ns = HashSet::new();
    let mut workloads = HashSet::new();
    let mut pods = HashSet::new();
    let mut ports = HashSet::new();
    let mut protocols = HashSet::new();
    let mut reasons = HashSet::new();
    let mut flows = 0u64;
    for p in pairs {
        flows = flows.saturating_add(p.total());
        for (n, pod) in [
            (&p.src_namespace, &p.src_pod),
            (&p.dst_namespace, &p.dst_pod),
        ] {
            ns.insert(endpoint_label(n, pod, Granularity::Namespace));
            workloads.insert(endpoint_label(n, pod, Granularity::Workload));
            pods.insert(endpoint_label(n, pod, Granularity::Pod));
        }
        ports.insert(p.port);
        protocols.insert(p.protocol.as_str());
        if p.dropped > 0 {
            reasons.insert(p.drop_reason.as_str());
        }
    }

    let mut contexts = Vec::with_capacity(4);
    let mut push = |name: &'static str, labels: &'static str, g: Option<Granularity>| {
        let (flow_series, drop_series) = count_series(pairs, g);
        let total_series = flow_series.saturating_add(drop_series);
        contexts.push(ContextEstimate {
            context: name,
            labels_context: labels,
            flow_series,
            drop_series,
            total_series,
            risk: risk(total_series, budget),
            fits_budget: total_series <= budget,
        });
    };
    push("none", "", None);
    for (g, name, labels) in PRESETS {
        push(name, labels, Some(g));
    }

    let best = contexts
        .iter()
        .rev()
        .find(|c| c.fits_budget && !c.labels_context.is_empty());
    let recommended = best.map(|c| c.context);
    let recommended_labels_context = best.map(|c| c.labels_context);

    let mut notes = vec![
        "Counts are series the flow and drop handlers would have created for the traffic in the \
         window. The flow handler also splits by type and subtype, which flow records do not \
         carry, so its figure is a lower bound."
            .to_string(),
        "source_ip and destination_ip contexts have at least as many series as pod contexts \
         (every pod has an IP and IPs are reused across pod restarts), so they are not offered."
            .to_string(),
        "Hubble drops series tied to a deleted pod one minute after it is gone, so live pod-context \
         series follow the pods running now, not the pods seen across the whole window."
            .to_string(),
    ];
    if recommended.is_none() {
        notes.push(
            "Even the namespace context exceeds the budget: raise the budget or enable the handlers \
             without labelsContext."
                .to_string(),
        );
    }
    if truncated {
        notes.push(
            "The store held more distinct flow groups than were read; the quietest were left out, \
             so every figure is a lower bound."
                .to_string(),
        );
    }

    CardinalityReport {
        budget,
        observed: Observed {
            namespaces: ns.len(),
            workloads: workloads.len(),
            pods: pods.len(),
            ports: ports.len(),
            protocols: protocols.len(),
            drop_reasons: reasons.len(),
            flows,
        },
        contexts,
        recommended,
        recommended_labels_context,
        notes,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc(src: (&str, &str), dst: (&str, &str), fwd: u64, drop: u64, reason: &str) -> PairCount {
        PairCount {
            src_namespace: src.0.into(),
            src_pod: src.1.into(),
            dst_namespace: dst.0.into(),
            dst_pod: dst.1.into(),
            protocol: "TCP".into(),
            port: 80,
            drop_reason: reason.into(),
            forwarded: fwd,
            dropped: drop,
            other: 0,
        }
    }

    /// 3 replicas of web calling 2 replicas of db, one drop reason on one pair.
    fn fixture() -> Vec<PairCount> {
        let mut v = Vec::new();
        for w in ["x2k4p", "z9c4d", "m5n6p"] {
            for d in ["0", "1"] {
                v.push(pc(
                    ("prod", &format!("web-7c9d8f6b5-{w}")),
                    ("data", &format!("db-{d}")),
                    10,
                    0,
                    "",
                ));
            }
        }
        v.push(pc(
            ("prod", "web-7c9d8f6b5-x2k4p"),
            ("data", "db-0"),
            0,
            3,
            "Policy denied",
        ));
        v
    }

    fn ctx<'a>(r: &'a CardinalityReport, name: &str) -> &'a ContextEstimate {
        r.contexts.iter().find(|c| c.context == name).unwrap()
    }

    #[test]
    fn detail_grows_with_context_as_expected() {
        let r = advise(&fixture(), 1000, false);
        // none: (TCP,FORWARDED) and (TCP,DROPPED) / (TCP,Policy denied)
        assert_eq!(
            (ctx(&r, "none").flow_series, ctx(&r, "none").drop_series),
            (2, 1)
        );
        // namespace: prod->data forwarded + dropped / dropped
        assert_eq!(
            (
                ctx(&r, "namespace").flow_series,
                ctx(&r, "namespace").drop_series
            ),
            (2, 1)
        );
        // workload: web->db forwarded + dropped / dropped
        assert_eq!(
            (
                ctx(&r, "workload").flow_series,
                ctx(&r, "workload").drop_series
            ),
            (2, 1)
        );
        // pod: 6 forwarded pairs + 1 dropped pair / 1 dropped pair
        assert_eq!(
            (ctx(&r, "pod").flow_series, ctx(&r, "pod").drop_series),
            (7, 1)
        );
        assert_eq!(r.observed.pods, 5);
        assert_eq!(r.observed.workloads, 2);
        assert_eq!(r.observed.namespaces, 2);
        assert_eq!(r.observed.flows, 63);
        assert_eq!(r.observed.drop_reasons, 1);
        assert_eq!(r.recommended, Some("pod"));
    }

    #[test]
    fn recommendation_steps_down_to_fit_the_budget() {
        let r = advise(&fixture(), MIN_BUDGET, false);
        assert_eq!(r.recommended, Some("pod")); // 8 series fits 100
                                                // Generate a pod-pair explosion: 200 callers x 200 callees.
        let mut big = Vec::new();
        for s in 0..200 {
            for d in 0..200 {
                big.push(pc(
                    ("a", &format!("c{s}")),
                    ("b", &format!("s{d}")),
                    1,
                    0,
                    "",
                ));
            }
        }
        let r = advise(&big, 20_000, false);
        let pod = ctx(&r, "pod");
        assert_eq!(pod.flow_series, 40_000);
        assert!(!pod.fits_budget);
        assert_eq!(pod.risk, "high");
        // c1..: the names are not "generated" shapes, so workloads equal pods
        // here too; the namespace context is the one that fits.
        assert_eq!(r.recommended, Some("namespace"));
        assert_eq!(
            r.recommended_labels_context,
            Some("source_namespace,destination_namespace")
        );
    }

    #[test]
    fn nothing_fits_says_so() {
        let mut big = Vec::new();
        for s in 0..300 {
            let mut p = pc(("a", "c"), ("b", "s"), 1, 1, &format!("reason-{s}"));
            p.protocol = format!("P{s}");
            big.push(p);
        }
        let r = advise(&big, MIN_BUDGET, false);
        assert!(ctx(&r, "namespace").total_series > MIN_BUDGET);
        assert_eq!(r.recommended, None);
        assert!(r
            .notes
            .iter()
            .any(|n| n.contains("Even the namespace context")));
    }

    #[test]
    fn risk_bands() {
        assert_eq!(risk(10, 100), "low");
        assert_eq!(risk(25, 100), "low");
        assert_eq!(risk(26, 100), "ok");
        assert_eq!(risk(100, 100), "ok");
        assert_eq!(risk(101, 100), "high");
        assert_eq!(risk(1001, 100), "extreme");
        assert_eq!(risk(u64::MAX, u64::MAX), "low");
    }

    #[test]
    fn empty_input_and_hostile_budget() {
        let r = advise(&[], 0, false);
        assert_eq!(r.budget, MIN_BUDGET);
        assert_eq!(advise(&[], u64::MAX, false).budget, MAX_BUDGET);
        assert_eq!(r.observed.flows, 0);
        assert!(r.contexts.iter().all(|c| c.total_series == 0));
        // An empty store fits every context; the most detailed is suggested.
        assert_eq!(r.recommended, Some("pod"));
    }

    #[test]
    fn huge_label_sets_do_not_blow_up() {
        let big = "x".repeat(100_000);
        let p = pc((&big, &big), (&big, &big), 1, 1, &big);
        let r = advise(&[p], 100, false);
        assert_eq!(r.contexts.len(), 4);
        assert_eq!(r.observed.pods, 1);
    }

    #[test]
    fn truncated_input_is_flagged() {
        let r = advise(&fixture(), 1000, true);
        assert!(r.truncated);
        assert!(r.notes.iter().any(|n| n.contains("lower bound")));
    }
}
