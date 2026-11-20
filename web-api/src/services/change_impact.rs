// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Correlate a recorded network change with flow verdict shifts.
//!
//! Compares bounded before/after windows. Correlation is never labeled causation;
//! gaps or short coverage return `inconclusive`.

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

use crate::handlers::has_namespace_access;
use crate::services::change_tracker;
use crate::services::flow_store::{normalize_ts, FlowQuery, StoredFlow};
use crate::AppState;

#[derive(Debug, Clone)]
pub struct ImpactQuery {
    pub before: Duration,
    pub after: Duration,
    pub limit: usize,
    /// Optional filter: only analyze if change.type matches (case-insensitive contains).
    pub kind: Option<String>,
    /// Optional filter: only analyze if change.namespace matches.
    pub namespace: Option<String>,
}

impl Default for ImpactQuery {
    fn default() -> Self {
        Self {
            before: Duration::minutes(30),
            after: Duration::minutes(30),
            limit: 40,
            kind: None,
            namespace: None,
        }
    }
}

/// Parse `30m` / `1h` / `15m` style windows. Caps at 2 hours.
pub fn parse_window(raw: &str, default: Duration) -> Duration {
    let s = raw.trim().to_lowercase();
    if s.is_empty() {
        return default;
    }
    let (num, unit) = if let Some(n) = s.strip_suffix('h') {
        (n, 'h')
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 'm')
    } else {
        return default;
    };
    let n: i64 = num.parse().unwrap_or(0);
    if n <= 0 {
        return default;
    }
    let d = match unit {
        'h' => Duration::hours(n),
        _ => Duration::minutes(n),
    };
    d.min(Duration::hours(2))
}

fn pair_key(f: &StoredFlow) -> String {
    format!(
        "{}/{} -> {}/{}:{}",
        f.src_namespace, f.src_pod, f.dst_namespace, f.dst_pod, f.port
    )
}

fn summarize(flows: &[StoredFlow], limit: usize) -> Value {
    let mut forwarded = 0u64;
    let mut dropped = 0u64;
    let mut other = 0u64;
    let mut pairs: HashMap<String, (u64, u64)> = HashMap::new();
    let mut evidence: Vec<String> = Vec::new();

    for f in flows {
        let v = f.verdict.to_uppercase();
        if v.contains("DROP") || v == "DENIED" {
            dropped += 1;
            if evidence.len() < limit {
                evidence.push(f.id.clone());
            }
        } else if v.contains("FORWARD") || v == "ALLOWED" || v == "AUDIT" {
            forwarded += 1;
        } else {
            other += 1;
        }
        let e = pairs.entry(pair_key(f)).or_insert((0, 0));
        if v.contains("DROP") || v == "DENIED" {
            e.1 += 1;
        } else {
            e.0 += 1;
        }
    }

    let mut pair_rows: Vec<Value> = pairs
        .into_iter()
        .map(|(path, (fwd, drop))| {
            json!({
                "path": path,
                "forwarded": fwd,
                "dropped": drop,
            })
        })
        .collect();
    pair_rows.sort_by(|a, b| {
        let da = a.get("dropped").and_then(|v| v.as_u64()).unwrap_or(0);
        let db = b.get("dropped").and_then(|v| v.as_u64()).unwrap_or(0);
        db.cmp(&da)
    });
    pair_rows.truncate(limit);

    json!({
        "forwarded": forwarded,
        "dropped": dropped,
        "other": other,
        "total": flows.len(),
        "pairs": pair_rows,
        "evidence_flow_ids": evidence,
    })
}

