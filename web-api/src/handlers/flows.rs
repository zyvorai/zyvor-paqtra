// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Flow monitoring endpoints
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use super::{flow_visible, to_json, track_error, track_request};
use crate::error::ApiError;
use crate::models::flow::{Flow, FlowQueryParams, FlowStats};
use crate::services::flow_store::FlowQuery;
use crate::AppState;

/// Cache key prefix for flow queries
const FLOWS_CACHE_PREFIX: &str = "flows";
/// Cache TTL for flow lists (seconds)
const FLOWS_CACHE_TTL: u64 = 10;
/// Maximum allowed limit to prevent excessively large responses
const MAX_LIMIT: usize = 1000;
/// Hard cap on live Hubble reads so the UI does not sit on a 15–60s hang.
const HUBBLE_FETCH_TIMEOUT: Duration = Duration::from_secs(12);

pub async fn list_flows(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Query(params): Query<FlowQueryParams>,
) -> Result<Json<Value>, ApiError> {
    tracing::info!("Fetching flows with params: {:?}", params);

    track_request(&state, |m| {
        m.hubble_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

    let limit = params.limit.unwrap_or(100).min(MAX_LIMIT);
    let offset = params.offset.unwrap_or(0);

    // Prefer the follow-ingest store: fast local SQLite, already fed by Hubble.
    // Always use spawn_blocking — rusqlite is sync and must not stall the runtime
    // (that previously made /ready miss and the pod flap NotReady).
    let store_stats = state.flow_store.stats();
    if store_stats.total > 0 || store_stats.stream_connected {
        let store = state.flow_store.clone();
        let namespace = params.namespace.clone();
        let verdict = params.verdict.clone();
        let q_limit = limit;
        let q_offset = offset;
        let unfiltered = namespace.is_none() && verdict.is_none();
        let indexed = store_stats.total;
        match tokio::task::spawn_blocking(move || {
            let q = FlowQuery {
                namespace,
                verdict,
                limit: q_limit,
                offset: q_offset,
                ..Default::default()
            };
            let rows = store.query(&q)?;
            // Full COUNT(*) over multi-million rows stalls every other request.
            // Unfiltered lists can use the indexed total; filtered lists report the page size.
            let total = if unfiltered {
                indexed
            } else {
                rows.len() as u64
            };
            Ok::<_, anyhow::Error>((rows, total))
        })
        .await
        {
            Ok(Ok((rows, total))) => {
                let mut flows: Vec<Flow> = rows.into_iter().map(|r| r.into_flow()).collect();
                flows.retain(|f| flow_visible(&state, &claims, f));
                state
                    .metrics
                    .flows_fetched
                    .fetch_add(flows.len() as u64, Ordering::Relaxed);
                return Ok(Json(json!({
                    "flows": flows,
                    "total": total,
                    "limit": limit,
                    "offset": offset,
                    "cached": false,
                    "source": "flow_store",
                })));
            }
            Ok(Err(e)) => {
                tracing::warn!("flow_store query failed, falling back to Hubble: {}", e);
            }
            Err(e) => {
                tracing::warn!("flow_store task join failed: {}", e);
            }
        }
    }

    let cache_key = format!(
        "{}:ns={}:v={}:l={}:o={}",
        FLOWS_CACHE_PREFIX,
        params.namespace.as_deref().unwrap_or("*"),
        params.verdict.as_deref().unwrap_or("*"),
        limit,
        offset
    );

    // Try cache first
    match state.cache.get::<Vec<Flow>>(&cache_key).await {
        Ok(Some(mut cached_flows)) => {
            tracing::debug!("Cache hit for flows (key={})", cache_key);
            state.metrics.cache_hits.fetch_add(1, Ordering::Relaxed);

            if let Some(ref verdict) = params.verdict {
                cached_flows.retain(|f| f.verdict.eq_ignore_ascii_case(verdict));
            }

            cached_flows.retain(|f| flow_visible(&state, &claims, f));

            state
                .metrics
                .flows_fetched
                .fetch_add(cached_flows.len() as u64, Ordering::Relaxed);

            let total = cached_flows.len();
            let page = apply_pagination(&cached_flows, offset, limit);

            return Ok(Json(json!({
                "flows": page,
                "total": total,
                "limit": limit,
                "offset": offset,
                "cached": true,
                "source": "cache",
            })));
        }
        Ok(None) => {
            tracing::info!(
                limit = limit,
                offset = offset,
                "Cache miss for flows, fetching from Hubble"
            );
            state.metrics.cache_misses.fetch_add(1, Ordering::Relaxed);
        }
        Err(e) => {
            tracing::warn!("Cache read error: {}", e);
        }
    }

    // Fetch from Hubble with a hard timeout so the UI does not hang.
    let mut flows = match tokio::time::timeout(
        HUBBLE_FETCH_TIMEOUT,
        state.hubble.get_flows_by_verdict(
            limit + offset,
            params.namespace.as_deref(),
            params.verdict.as_deref(),
        ),
    )
    .await
    {
        Ok(Ok(f)) => f,
        Ok(Err(e)) => {
            tracing::error!("Failed to fetch flows from Hubble: {}", e);
            track_error(&state).await;
            return Err(ApiError::InternalError(e.to_string()));
        }
        Err(_) => {
            tracing::error!("Hubble get_flows timed out after {:?}", HUBBLE_FETCH_TIMEOUT);
            track_error(&state).await;
            return Err(ApiError::InternalError(
                "timed out reading flows from Hubble".into(),
            ));
        }
    };

    if let Some(ref verdict) = params.verdict {
        flows.retain(|f| f.verdict.eq_ignore_ascii_case(verdict));
    }

    if let Err(e) = state.cache.set(&cache_key, &flows, FLOWS_CACHE_TTL).await {
        tracing::warn!("Cache write error: {}", e);
    }

    flows.retain(|f| flow_visible(&state, &claims, f));

    state
        .metrics
        .flows_fetched
        .fetch_add(flows.len() as u64, Ordering::Relaxed);

    let total = flows.len();
    let page = apply_pagination(&flows, offset, limit);

    Ok(Json(json!({
        "flows": page,
        "total": total,
        "limit": limit,
        "offset": offset,
        "cached": false,
        "source": "hubble",
    })))
}

pub async fn get_flow(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    tracing::info!("Fetching flow: {}", id);

    track_request(&state, |m| {
        m.hubble_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

    if let Ok(Some(row)) = state.flow_store.get_by_id(&id) {
        let flow = row.into_flow();
        return if flow_visible(&state, &claims, &flow) {
            Ok(Json(to_json(&flow)))
        } else {
            Err(ApiError::NotFound)
        };
    }

    let cache_key = format!("flow:{}", id);
    if let Ok(Some(flow)) = state.cache.get::<Flow>(&cache_key).await {
        state.metrics.cache_hits.fetch_add(1, Ordering::Relaxed);
        return if flow_visible(&state, &claims, &flow) {
            Ok(Json(to_json(&flow)))
        } else {
            Err(ApiError::NotFound)
        };
    }

    let flows = match tokio::time::timeout(HUBBLE_FETCH_TIMEOUT, state.hubble.get_flows(500, None))
        .await
    {
        Ok(Ok(f)) => f,
        Ok(Err(e)) => {
            tracing::error!("Failed to fetch flows from Hubble: {}", e);
            return Err(ApiError::InternalError(e.to_string()));
        }
        Err(_) => {
            return Err(ApiError::InternalError(
                "timed out reading flows from Hubble".into(),
            ));
        }
    };

    match flows.into_iter().find(|f| f.id == id) {
        Some(flow) => {
            let _ = state.cache.set(&cache_key, &flow, 30).await;
            if flow_visible(&state, &claims, &flow) {
                Ok(Json(to_json(&flow)))
            } else {
                Err(ApiError::NotFound)
            }
        }
        None => Err(ApiError::NotFound),
    }
}

pub async fn flow_stats(State(state): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    track_request(&state, |m| {
        m.hubble_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

    let cache_key = "flow_stats";
    if let Ok(Some(stats)) = state.cache.get::<FlowStats>(cache_key).await {
        state.metrics.cache_hits.fetch_add(1, Ordering::Relaxed);
        return Ok(Json(to_json(&stats)));
    }

    // Prefer local store: windowed sample + ingest rate (no Hubble round-trip).
    let store_stats = state.flow_store.stats();
    if store_stats.total > 0 || store_stats.stream_connected {
        let store = state.flow_store.clone();
        let recent = tokio::task::spawn_blocking(move || {
            store.query(&FlowQuery {
                limit: 1000,
                offset: 0,
                ..Default::default()
            })
        })
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or_default();
        let forwarded = recent
            .iter()
            .filter(|f| f.verdict.eq_ignore_ascii_case("FORWARDED"))
            .count() as u64;
        let dropped = recent
            .iter()
            .filter(|f| f.verdict.eq_ignore_ascii_case("DROPPED"))
            .count() as u64;
        let stats = FlowStats {
            total_flows: store_stats.total.max(recent.len() as u64),
            forwarded,
            dropped,
            requests_per_second: store_stats.events_per_sec,
            avg_latency_ms: 0.0,
        };
        let _ = state.cache.set(cache_key, &stats, 5).await;
        return Ok(Json(json!({
            "total_flows": stats.total_flows,
            "forwarded": stats.forwarded,
            "dropped": stats.dropped,
            "requests_per_second": stats.requests_per_second,
            "avg_latency_ms": stats.avg_latency_ms,
            "indexed": store_stats.total,
            "source": "flow_store",
            "window_sampled": recent.len(),
        })));
    }

    match tokio::time::timeout(HUBBLE_FETCH_TIMEOUT, state.hubble.get_flow_stats()).await {
        Ok(Ok(stats)) => {
            let _ = state.cache.set(cache_key, &stats, 5).await;
            Ok(Json(to_json(&stats)))
        }
        Ok(Err(e)) => {
            tracing::error!("Failed to compute flow stats: {}", e);
            track_error(&state).await;
            Err(ApiError::InternalError(e.to_string()))
        }
        Err(_) => {
            tracing::error!("Hubble get_flow_stats timed out");
            track_error(&state).await;
            Err(ApiError::InternalError(
                "timed out reading flows from Hubble".into(),
            ))
        }
    }
}

/// Apply offset/limit pagination to a slice
fn apply_pagination(flows: &[Flow], offset: usize, limit: usize) -> &[Flow] {
    let start = offset.min(flows.len());
    let end = (start + limit).min(flows.len());
    &flows[start..end]
}
