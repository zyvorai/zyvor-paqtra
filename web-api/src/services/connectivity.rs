// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Declared connectivity path monitoring (observe-only).
//!
//! Teams declare `src ns/workload -> dst service:port/protocol`. Paqtra compares
//! recent observed flows with a short baseline. Quiet traffic is `unknown`, not
//! healthy. Sustained new drops become deduplicated alerts. Never applies policy
//! or mutates BPF.

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::handlers::has_namespace_access;
use crate::services::flow_store::{normalize_ts, FlowQuery};
use crate::AppState;

const PATHS_PREFIX: &str = "cv:connectivity_path:";
const ALERTS_PREFIX: &str = "cv:connectivity_alert:";
const SILENCE_PREFIX: &str = "cv:connectivity_silence:";
const SAMPLE_PREFIX: &str = "cv:connectivity_sample:";
const PATHS_TTL: u64 = 2_592_000; // 30 days
const ALERT_TTL: u64 = 604_800; // 7 days
const SUSTAIN_MINUTES: i64 = 5;
const SUSTAIN_SAMPLES: usize = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectivityPath {
    pub id: String,
    pub name: String,
    pub src_namespace: String,
    pub src_workload: String,
    pub dst_namespace: String,
    pub dst_service: String,
    pub port: u16,
    pub protocol: String,
    pub created_at: String,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatePathRequest {
    pub name: String,
    pub src_namespace: String,
    pub src_workload: String,
    pub dst_namespace: String,
    pub dst_service: String,
    pub port: u16,
    #[serde(default = "default_proto")]
    pub protocol: String,
}

fn default_proto() -> String {
    "TCP".into()
}

pub fn spawn_connectivity_monitor(state: Arc<AppState>) {
    tokio::spawn(async move {
        // Delay first evaluation; skip missed ticks under load.
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(120));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(e) = evaluate_all(&state).await {
                tracing::debug!("connectivity monitor cycle failed: {e}");
            }
        }
    });
}

pub async fn list_paths(
    state: &AppState,
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Vec<Value> {
    let mut out = Vec::new();
    for p in state
        .cache
        .list_values(PATHS_PREFIX)
        .await
        .unwrap_or_default()
    {
        let src = p
            .get("src_namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let dst = p
            .get("dst_namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if has_namespace_access(state, claims, src) || has_namespace_access(state, claims, dst) {
            out.push(p);
        }
    }
    out.sort_by(|a, b| {
        let ta = a.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
        let tb = b.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    out
}

pub async fn create_path(
    state: &AppState,
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
    req: CreatePathRequest,
) -> Result<ConnectivityPath, (axum::http::StatusCode, String)> {
    if req.src_namespace.is_empty() || req.dst_namespace.is_empty() || req.port == 0 {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "src_namespace, dst_namespace, and port are required".into(),
        ));
    }
    if !has_namespace_access(state, claims, &req.src_namespace)
        || !has_namespace_access(state, claims, &req.dst_namespace)
    {
        return Err((
            axum::http::StatusCode::FORBIDDEN,
            "namespace not in scope".into(),
        ));
    }
    let path = ConnectivityPath {
        id: format!("conn-{}", &Uuid::new_v4().to_string()[..8]),
        name: req.name,
        src_namespace: req.src_namespace,
        src_workload: req.src_workload,
        dst_namespace: req.dst_namespace,
        dst_service: req.dst_service,
        port: req.port,
        protocol: req.protocol,
        created_at: Utc::now().to_rfc3339(),
        enabled: true,
    };
    let key = format!("{}{}", PATHS_PREFIX, path.id);
    let val = serde_json::to_value(&path).unwrap_or(json!({}));
    state
        .cache
        .set_durable(&key, &val, PATHS_TTL)
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                e.to_string(),
            )
        })?;
    Ok(path)
}

pub async fn delete_path(
    state: &AppState,
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
    id: &str,
) -> Result<(), (axum::http::StatusCode, String)> {
    let key = format!("{}{}", PATHS_PREFIX, id);
    let existing = state
        .cache
        .get::<Value>(&key)
        .await
        .ok()
        .flatten()
        .ok_or((axum::http::StatusCode::NOT_FOUND, "path not found".into()))?;
    let src = existing
        .get("src_namespace")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !has_namespace_access(state, claims, src) {
        return Err((
            axum::http::StatusCode::FORBIDDEN,
            "namespace not in scope".into(),
        ));
    }
    let _ = state.cache.delete(&key).await;
    Ok(())
}

