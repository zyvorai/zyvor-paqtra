// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Shared building blocks for views derived from stored flows: the grouped row
//! the flow store returns, and the endpoint labelling both the service-edge
//! view and the metric-cardinality advisor use.
//!
//! Everything here is pure. Flow records come from the cluster, so every
//! string is treated as hostile: bounded in length, never trusted to be a
//! well-formed Kubernetes name.

use super::flow_store::StoredFlow;
use std::collections::HashMap;

/// Longest label kept, in characters (a Kubernetes name is at most 253).
pub const MAX_LABEL_CHARS: usize = 253;
/// Most grouped rows one request reads from the store. Rows are ordered by
/// flow count, so what is cut off is the long tail (scans, one-off peers).
pub const MAX_GROUPS: usize = 50_000;
/// Label for endpoints Hubble reports without a namespace or pod (world, host,
/// kube-apiserver and other reserved identities). Their IPs are not kept: a
/// scan would otherwise make every address its own node.
pub const EXTERNAL: &str = "external";

/// Flows between one pair of endpoints on one protocol and port, with the
/// reason when they were dropped. Counts are flows in the stored sample.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PairCount {
    pub src_namespace: String,
    pub src_pod: String,
    pub dst_namespace: String,
    pub dst_pod: String,
    pub protocol: String,
    pub port: u16,
    /// Set only on rows that count dropped flows.
    pub drop_reason: String,
    pub forwarded: u64,
    pub dropped: u64,
    pub other: u64,
}

impl PairCount {
    pub fn total(&self) -> u64 {
        self.forwarded
            .saturating_add(self.dropped)
            .saturating_add(self.other)
    }
}

/// How finely endpoints are told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Namespace,
    Workload,
    Pod,
}

impl Granularity {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "namespace" => Some(Self::Namespace),
            "workload" => Some(Self::Workload),
            "pod" => Some(Self::Pod),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Namespace => "namespace",
            Self::Workload => "workload",
            Self::Pod => "pod",
        }
    }
}

/// Characters Kubernetes uses for generated name suffixes (`generateName`
/// and `pod-template-hash`): no vowels and no easily confused digits.
fn is_generated(seg: &str, lens: std::ops::RangeInclusive<usize>) -> bool {
    lens.contains(&seg.len())
        && seg
            .chars()
            .all(|c| "bcdfghjklmnpqrstvwxz2456789".contains(c))
}

/// Best-effort owner name of a pod: strips the ReplicaSet hash and pod suffix
/// of a Deployment pod (`web-7c9d8f6b5-x2k4p` is `web`), the suffix of a
/// DaemonSet or Job pod, and the ordinal of a StatefulSet pod (`db-0` is
/// `db`). A name that does not look generated is returned unchanged, so a
/// wrong guess merges nothing that was not already similar. Empty in, empty out.
pub fn workload_of(pod: &str) -> String {
    let pod = truncate(pod);
    let mut segs: Vec<&str> = pod.split('-').collect();
    if segs.len() < 2 {
        return pod;
    }
    let last = segs[segs.len() - 1];
    if !last.is_empty() && last.chars().all(|c| c.is_ascii_digit()) {
        segs.pop();
    } else if is_generated(last, 5..=5) {
        segs.pop();
        if segs.len() >= 2 && is_generated(segs[segs.len() - 1], 8..=10) {
            segs.pop();
        }
    }
    let name = segs.join("-");
    if name.is_empty() {
        pod
    } else {
        name
    }
}

/// Cut to [`MAX_LABEL_CHARS`] characters and drop control characters.
pub fn truncate(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(MAX_LABEL_CHARS)
        .collect()
}

/// The node an endpoint becomes at a granularity.
pub fn endpoint_label(namespace: &str, pod: &str, g: Granularity) -> String {
    let ns = truncate(namespace);
    if ns.is_empty() && pod.is_empty() {
        return EXTERNAL.to_string();
    }
    match g {
        Granularity::Namespace => {
            if ns.is_empty() {
                EXTERNAL.to_string()
            } else {
                ns
            }
        }
        Granularity::Workload | Granularity::Pod => {
            let name = if pod.is_empty() {
                "(no pod)".to_string()
            } else if g == Granularity::Workload {
                workload_of(pod)
            } else {
                truncate(pod)
            };
            if ns.is_empty() {
                name
            } else {
                format!("{ns}/{name}")
            }
        }
    }
}

/// Group stored flows the way the store does in SQL; used by the in-memory
/// store and by tests. Ordered by flow count, largest first, then by name so
/// the result is stable.
pub fn group_stored<'a>(flows: impl Iterator<Item = &'a StoredFlow>) -> Vec<PairCount> {
    type Key = (String, String, String, String, String, u16, String);
    let mut map: HashMap<Key, PairCount> = HashMap::new();
    for f in flows {
        let reason = if f.verdict == "DROPPED" {
            f.drop_reason.clone()
        } else {
            String::new()
        };
        let key = (
            f.src_namespace.clone(),
            f.src_pod.clone(),
            f.dst_namespace.clone(),
            f.dst_pod.clone(),
            f.protocol.clone(),
            f.port,
            reason.clone(),
        );
        let row = map.entry(key).or_insert_with(|| PairCount {
            src_namespace: f.src_namespace.clone(),
            src_pod: f.src_pod.clone(),
            dst_namespace: f.dst_namespace.clone(),
            dst_pod: f.dst_pod.clone(),
            protocol: f.protocol.clone(),
            port: f.port,
            drop_reason: reason,
            ..Default::default()
        });
        match f.verdict.as_str() {
            "FORWARDED" => row.forwarded += 1,
            "DROPPED" => row.dropped += 1,
            _ => row.other += 1,
        }
    }
    let mut rows: Vec<PairCount> = map.into_values().collect();
    sort_pairs(&mut rows);
    rows
}

