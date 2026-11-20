// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Health / readiness — keep probes cheap so kubelet never kills the API under load.
//!
//! `/ready` never calls Hubble or kubectl. `/health` serves last-known subsystem
//! status refreshed by a background sampler (see `spawn_health_sampler`).

use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::timeout;

use crate::AppState;

const SAMPLE_TIMEOUT: Duration = Duration::from_secs(2);
const SAMPLE_INTERVAL: Duration = Duration::from_secs(15);

static START_TIME: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
static HUBBLE_OK: AtomicBool = AtomicBool::new(false);
static K8S_OK: AtomicBool = AtomicBool::new(false);
static LAST_SAMPLE_UNIX: AtomicU64 = AtomicU64::new(0);

fn process_start() -> Instant {
    *START_TIME.get_or_init(Instant::now)
}

/// Background sampler so `/health` never blocks on live Hubble/kubectl.
pub fn spawn_health_sampler(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SAMPLE_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let (hubble_res, k8s_res) = tokio::join!(
                timeout(SAMPLE_TIMEOUT, state.hubble.is_healthy()),
                timeout(SAMPLE_TIMEOUT, state.k8s.is_healthy()),
            );
            HUBBLE_OK.store(hubble_res.unwrap_or(false), Ordering::Relaxed);
            K8S_OK.store(k8s_res.unwrap_or(false), Ordering::Relaxed);
            LAST_SAMPLE_UNIX.store(
                chrono::Utc::now().timestamp().max(0) as u64,
                Ordering::Relaxed,
            );
        }
    });
}

pub async fn health_check(State(state): State<Arc<AppState>>) -> (StatusCode, Json<Value>) {
    let uptime = process_start().elapsed();
    let hubble_ok = HUBBLE_OK.load(Ordering::Relaxed);
    let k8s_ok = K8S_OK.load(Ordering::Relaxed);
    let overall = if hubble_ok && k8s_ok {
        "healthy"
    } else {
        "degraded"
    };

    let flow_stats = state.flow_store.stats();
    let recent_gaps = state.flow_store.recent_gaps();

    (
        StatusCode::OK,
        Json(json!({
            "status": overall,
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_secs": uptime.as_secs(),
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "probe_cached": true,
            "probe_sampled_at_unix": LAST_SAMPLE_UNIX.load(Ordering::Relaxed),
            "subsystems": {
                "cache": "ok",
                "hubble_relay": if hubble_ok { "ok" } else { "unavailable" },
                "kubernetes": if k8s_ok { "ok" } else { "unavailable" },
                "flow_ingest": {
                    "status": if flow_stats.last_ingest_ok { "ok" } else { "unavailable" },
                    "source": flow_stats.ingest_source,
                    "indexed": flow_stats.total,
                    "last_at": flow_stats.last_ingest_at,
                    "hubble_mode": match state.hubble.mode() {
                        crate::config::HubbleMode::Auto => "auto",
                        crate::config::HubbleMode::Grpc => "grpc",
                        crate::config::HubbleMode::Cli => "cli",
                    },
                    "connected": flow_stats.stream_connected,
                    "disconnects": flow_stats.disconnect_count,
                    "gaps": flow_stats.gap_count,
                    "last_gap_at": flow_stats.last_gap_at,
                    "events_per_sec": flow_stats.events_per_sec,
                    "lag_secs": flow_stats.lag_secs,
                    "recent_gaps": recent_gaps,
                    "retention_days": state.flow_store.retention_days(),
                },
            }
        })),
    )
}

/// Liveness/readiness for kubelet — must stay under probe timeout even when
/// Hubble, kubectl, or SQLite are saturated.
pub async fn readiness_check(State(state): State<Arc<AppState>>) -> (StatusCode, Json<Value>) {
    let _ = state;
    let hubble_ok = HUBBLE_OK.load(Ordering::Relaxed);
    let k8s_ok = K8S_OK.load(Ordering::Relaxed);
    let overall = if hubble_ok && k8s_ok {
        "ready"
    } else {
        "degraded"
    };

    (
        StatusCode::OK,
        Json(json!({
            "status": overall,
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_secs": process_start().elapsed().as_secs(),
            "checks": {
                "process": "ok",
                "cache": "ok",
                "hubble": if hubble_ok { "ok" } else { "unavailable" },
                "kubernetes": if k8s_ok { "ok" } else { "unavailable" },
            },
            "probe_cached": true,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        })),
    )
}