pub async fn status_for_path(state: &AppState, path: &Value) -> Value {
    let src_ns = path
        .get("src_namespace")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let src_wl = path
        .get("src_workload")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let dst_ns = path
        .get("dst_namespace")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let dst_svc = path
        .get("dst_service")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let port = path.get("port").and_then(|v| v.as_u64()).unwrap_or(0) as u16;

    let now = Utc::now();
    let recent_since = normalize_ts(&(now - Duration::minutes(SUSTAIN_MINUTES)).to_rfc3339());
    let baseline_since = normalize_ts(&(now - Duration::hours(1)).to_rfc3339());
    let baseline_until = recent_since.clone();

    let src_ns_q = src_ns.to_string();
    let src_wl_q = src_wl.to_string();
    let dst_ns_q = dst_ns.to_string();
    let dst_svc_q = dst_svc.to_string();
    let store = state.flow_store.clone();

    let (recent, baseline) = tokio::task::spawn_blocking(move || {
        let recent = store
            .query(&FlowQuery {
                src_namespace: Some(src_ns_q.clone()),
                src_pod: if src_wl_q.is_empty() {
                    None
                } else {
                    Some(src_wl_q.clone())
                },
                dst_namespace: Some(dst_ns_q.clone()),
                dst_pod: if dst_svc_q.is_empty() {
                    None
                } else {
                    Some(dst_svc_q.clone())
                },
                port: if port == 0 { None } else { Some(port) },
                since_rfc3339: recent_since,
                limit: 200,
                ..Default::default()
            })
            .unwrap_or_default();
        let baseline = store
            .query(&FlowQuery {
                src_namespace: Some(src_ns_q),
                src_pod: if src_wl_q.is_empty() {
                    None
                } else {
                    Some(src_wl_q)
                },
                dst_namespace: Some(dst_ns_q),
                dst_pod: if dst_svc_q.is_empty() {
                    None
                } else {
                    Some(dst_svc_q)
                },
                port: if port == 0 { None } else { Some(port) },
                since_rfc3339: baseline_since,
                until_rfc3339: baseline_until,
                limit: 200,
                ..Default::default()
            })
            .unwrap_or_default();
        (recent, baseline)
    })
    .await
    .unwrap_or_default();

    let count = |flows: &[crate::services::flow_store::StoredFlow], drop: bool| {
        flows
            .iter()
            .filter(|f| {
                let v = f.verdict.to_uppercase();
                let is_drop = v.contains("DROP") || v == "DENIED";
                if drop {
                    is_drop
                } else {
                    !is_drop
                }
            })
            .count()
    };

    let recent_fwd = count(&recent, false);
    let recent_drop = count(&recent, true);
    let base_fwd = count(&baseline, false);
    let base_drop = count(&baseline, true);

    let stats = state.flow_store.stats();
    let mut confidence = "observed";
    let mut status = "healthy";
    let mut notes: Vec<String> = Vec::new();

    if stats.gap_count > 0 || !stats.last_ingest_ok {
        confidence = "unavailable";
        status = "unknown";
        notes.push("capture gaps suppress confident conclusions".into());
    } else if recent.is_empty() {
        confidence = "unavailable";
        status = "unknown";
        notes.push("quiet traffic — no recent observed flows for this path".into());
    } else if recent_drop > 0 && recent_fwd == 0 && (base_drop == 0 || recent_drop > base_drop) {
        confidence = "inferred";
        status = "regression";
        notes.push("sustained drop pattern vs baseline".into());
    } else if recent_drop > base_drop.saturating_mul(2).max(3) {
        confidence = "inferred";
        status = "degraded";
        notes.push("elevated drops compared with baseline hour".into());
    }

    let evidence: Vec<String> = recent
        .iter()
        .filter(|f| {
            let v = f.verdict.to_uppercase();
            v.contains("DROP") || v == "DENIED"
        })
        .take(20)
        .map(|f| f.id.clone())
        .collect();

    json!({
        "path_id": path.get("id"),
        "status": status,
        "confidence": confidence,
        "recent": { "forwarded": recent_fwd, "dropped": recent_drop, "total": recent.len() },
        "baseline": { "forwarded": base_fwd, "dropped": base_drop, "total": baseline.len() },
        "evidence_flow_ids": evidence,
        "notes": notes,
        "investigate": {
            "source": { "namespace": src_ns, "name": src_wl },
            "destination": { "namespace": dst_ns, "name": dst_svc },
            "port": port,
        },
    })
}

