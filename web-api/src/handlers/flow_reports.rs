// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// More views derived from stored flow history: drop report by cause, top
// talkers, policy-audit readiness and drop spikes. Reads of the flow store
// only; none touches Cilium or the cluster.

use super::flow_history::{
    auto_bucket, build_query, context, fill_buckets, on_pool, parse_range, scope_of, HistoryParams,
    BUCKETS, MAX_BUCKETS,
};
use super::track_request;
use crate::services::flow_agg::{Granularity, PairCount, MAX_GROUPS};
use crate::services::flow_store::Coverage;
use crate::services::{audit, drops, spikes, top};
use crate::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;
type Claims = Option<axum::Extension<crate::middleware::auth::Claims>>;

fn bad(msg: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": msg })))
}

#[derive(Debug, Deserialize, Default)]
pub struct DropParams {
    /// `namespace`, `workload` (default) or `pod`.
    pub granularity: Option<String>,
    /// Reasons listed, 1..=200 (default 20).
    pub max_reasons: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub struct TopParams {
    /// `namespace`, `workload` (default) or `pod`.
    pub granularity: Option<String>,
    /// Entries per ranking, 1..=100 (default 10).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub struct AuditParams {
    /// `namespace`, `workload` (default) or `pod`.
    pub granularity: Option<String>,
    /// Edges listed, 1..=1000 (default 50).
    pub max_items: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub struct SpikeParams {
    /// Scaled deviations above the median that make a bucket a spike, 1..=20 (default 3).
    pub sensitivity: Option<f64>,
    /// Drops a bucket needs at least, 1..=1000000 (default 10).
    pub min_drops: Option<u64>,
}

fn granularity(v: &Option<String>) -> Result<Granularity, (StatusCode, Json<Value>)> {
    match v.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(Granularity::Workload),
        Some(v) => Granularity::parse(v)
            .ok_or_else(|| bad("granularity must be one of: namespace, workload, pod")),
    }
}

fn merge(mut body: Value, report: &impl Serialize) -> Json<Value> {
    let report = serde_json::to_value(report).unwrap_or_default();
    if let (Some(b), Some(r)) = (body.as_object_mut(), report.as_object()) {
        b.extend(r.clone());
    }
    Json(body)
}

/// Read grouped pair counts for the requested range and filters. `verdict`
/// forces the verdict filter (the audit view needs `AUDIT` whatever the caller
/// passed).
async fn read_pairs(
    state: &Arc<AppState>,
    claims: &Claims,
    p: &HistoryParams,
    verdict: Option<&str>,
) -> Result<
    (
        crate::handlers::flow_history::Range,
        Vec<PairCount>,
        bool,
        Coverage,
    ),
    (StatusCode, Json<Value>),
> {
    let range = parse_range(p.from.as_deref(), p.to.as_deref(), Utc::now()).map_err(|m| bad(&m))?;
    let mut q = build_query(p, &range, scope_of(state, claims))?;
    if let Some(v) = verdict {
        q.verdict = Some(v.to_string());
    }
    let store = state.flow_store.clone();
    let ((pairs, truncated), coverage) = on_pool(move || {
        Ok((
            store.pair_counts(&q, MAX_GROUPS)?,
            store.coverage(q.scope.as_deref())?,
        ))
    })
    .await?;
    Ok((range, pairs, truncated, coverage))
}

/// GET /api/v1/flows/history/drops: why flows were dropped, by cause.
pub async fn drop_report(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Query(p): Query<HistoryParams>,
    Query(d): Query<DropParams>,
) -> ApiResult {
    track_request(&state, |_| {}).await;
    let g = granularity(&d.granularity)?;
    let max = d.max_reasons.unwrap_or(drops::DEFAULT_MAX_REASONS);
    if max == 0 || max > drops::MAX_MAX_REASONS {
        return Err(bad("max_reasons must be between 1 and 200"));
    }
    let (range, pairs, truncated, coverage) = read_pairs(&state, &claims, &p, None).await?;
    let report = drops::report(&pairs, g, max, truncated);
    Ok(merge(context(&state, &range, &coverage), &report))
}

/// GET /api/v1/flows/history/top: busiest sources, destinations and ports.
pub async fn top_talkers(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Query(p): Query<HistoryParams>,
    Query(t): Query<TopParams>,
) -> ApiResult {
    track_request(&state, |_| {}).await;
    let g = granularity(&t.granularity)?;
    let limit = t.limit.unwrap_or(top::DEFAULT_LIMIT);
    if limit == 0 || limit > top::MAX_LIMIT {
        return Err(bad("limit must be between 1 and 100"));
    }
    let (range, pairs, truncated, coverage) = read_pairs(&state, &claims, &p, None).await?;
    let report = top::report(&pairs, g, limit, truncated);
    Ok(merge(context(&state, &range, &coverage), &report))
}

/// GET /api/v1/flows/history/audit: edges Cilium's policy audit mode flagged
/// as would-be-denied. Any `verdict` filter is ignored: this view is `AUDIT`.
pub async fn policy_audit(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Query(p): Query<HistoryParams>,
    Query(a): Query<AuditParams>,
) -> ApiResult {
    track_request(&state, |_| {}).await;
    let g = granularity(&a.granularity)?;
    let max = a.max_items.unwrap_or(audit::DEFAULT_MAX_ITEMS);
    if max == 0 || max > audit::MAX_MAX_ITEMS {
        return Err(bad("max_items must be between 1 and 1000"));
    }
    let (range, pairs, truncated, coverage) =
        read_pairs(&state, &claims, &p, Some("AUDIT")).await?;
    let report = audit::report(&pairs, g, max, truncated);
    Ok(merge(context(&state, &range, &coverage), &report))
}

/// GET /api/v1/flows/history/spikes: episodes where drops jumped above the
/// window's own typical level. Notifies only.
pub async fn drop_spikes(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Query(p): Query<HistoryParams>,
    Query(s): Query<SpikeParams>,
) -> ApiResult {
    track_request(&state, |_| {}).await;
    let range = parse_range(p.from.as_deref(), p.to.as_deref(), Utc::now()).map_err(|m| bad(&m))?;
    let sensitivity = s.sensitivity.unwrap_or(spikes::DEFAULT_SENSITIVITY);
    if !sensitivity.is_finite()
        || !(spikes::MIN_SENSITIVITY..=spikes::MAX_SENSITIVITY).contains(&sensitivity)
    {
        return Err(bad("sensitivity must be between 1 and 20"));
    }
    let min_drops = s.min_drops.unwrap_or(spikes::DEFAULT_MIN_DROPS);
    if min_drops == 0 || min_drops > spikes::MAX_MIN_DROPS {
        return Err(bad("min_drops must be between 1 and 1000000"));
    }
    let range_secs = (range.to - range.from).num_seconds();
    let bucket = match p.bucket_secs {
        None => auto_bucket(range_secs),
        Some(b) if BUCKETS.contains(&b) => b,
        Some(_) => {
            return Err(bad(
                "bucket_secs must be one of 60, 300, 900, 3600, 21600, 86400",
            ))
        }
    };
    if range_secs / bucket > MAX_BUCKETS {
        return Err(bad(
            "too many buckets for this range: use a larger bucket_secs",
        ));
    }
    let q = build_query(&p, &range, scope_of(&state, &claims))?;
    let store = state.flow_store.clone();
    let (found, coverage) = on_pool(move || {
        Ok((
            store.timeline(&q, bucket)?,
            store.coverage(q.scope.as_deref())?,
        ))
    })
    .await?;
    let buckets = fill_buckets(&found, range.from.timestamp(), range.to.timestamp(), bucket);
    let report = spikes::detect(&buckets, bucket, sensitivity, min_drops);
    Ok(merge(context(&state, &range, &coverage), &report))
}
