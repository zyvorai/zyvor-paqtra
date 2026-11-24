// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Views derived from stored flow history: service edges with golden signals,
// the Hubble metric cardinality advisor and an OpenTelemetry log export.
// All three are reads of the flow store; none touches Cilium or the cluster.

use super::flow_history::{
    build_query, context, on_pool, parse_range, scope_of, HistoryParams, Range,
};
use super::track_request;
use crate::services::cardinality::{self, DEFAULT_BUDGET};
use crate::services::edges::{self, DEFAULT_MAX_EDGES};
use crate::services::fanout;
use crate::services::flow_agg::{Granularity, MAX_GROUPS};
use crate::services::flow_diff;
use crate::services::flow_store::format_ts;
use crate::services::otel_flow::{self, MAX_RECORDS};
use crate::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;
type Claims = Option<axum::Extension<crate::middleware::auth::Claims>>;

fn bad(msg: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": msg })))
}

#[derive(Debug, Deserialize, Default)]
pub struct EdgeParams {
    /// `namespace`, `workload` (default) or `pod`.
    pub granularity: Option<String>,
    /// Edges listed, 1..=1000 (default 200); the rest are folded into totals.
    pub max_edges: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub struct CardinalityParams {
    /// Series the Prometheus can afford for the flow and drop handlers together.
    pub budget: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct OtlpParams {
    /// Records returned, 1..=1000 (default 1000).
    pub limit: Option<usize>,
}

/// GET /api/v1/flows/history/edges: who talks to whom over the range, with
/// traffic, drop ratio and drop reasons per edge and per service.
pub async fn service_edges(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Query(p): Query<HistoryParams>,
    Query(e): Query<EdgeParams>,
) -> ApiResult {
    track_request(&state, |_| {}).await;
    let range = parse_range(p.from.as_deref(), p.to.as_deref(), Utc::now()).map_err(|m| bad(&m))?;
    let g = match e
        .granularity
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => Granularity::Workload,
        Some(v) => Granularity::parse(v)
            .ok_or_else(|| bad("granularity must be one of: namespace, workload, pod"))?,
    };
    let max_edges = e.max_edges.unwrap_or(DEFAULT_MAX_EDGES);
    if max_edges == 0 || max_edges > edges::MAX_MAX_EDGES {
        return Err(bad("max_edges must be between 1 and 1000"));
    }
    let q = build_query(&p, &range, scope_of(&state, &claims))?;
    let store = state.flow_store.clone();
    let ((pairs, truncated), coverage) = on_pool(move || {
        Ok((
            store.pair_counts(&q, MAX_GROUPS)?,
            store.coverage(q.scope.as_deref())?,
        ))
    })
    .await?;

    let range_secs = (range.to - range.from).num_seconds();
    let report = edges::build(&pairs, g, range_secs, max_edges, truncated);
    let mut body = context(&state, &range, &coverage);
    let report = serde_json::to_value(&report).unwrap_or_default();
    if let (Some(b), Some(r)) = (body.as_object_mut(), report.as_object()) {
        b.extend(r.clone());
    }
    Ok(Json(body))
}

/// GET /api/v1/flows/history/cardinality: how many Hubble `flow` and `drop`
/// series each `labelsContext` choice would have produced for this traffic.
pub async fn metric_cardinality(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Query(p): Query<HistoryParams>,
    Query(c): Query<CardinalityParams>,
) -> ApiResult {
    track_request(&state, |_| {}).await;
    let range = parse_range(p.from.as_deref(), p.to.as_deref(), Utc::now()).map_err(|m| bad(&m))?;
    let budget = c.budget.unwrap_or(DEFAULT_BUDGET);
    if !(cardinality::MIN_BUDGET..=cardinality::MAX_BUDGET).contains(&budget) {
        return Err(bad("budget must be between 100 and 10000000 series"));
    }
    let q = build_query(&p, &range, scope_of(&state, &claims))?;
    let store = state.flow_store.clone();
    let ((pairs, truncated), coverage) = on_pool(move || {
        Ok((
            store.pair_counts(&q, MAX_GROUPS)?,
            store.coverage(q.scope.as_deref())?,
        ))
    })
    .await?;

    let report = cardinality::advise(&pairs, budget, truncated);
    let mut body = context(&state, &range, &coverage);
    let report = serde_json::to_value(&report).unwrap_or_default();
    if let (Some(b), Some(r)) = (body.as_object_mut(), report.as_object()) {
        b.extend(r.clone());
    }
    Ok(Json(body))
}

#[derive(Debug, Deserialize, Default)]
pub struct DiffParams {
    /// Baseline start, RFC 3339. Give both baseline bounds or neither.
    pub baseline_from: Option<String>,
    /// Baseline end, exclusive, RFC 3339. Default baseline: the window of the
    /// same length that ends where the current one starts.
    pub baseline_to: Option<String>,
    /// `namespace`, `workload` (default) or `pod`.
    pub granularity: Option<String>,
    /// Entries per list, 1..=1000 (default 100).
    pub max_items: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub struct FanoutParams {
    /// `namespace`, `workload` or `pod` (default).
    pub granularity: Option<String>,
    /// Destinations, or ports on one destination, that make a source listed.
    pub min_targets: Option<usize>,
    /// Drop ratio (0..1) at which a listed source is a suspect.
    pub min_drop_ratio: Option<f64>,
    /// Sources listed, 1..=1000 (default 50).
    pub limit: Option<usize>,
}

fn granularity_param(
    v: &Option<String>,
    default: Granularity,
) -> Result<Granularity, (StatusCode, Json<Value>)> {
    match v.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(default),
        Some(v) => Granularity::parse(v)
            .ok_or_else(|| bad("granularity must be one of: namespace, workload, pod")),
    }
}

/// GET /api/v1/flows/history/diff: what changed between a baseline window and
/// the requested one: new and vanished edges, drop-ratio regressions and large
/// traffic-rate changes. Read only.
pub async fn flow_diff(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Query(p): Query<HistoryParams>,
    Query(d): Query<DiffParams>,
) -> ApiResult {
    track_request(&state, |_| {}).await;
    let now = Utc::now();
    let range = parse_range(p.from.as_deref(), p.to.as_deref(), now).map_err(|m| bad(&m))?;
    let g = granularity_param(&d.granularity, Granularity::Workload)?;
    let max_items = d.max_items.unwrap_or(flow_diff::DEFAULT_MAX_ITEMS);
    if max_items == 0 || max_items > flow_diff::MAX_MAX_ITEMS {
        return Err(bad("max_items must be between 1 and 1000"));
    }
    let given = |v: &Option<String>| v.as_deref().is_some_and(|s| !s.trim().is_empty());
    let base = match (given(&d.baseline_from), given(&d.baseline_to)) {
        (false, false) => Range {
            from: range.from - (range.to - range.from),
            to: range.from,
        },
        (true, true) => {
            let b = parse_range(d.baseline_from.as_deref(), d.baseline_to.as_deref(), now)
                .map_err(|m| bad(&format!("baseline: {m}")))?;
            if b.from < range.to && b.to > range.from {
                return Err(bad("the baseline must not overlap the compared range"));
            }
            b
        }
        _ => return Err(bad("give both baseline_from and baseline_to, or neither")),
    };

    let scope = scope_of(&state, &claims);
    let cq = build_query(&p, &range, scope.clone())?;
    let bq = build_query(&p, &base, scope)?;
    let store = state.flow_store.clone();
    let ((cur, cur_trunc), (old, old_trunc), coverage) = on_pool(move || {
        Ok((
            store.pair_counts(&cq, MAX_GROUPS)?,
            store.pair_counts(&bq, MAX_GROUPS)?,
            store.coverage(cq.scope.as_deref())?,
        ))
    })
    .await?;

    let report = flow_diff::diff(
        &old,
        &cur,
        g,
        (base.to - base.from).num_seconds(),
        (range.to - range.from).num_seconds(),
        max_items,
    );
    let base_from_ts = format_ts(base.from);
    let base_before_oldest = coverage
        .oldest
        .as_deref()
        .map(|o| base_from_ts.as_str() < o);
    let mut warnings: Vec<&str> = Vec::new();
    if base_before_oldest != Some(false) {
        warnings.push(
            "The baseline starts before the oldest stored flow (or the store is empty): edges \
             reported as new may only be missing from history.",
        );
    }
    if cur_trunc || old_trunc {
        warnings.push(
            "A window had more groups than were read, so its quietest edges were left out: new \
             and vanished edges at the bottom of the traffic ranking can be artefacts.",
        );
    }
    let mut body = context(&state, &range, &coverage);
    let report = serde_json::to_value(&report).unwrap_or_default();
    if let Some(b) = body.as_object_mut() {
        b.insert(
            "baseline".into(),
            json!({
                "from": base_from_ts,
                "to": format_ts(base.to),
                "starts_before_oldest": base_before_oldest,
                "truncated": old_trunc,
            }),
        );
        b.insert("current_truncated".into(), json!(cur_trunc));
        b.insert("warnings".into(), json!(warnings));
        if let Some(r) = report.as_object() {
            b.extend(r.clone());
        }
    }
    Ok(Json(body))
}

/// GET /api/v1/flows/history/fanout: sources that reached for many
/// destinations or many ports on one destination, flagged when mostly denied.
/// A scan / lateral-movement heuristic. Notifies only.
pub async fn flow_fanout(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Query(p): Query<HistoryParams>,
    Query(f): Query<FanoutParams>,
) -> ApiResult {
    track_request(&state, |_| {}).await;
    let range = parse_range(p.from.as_deref(), p.to.as_deref(), Utc::now()).map_err(|m| bad(&m))?;
    let g = granularity_param(&f.granularity, Granularity::Pod)?;
    let min_targets = f.min_targets.unwrap_or(fanout::DEFAULT_MIN_TARGETS);
    if !(fanout::MIN_MIN_TARGETS..=fanout::MAX_MIN_TARGETS).contains(&min_targets) {
        return Err(bad("min_targets must be between 2 and 100000"));
    }
    let min_ratio = f.min_drop_ratio.unwrap_or(fanout::DEFAULT_MIN_DROP_RATIO);
    if !(0.0..=1.0).contains(&min_ratio) {
        return Err(bad("min_drop_ratio must be between 0 and 1"));
    }
    let limit = f.limit.unwrap_or(fanout::DEFAULT_LIMIT);
    if limit == 0 || limit > fanout::MAX_LIMIT {
        return Err(bad("limit must be between 1 and 1000"));
    }
    let q = build_query(&p, &range, scope_of(&state, &claims))?;
    let store = state.flow_store.clone();
    let ((pairs, truncated), coverage) = on_pool(move || {
        Ok((
            store.pair_counts(&q, MAX_GROUPS)?,
            store.coverage(q.scope.as_deref())?,
        ))
    })
    .await?;

    let report = fanout::analyse(&pairs, g, min_targets, min_ratio, limit, truncated);
    let mut body = context(&state, &range, &coverage);
    let report = serde_json::to_value(&report).unwrap_or_default();
    if let (Some(b), Some(r)) = (body.as_object_mut(), report.as_object()) {
        b.extend(r.clone());
    }
    Ok(Json(body))
}

/// GET /api/v1/flows/history/otlp: stored flows as an OTLP/JSON logs document
/// (OpenTelemetry network attributes). Read only; Paqtra sends nothing.
pub async fn flows_otlp(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Query(p): Query<HistoryParams>,
    Query(o): Query<OtlpParams>,
) -> ApiResult {
    track_request(&state, |_| {}).await;
    let range = parse_range(p.from.as_deref(), p.to.as_deref(), Utc::now()).map_err(|m| bad(&m))?;
    let limit = o.limit.unwrap_or(MAX_RECORDS);
    if limit == 0 || limit > MAX_RECORDS {
        return Err(bad("limit must be between 1 and 1000"));
    }
    let mut q = build_query(&p, &range, scope_of(&state, &claims))?;
    q.limit = limit;
    let store = state.flow_store.clone();
    let flows = on_pool(move || store.query(&q)).await?;

    let (doc, skipped) = otel_flow::to_otlp_logs(&flows, env!("CARGO_PKG_VERSION"));
    Ok(Json(json!({
        "range": { "from": crate::services::flow_store::format_ts(range.from), "to": crate::services::flow_store::format_ts(range.to) },
        "records": flows.len() - skipped,
        "skipped_no_timestamp": skipped,
        "limit": limit,
        "otlp": doc,
    })))
}
