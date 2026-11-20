// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Service level objectives.
//
// An SLO stores its definition (target and window); the current value is
// computed when it is read. The only signal available today is the latest
// sample of Hubble flows, so availability is measured over that sample and the
// error budget is *projected*: "if this error rate held for the whole window".
// Every evaluation says so in `measurement` and reports `sample_size`, so it is
// never mistaken for measured history. `no_data` is reported, not guessed, when
// the sample has no forwarded or dropped flows.

use crate::models::flow::Flow;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const SLOS_PREFIX: &str = "cv:slos:";
/// Metric supported today: forwarded / (forwarded + dropped).
pub const METRIC_AVAILABILITY: &str = "availability";
/// Budget consumption above this share of the budget marks an SLO at risk.
const AT_RISK_SHARE: f64 = 0.5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slo {
    pub id: String,
    pub name: String,
    /// Scope: flows to or from this namespace. None means all namespaces.
    pub namespace: Option<String>,
    /// Target availability in percent, exclusive of 0 and 100.
    pub target: f64,
    /// Objective window such as `30d`, `12h` or `2w`.
    pub window: String,
    pub created_at: String,
}

/// Minutes in a window string (`<n>h|d|w`), limited to 1 hour..=90 days.
pub fn window_minutes(window: &str) -> Option<u64> {
    let (num, unit) = window.split_at(window.len().checked_sub(1)?);
    let n: u64 = num.parse().ok()?;
    let per = match unit {
        "h" => 60,
        "d" => 60 * 24,
        "w" => 60 * 24 * 7,
        _ => return None,
    };
    let minutes = n.checked_mul(per)?;
    (60..=90 * 24 * 60).contains(&minutes).then_some(minutes)
}

pub fn valid_target(target: f64) -> bool {
    target.is_finite() && target > 0.0 && target < 100.0
}

fn round(v: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (v * f).round() / f
}

