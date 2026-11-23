// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Views derived from stored flow history: service edges with golden signals,
// the Hubble metric cardinality advisor and an OpenTelemetry log export.
// All three are reads of the flow store; none touches Cilium or the cluster.

use super::flow_history::{build_query, context, on_pool, parse_range, scope_of, HistoryParams};
use super::track_request;
use crate::services::cardinality::{self, DEFAULT_BUDGET};
use crate::services::edges::{self, DEFAULT_MAX_EDGES};
use crate::services::flow_agg::{Granularity, MAX_GROUPS};
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