async fn evaluate_all(state: &AppState) -> anyhow::Result<()> {
    let paths = state.cache.list_values(PATHS_PREFIX).await.unwrap_or_default();
    for p in paths {
        if p.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
            continue;
        }
        let status = status_for_path(state, &p).await;
        let st = status.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let path_id = p.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if path_id.is_empty() {
            continue;
        }

        // Rolling sample history for sustained-evidence gating.
        let sample_key = format!("{}{}", SAMPLE_PREFIX, path_id);
        let mut samples: Vec<String> = state
            .cache
            .get::<Value>(&sample_key)
            .await
            .ok()
            .flatten()
            .and_then(|v| {
                v.get("statuses")
                    .and_then(|s| s.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    })
            })
            .unwrap_or_default();
        samples.push(st.to_string());
        if samples.len() > 8 {
            let drain = samples.len() - 8;
            samples.drain(0..drain);
        }
        let _ = state
            .cache
            .set_durable(
                &sample_key,
                &json!({ "statuses": samples.clone(), "updated_at": Utc::now().to_rfc3339() }),
                ALERT_TTL,
            )
            .await;

        let sustained = samples.len() >= SUSTAIN_SAMPLES
            && samples
                .iter()
                .rev()
                .take(SUSTAIN_SAMPLES)
                .all(|s| s == "regression" || s == "degraded");
        if !sustained {
            continue;
        }

        let silence_key = format!("{}{}", SILENCE_PREFIX, path_id);
        if state
            .cache
            .get::<Value>(&silence_key)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            continue;
        }

        let alert_key = format!("{}{}", ALERTS_PREFIX, path_id);
        let alert = json!({
            "id": format!("calert-{}", path_id),
            "path_id": path_id,
            "status": st,
            "confidence": status.get("confidence"),
            "summary": status.get("notes"),
            "evidence_flow_ids": status.get("evidence_flow_ids"),
            "investigate": status.get("investigate"),
            "flows_link": "/flows",
            "investigate_link": "/investigate",
            "samples": samples,
            "updated_at": Utc::now().to_rfc3339(),
            "observe_only": true,
        });
        let _ = state.cache.set_durable(&alert_key, &alert, ALERT_TTL).await;
    }
    Ok(())
}

pub async fn silence_alert(
    state: &AppState,
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
    alert_or_path_id: &str,
    minutes: u64,
) -> Result<Value, (axum::http::StatusCode, String)> {
    let path_id = alert_or_path_id
        .strip_prefix("calert-")
        .unwrap_or(alert_or_path_id);
    let path_key = format!("{}{}", PATHS_PREFIX, path_id);
    let path = state
        .cache
        .get::<Value>(&path_key)
        .await
        .ok()
        .flatten()
        .ok_or((axum::http::StatusCode::NOT_FOUND, "path not found".into()))?;
    let src = path
        .get("src_namespace")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !has_namespace_access(state, claims, src) {
        return Err((
            axum::http::StatusCode::FORBIDDEN,
            "namespace not in scope".into(),
        ));
    }
    let ttl = minutes.clamp(5, 24 * 60) * 60;
    let silence_key = format!("{}{}", SILENCE_PREFIX, path_id);
    let body = json!({
        "path_id": path_id,
        "silenced_until": (Utc::now() + chrono::Duration::seconds(ttl as i64)).to_rfc3339(),
        "minutes": minutes.clamp(5, 24 * 60),
    });
    state
        .cache
        .set_durable(&silence_key, &body, ttl)
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                e.to_string(),
            )
        })?;
    // Drop active alert while silenced.
    let _ = state
        .cache
        .delete(&format!("{}{}", ALERTS_PREFIX, path_id))
        .await;
    Ok(body)
}

pub async fn list_alerts(
    state: &AppState,
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Vec<Value> {
    let paths = list_paths(state, claims).await;
    let allowed: std::collections::HashSet<String> = paths
        .iter()
        .filter_map(|p| p.get("id").and_then(|v| v.as_str()).map(str::to_string))
        .collect();
    state
        .cache
        .list_values(ALERTS_PREFIX)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|a| {
            a.get("path_id")
                .and_then(|v| v.as_str())
                .is_some_and(|id| allowed.contains(id))
        })
        .collect()
}