/// Evaluate an SLO against a sample of flows.
pub fn evaluate(slo: &Slo, flows: &[Flow]) -> Value {
    let in_scope = |f: &&Flow| match &slo.namespace {
        Some(ns) => f.source.namespace == *ns || f.destination.namespace == *ns,
        None => true,
    };
    let (mut good, mut bad) = (0u64, 0u64);
    for f in flows.iter().filter(in_scope) {
        match f.verdict.as_str() {
            "FORWARDED" => good += 1,
            "DROPPED" => bad += 1,
            _ => {}
        }
    }
    let sample = good + bad;
    let window_min = window_minutes(&slo.window).unwrap_or(0) as f64;
    let budget_total = window_min * (1.0 - slo.target / 100.0);

    let mut out = json!({
        "id": slo.id,
        "name": slo.name,
        "service": slo.namespace.clone().unwrap_or_else(|| "all namespaces".to_string()),
        "namespace": slo.namespace,
        "metric": METRIC_AVAILABILITY,
        "target": slo.target,
        "window": slo.window,
        "sample_size": sample,
        "budget_total": round(budget_total, 2),
        "created_at": slo.created_at,
    });

    if sample == 0 {
        out["status"] = json!("no_data");
        out["current"] = Value::Null;
        out["budget_remaining"] = Value::Null;
        out["measurement"] =
            json!("No forwarded or dropped flows in the latest sample for this scope");
        return out;
    }

    let current = good as f64 / sample as f64 * 100.0;
    let consumed = window_min * (bad as f64 / sample as f64);
    let status = if current < slo.target {
        "breached"
    } else if consumed > AT_RISK_SHARE * budget_total {
        "at_risk"
    } else {
        "met"
    };
    out["status"] = json!(status);
    out["current"] = json!(round(current, 3));
    out["budget_remaining"] = json!(round(budget_total - consumed, 2));
    out["measurement"] = json!(format!(
        "Projected from the latest {sample} flows (not {} of history): budget assumes this error rate holds for the whole window",
        slo.window
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::flow::FlowEndpoint;

    fn flow(verdict: &str, src_ns: &str, dst_ns: &str) -> Flow {
        let ep = |ns: &str| FlowEndpoint {
            namespace: ns.into(),
            pod: String::new(),
            ip: String::new(),
        };
        Flow {
            id: "f".into(),
            timestamp: String::new(),
            source: ep(src_ns),
            destination: ep(dst_ns),
            verdict: verdict.into(),
            protocol: "TCP".into(),
            port: 80,
            http_method: None,
            http_url: None,
            http_code: None,
            cluster: None,
            ..Default::default()
        }
    }

    fn slo(ns: Option<&str>, target: f64, window: &str) -> Slo {
        Slo {
            id: "slo-1".into(),
            name: "n".into(),
            namespace: ns.map(String::from),
            target,
            window: window.into(),
            created_at: String::new(),
        }
    }

    fn flows(good: usize, bad: usize) -> Vec<Flow> {
        let mut v = vec![flow("FORWARDED", "a", "b"); good];
        v.extend(vec![flow("DROPPED", "a", "b"); bad]);
        v
    }

    #[test]
    fn window_parsing() {
        assert_eq!(window_minutes("30d"), Some(43_200));
        assert_eq!(window_minutes("12h"), Some(720));
        assert_eq!(window_minutes("2w"), Some(20_160));
        for bad in ["", "d", "30", "30x", "-1d", "0h", "91d", "1.5d", "30dd"] {
            assert_eq!(window_minutes(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn targets_are_open_interval() {
        assert!(valid_target(99.9));
        for bad in [0.0, 100.0, -1.0, 100.5, f64::NAN, f64::INFINITY] {
            assert!(!valid_target(bad), "{bad}");
        }
    }

    #[test]
    fn met_at_risk_and_breached() {
        // 99.9% over 30d: budget = 43.2 minutes.
        let s = slo(None, 99.9, "30d");
        let met = evaluate(&s, &flows(10_000, 3)); // 0.03% errors: ~13 of 43 min
        assert_eq!(met["status"], "met");
        let at_risk = evaluate(&s, &flows(10_000, 6)); // 0.06% errors: ~26 of 43 min
        assert_eq!(at_risk["status"], "at_risk");
        let breached = evaluate(&s, &flows(99, 1)); // 1% errors
        assert_eq!(breached["status"], "breached");
        assert!(breached["budget_remaining"].as_f64().unwrap() < 0.0);
    }

    #[test]
    fn budget_is_in_minutes_and_matches_target() {
        let out = evaluate(&slo(None, 99.9, "30d"), &flows(1000, 0));
        assert_eq!(out["budget_total"], 43.2);
        assert_eq!(out["budget_remaining"], 43.2);
        assert_eq!(out["current"], 100.0);
        assert_eq!(out["status"], "met");
    }

    #[test]
    fn no_data_is_reported_not_guessed() {
        let out = evaluate(&slo(None, 99.0, "7d"), &[flow("AUDIT", "a", "b")]);
        assert_eq!(out["status"], "no_data");
        assert!(out["current"].is_null());
        assert!(out["budget_remaining"].is_null());
        assert_eq!(out["sample_size"], 0);
    }

    #[test]
    fn namespace_scope_matches_source_or_destination() {
        let all = vec![
            flow("FORWARDED", "shop", "db"),
            flow("DROPPED", "web", "shop"),
            flow("DROPPED", "other", "elsewhere"),
        ];
        let scoped = evaluate(&slo(Some("shop"), 90.0, "7d"), &all);
        assert_eq!(scoped["sample_size"], 2);
        assert_eq!(scoped["current"], 50.0);
        let unscoped = evaluate(&slo(None, 90.0, "7d"), &all);
        assert_eq!(unscoped["sample_size"], 3);
    }

    #[test]
    fn measurement_discloses_it_is_a_projection() {
        let out = evaluate(&slo(None, 99.0, "30d"), &flows(50, 0));
        let m = out["measurement"].as_str().unwrap();
        assert!(
            m.contains("Projected") && m.contains("50 flows") && m.contains("30d"),
            "{m}"
        );
    }
}