pub fn sort_pairs(rows: &mut [PairCount]) {
    rows.sort_by(|a, b| {
        b.total().cmp(&a.total()).then_with(|| {
            (
                &a.src_namespace,
                &a.src_pod,
                &a.dst_namespace,
                &a.dst_pod,
                a.port,
                &a.drop_reason,
            )
                .cmp(&(
                    &b.src_namespace,
                    &b.src_pod,
                    &b.dst_namespace,
                    &b.dst_pod,
                    b.port,
                    &b.drop_reason,
                ))
        })
    });
}

/// Round to `places` decimals.
pub fn round(v: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (v * f).round() / f
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    /// A stored flow for tests.
    pub fn flow(
        verdict: &str,
        src: (&str, &str),
        dst: (&str, &str),
        port: u16,
        reason: &str,
    ) -> StoredFlow {
        StoredFlow {
            id: format!("{verdict}-{}-{}-{port}", src.1, dst.1),
            ts: "2026-11-01T00:00:00.000000Z".into(),
            cluster: "local".into(),
            verdict: verdict.into(),
            drop_reason: reason.into(),
            protocol: "TCP".into(),
            port,
            src_namespace: src.0.into(),
            src_pod: src.1.into(),
            src_ip: "10.0.0.1".into(),
            src_identity: 0,
            dst_namespace: dst.0.into(),
            dst_pod: dst.1.into(),
            dst_ip: "10.0.0.2".into(),
            dst_identity: 0,
            source: super::super::flow_store::FlowSource::HubbleGrpc,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::flow;
    use super::*;

    #[test]
    fn workload_names() {
        for (pod, want) in [
            ("web-7c9d8f6b5-x2k4p", "web"),
            ("my-app-backend-5d8f9c7b6d-9xzkq", "my-app-backend"),
            ("db-0", "db"),
            ("db-12", "db"),
            ("cilium-x2k4p", "cilium"),
            ("standalone", "standalone"),
            ("", ""),
            // Vowels mean it was not generated: left alone.
            ("my-service-alpha", "my-service-alpha"),
            ("-", "-"),
            ("0", "0"),
            ("-0", "-0"),
        ] {
            assert_eq!(workload_of(pod), want, "{pod:?}");
        }
    }

    #[test]
    fn hostile_names_are_bounded() {
        let long = "a".repeat(10_000);
        assert_eq!(workload_of(&long).chars().count(), MAX_LABEL_CHARS);
        let ctrl = "web\n\u{1b}[31m-7c9d8f6b5-x2k4p";
        assert!(!workload_of(ctrl).chars().any(char::is_control));
        // Multi-byte input must never be split inside a character.
        let wide = "é".repeat(1000);
        assert_eq!(
            endpoint_label("ns", &wide, Granularity::Pod)
                .chars()
                .count(),
            3 + MAX_LABEL_CHARS
        );
    }

    #[test]
    fn endpoint_labels() {
        assert_eq!(endpoint_label("", "", Granularity::Pod), EXTERNAL);
        assert_eq!(endpoint_label("", "", Granularity::Namespace), EXTERNAL);
        assert_eq!(
            endpoint_label("prod", "web-7c9d8f6b5-x2k4p", Granularity::Namespace),
            "prod"
        );
        assert_eq!(
            endpoint_label("prod", "web-7c9d8f6b5-x2k4p", Granularity::Workload),
            "prod/web"
        );
        assert_eq!(
            endpoint_label("prod", "web-7c9d8f6b5-x2k4p", Granularity::Pod),
            "prod/web-7c9d8f6b5-x2k4p"
        );
        assert_eq!(
            endpoint_label("prod", "", Granularity::Workload),
            "prod/(no pod)"
        );
        assert_eq!(
            endpoint_label("", "host-pod", Granularity::Workload),
            "host-pod"
        );
    }

    #[test]
    fn granularity_parse_roundtrip() {
        for g in [
            Granularity::Namespace,
            Granularity::Workload,
            Granularity::Pod,
        ] {
            assert_eq!(Granularity::parse(g.as_str()), Some(g));
        }
        assert_eq!(Granularity::parse("ip"), None);
        assert_eq!(Granularity::parse(""), None);
    }

    #[test]
    fn grouping_splits_drop_reasons_and_counts_verdicts() {
        let flows = [
            flow("FORWARDED", ("a", "x"), ("b", "y"), 80, ""),
            flow("FORWARDED", ("a", "x"), ("b", "y"), 80, ""),
            flow("DROPPED", ("a", "x"), ("b", "y"), 80, "Policy denied"),
            flow("DROPPED", ("a", "x"), ("b", "y"), 80, "Policy denied"),
            flow("DROPPED", ("a", "x"), ("b", "y"), 80, "TTL exceeded"),
            flow("AUDIT", ("a", "x"), ("b", "y"), 80, "ignored reason"),
        ];
        let g = group_stored(flows.iter());
        // forwarded+audit share the empty-reason row; drops split by reason.
        assert_eq!(g.len(), 3);
        let plain = g.iter().find(|r| r.drop_reason.is_empty()).unwrap();
        assert_eq!((plain.forwarded, plain.dropped, plain.other), (2, 0, 1));
        let denied = g.iter().find(|r| r.drop_reason == "Policy denied").unwrap();
        assert_eq!(denied.dropped, 2);
        // Largest first.
        assert!(g[0].total() >= g[1].total() && g[1].total() >= g[2].total());
    }
}