fn newly_dropped_pairs(before: &Value, after: &Value) -> Vec<Value> {
    let before_pairs: HashMap<String, u64> = before
        .get("pairs")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let path = p.get("path")?.as_str()?.to_string();
            let dropped = p.get("dropped")?.as_u64()?;
            Some((path, dropped))
        })
        .collect();

    after
        .get("pairs")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let path = p.get("path")?.as_str()?.to_string();
            let dropped = p.get("dropped")?.as_u64()?;
            let prev = before_pairs.get(&path).copied().unwrap_or(0);
            if dropped > prev {
                Some(json!({
                    "path": path,
                    "dropped_before": prev,
                    "dropped_after": dropped,
                    "delta": dropped.saturating_sub(prev),
                }))
            } else {
                None
            }
        })
        .collect()
}

/// Analyze flow impact around a stored change id.
pub async fn analyze_change_impact(
    state: &AppState,
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
    change_id: &str,
    q: ImpactQuery,
) -> Result<Value, (axum::http::StatusCode, String)> {
    let change = change_tracker::get_change(state, change_id)
        .await
        .ok_or((axum::http::StatusCode::NOT_FOUND, "change not found".into()))?;

    if let Some(want) = q.kind.as_deref().filter(|s| !s.is_empty()) {
        let got = change
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !got.to_lowercase().contains(&want.to_lowercase()) {
            return Err((
                axum::http::StatusCode::NOT_FOUND,
                format!("change kind {got} does not match filter {want}"),
            ));
        }
    }
    if let Some(want_ns) = q.namespace.as_deref().filter(|s| !s.is_empty()) {
        let got = change
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if got != want_ns {
            return Err((
                axum::http::StatusCode::NOT_FOUND,
                format!("change namespace {got} does not match filter {want_ns}"),
            ));
        }
    }

    let ns = change
        .get("namespace")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !ns.is_empty() && !has_namespace_access(state, claims, ns) {
        return Err((
            axum::http::StatusCode::FORBIDDEN,
            "namespace not in scope".into(),
        ));
    }

    let ts_raw = change
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let pivot = DateTime::parse_from_rfc3339(ts_raw)
        .map(|t| t.with_timezone(&Utc))
        .or_else(|_| {
            // Some events omit timezone; try treating as UTC.
            DateTime::parse_from_str(ts_raw, "%Y-%m-%dT%H:%M:%SZ")
                .map(|t| t.with_timezone(&Utc))
        })
        .unwrap_or_else(|_| Utc::now());

    let before_start = pivot - q.before;
    let after_end = pivot + q.after;

    let scope = namespace_scope(claims);

    let ns_owned = ns.to_string();
    let before_since = normalize_ts(&before_start.to_rfc3339());
    let pivot_ts = normalize_ts(&pivot.to_rfc3339());
    let after_until = normalize_ts(&after_end.to_rfc3339());
    let store = state.flow_store.clone();
    let scope_q = scope.clone();

    let (before_flows, after_flows) = tokio::task::spawn_blocking(move || {
        let before_flows = store
            .query(&FlowQuery {
                namespace: if ns_owned.is_empty() {
                    None
                } else {
                    Some(ns_owned.clone())
                },
                since_rfc3339: before_since,
                until_rfc3339: pivot_ts.clone(),
                scope: scope_q.clone(),
                limit: 1000,
                ..Default::default()
            })
            .unwrap_or_default();
        let after_flows = store
            .query(&FlowQuery {
                namespace: if ns_owned.is_empty() {
                    None
                } else {
                    Some(ns_owned)
                },
                since_rfc3339: pivot_ts,
                until_rfc3339: after_until,
                scope: scope_q,
                limit: 1000,
                ..Default::default()
            })
            .unwrap_or_default();
        (before_flows, after_flows)
    })
    .await
    .unwrap_or_default();

    // Drop evidence the caller may not see (defense in depth beyond SQL scope).
    let visible = |f: &StoredFlow| {
        has_namespace_access(state, claims, &f.src_namespace)
            || has_namespace_access(state, claims, &f.dst_namespace)
    };
    let before_flows: Vec<_> = before_flows.into_iter().filter(visible).collect();
    let after_flows: Vec<_> = after_flows.into_iter().filter(visible).collect();

    let before_sum = summarize(&before_flows, q.limit);
    let after_sum = summarize(&after_flows, q.limit);
    let regressions = newly_dropped_pairs(&before_sum, &after_sum);

    let stats = state.flow_store.stats();
    let coverage = state.flow_store.coverage(scope.as_deref()).ok();
    let mut reasons = Vec::new();
    if stats.gap_count > 0 {
        reasons.push(format!(
            "ingest reported {} gap(s); comparison may miss flows",
            stats.gap_count
        ));
    }
    if !stats.stream_connected && !stats.last_ingest_ok {
        reasons.push("flow ingest unhealthy — capture coverage incomplete".into());
    }
    if before_flows.is_empty() && after_flows.is_empty() {
        reasons.push("no flows in either window".into());
    }
    if q.before < Duration::minutes(5) || q.after < Duration::minutes(5) {
        reasons.push("windows shorter than 5m reduce confidence".into());
    }

    let confidence = if !reasons.is_empty() {
        "unavailable"
    } else if regressions.is_empty() {
        "observed"
    } else {
        "inferred"
    };

    let status = if !reasons.is_empty() {
        "inconclusive"
    } else if !regressions.is_empty() {
        "regression_observed"
    } else {
        "no_regression"
    };

    let mut evidence_ids: HashSet<String> = HashSet::new();
    for id in after_sum
        .get("evidence_flow_ids")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
    {
        evidence_ids.insert(id.to_string());
    }

    Ok(json!({
        "change": {
            "id": change.get("id"),
            "type": change.get("type"),
            "resource": change.get("resource"),
            "namespace": change.get("namespace"),
            "timestamp": change.get("timestamp"),
            "reason": change.get("reason"),
            "message": change.get("message"),
        },
        "windows": {
            "before": format!("{}s", q.before.num_seconds()),
            "after": format!("{}s", q.after.num_seconds()),
            "pivot": pivot.to_rfc3339(),
            "before_start": before_start.to_rfc3339(),
            "after_end": after_end.to_rfc3339(),
        },
        "before": before_sum,
        "after": after_sum,
        "chart": {
            "labels": ["before", "after"],
            "forwarded": [
                before_sum.get("forwarded").and_then(|v| v.as_u64()).unwrap_or(0),
                after_sum.get("forwarded").and_then(|v| v.as_u64()).unwrap_or(0),
            ],
            "dropped": [
                before_sum.get("dropped").and_then(|v| v.as_u64()).unwrap_or(0),
                after_sum.get("dropped").and_then(|v| v.as_u64()).unwrap_or(0),
            ],
        },
        "flows_link": "/flows",
        "investigate_link": "/investigate",
        "regressions": regressions,
        "evidence_flow_ids": evidence_ids.into_iter().collect::<Vec<_>>(),
        "capture": {
            "stream_connected": stats.stream_connected,
            "gap_count": stats.gap_count,
            "last_gap_at": stats.last_gap_at,
            "ingest_source": stats.ingest_source,
            "coverage": coverage,
        },
        "status": status,
        "confidence": confidence,
        "notes": reasons,
        "disclaimer": "Correlation is not causation — treat as evidence-backed hints only.",
    }))
}

fn namespace_scope(
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Option<Vec<String>> {
    match claims.as_ref() {
        Some(c) if c.role == "admin" || c.namespaces.is_empty() => None,
        Some(c) => Some(c.namespaces.clone()),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_window_caps_and_defaults() {
        assert_eq!(parse_window("30m", Duration::minutes(10)), Duration::minutes(30));
        assert_eq!(parse_window("1h", Duration::minutes(10)), Duration::hours(1));
        assert_eq!(parse_window("9h", Duration::minutes(10)), Duration::hours(2));
        assert_eq!(parse_window("", Duration::minutes(15)), Duration::minutes(15));
    }
}
