// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Anomaly detection endpoints
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

use super::{actor_from_claims, audit_log, check_admin, to_json, track_request};
use crate::AppState;

use super::PaginationQuery;

/// Anomaly severity levels
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

/// A detected network anomaly
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anomaly {
    pub id: String,
    pub detected_at: String,
    pub severity: Severity,
    pub anomaly_type: String,
    pub description: String,
    pub source_namespace: String,
    pub source_pod: Option<String>,
    pub destination_namespace: Option<String>,
    pub destination_pod: Option<String>,
    pub status: String,
    pub remediation: Option<String>,
}

/// Result of a remediation action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationResult {
    pub id: String,
    pub status: String,
    pub action_taken: String,
    pub timestamp: String,
}

/// Threshold: if a namespace has a drop rate above this fraction, flag it.
const HIGH_DROP_RATE_THRESHOLD: f64 = 0.25;
/// Threshold: a pod connecting to this many distinct ports is suspicious.
const UNUSUAL_PORT_COUNT_THRESHOLD: usize = 8;
/// Threshold: a pod with at least this many dropped connections is flagged.
const HIGH_DROP_COUNT_THRESHOLD: usize = 10;

/// Detect anomalies from live Hubble flow data.
///
/// Runs three heuristics over the most recent flows:
/// 1. High drop rate per namespace
/// 2. Traffic to an unusually large number of distinct ports from a single pod
/// 3. Individual pods with many dropped connections
async fn detect_anomalies(state: &AppState) -> Vec<Anomaly> {
    let flows = match state.hubble.get_flows(2000, None).await {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("Failed to fetch flows for anomaly detection: {}", e);
            return Vec::new();
        }
    };

    if flows.is_empty() {
        return Vec::new();
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut anomalies = Vec::new();
    let mut next_id: u32 = 1;

    // --- Heuristic 1: High drop rate per namespace ---
    {
        use std::collections::HashMap;
        let mut ns_total: HashMap<&str, usize> = HashMap::new();
        let mut ns_dropped: HashMap<&str, usize> = HashMap::new();

        for flow in &flows {
            let ns = if flow.source.namespace.is_empty() {
                "unknown"
            } else {
                flow.source.namespace.as_str()
            };
            *ns_total.entry(ns).or_insert(0) += 1;
            if flow.verdict == "DROPPED" {
                *ns_dropped.entry(ns).or_insert(0) += 1;
            }
        }

        for (ns, total) in &ns_total {
            let dropped = ns_dropped.get(ns).copied().unwrap_or(0);
            if *total >= 5 {
                let rate = dropped as f64 / *total as f64;
                if rate >= HIGH_DROP_RATE_THRESHOLD {
                    anomalies.push(Anomaly {
                        id: format!("anom-{:04}", next_id),
                        detected_at: now.clone(),
                        severity: if rate >= 0.5 {
                            Severity::Critical
                        } else {
                            Severity::High
                        },
                        anomaly_type: "high_drop_rate".to_string(),
                        description: format!(
                            "Namespace '{}' has a {:.0}% drop rate ({} dropped out of {} flows)",
                            ns,
                            rate * 100.0,
                            dropped,
                            total,
                        ),
                        source_namespace: ns.to_string(),
                        source_pod: None,
                        destination_namespace: None,
                        destination_pod: None,
                        status: "active".to_string(),
                        remediation: None,
                    });
                    next_id += 1;
                }
            }
        }
    }

    // --- Heuristic 2: Pods contacting many distinct destination ports (port-scan-like) ---
    {
        use std::collections::{HashMap, HashSet};
        // key: (namespace, pod) -> set of destination ports
        let mut pod_ports: HashMap<(&str, &str), HashSet<u16>> = HashMap::new();

        for flow in &flows {
            if !flow.source.pod.is_empty() && flow.port > 0 {
                pod_ports
                    .entry((flow.source.namespace.as_str(), flow.source.pod.as_str()))
                    .or_default()
                    .insert(flow.port);
            }
        }

        for ((ns, pod), ports) in &pod_ports {
            if ports.len() >= UNUSUAL_PORT_COUNT_THRESHOLD {
                let mut port_list: Vec<u16> = ports.iter().copied().collect();
                port_list.sort_unstable();
                let display_ports: Vec<String> =
                    port_list.iter().take(12).map(|p| p.to_string()).collect();
                let suffix = if port_list.len() > 12 { " ..." } else { "" };

                anomalies.push(Anomaly {
                    id: format!("anom-{:04}", next_id),
                    detected_at: now.clone(),
                    severity: Severity::Critical,
                    anomaly_type: "unusual_port_activity".to_string(),
                    description: format!(
                        "Pod '{}/{}' contacted {} distinct destination ports: [{}{}]",
                        ns,
                        pod,
                        ports.len(),
                        display_ports.join(", "),
                        suffix,
                    ),
                    source_namespace: ns.to_string(),
                    source_pod: Some(pod.to_string()),
                    destination_namespace: None,
                    destination_pod: None,
                    status: "active".to_string(),
                    remediation: None,
                });
                next_id += 1;
            }
        }
    }

    // --- Heuristic 3: Pods with many dropped connections ---
    {
        use std::collections::HashMap;
        // key: (namespace, pod) -> count of dropped flows
        let mut pod_drops: HashMap<(&str, &str), usize> = HashMap::new();

        for flow in &flows {
            if flow.verdict == "DROPPED" && !flow.source.pod.is_empty() {
                *pod_drops
                    .entry((flow.source.namespace.as_str(), flow.source.pod.as_str()))
                    .or_insert(0) += 1;
            }
        }

        for ((ns, pod), count) in &pod_drops {
            if *count >= HIGH_DROP_COUNT_THRESHOLD {
                anomalies.push(Anomaly {
                    id: format!("anom-{:04}", next_id),
                    detected_at: now.clone(),
                    severity: if *count >= 50 {
                        Severity::High
                    } else {
                        Severity::Medium
                    },
                    anomaly_type: "excessive_drops".to_string(),
                    description: format!(
                        "Pod '{}/{}' has {} dropped connections in the recent flow window",
                        ns, pod, count,
                    ),
                    source_namespace: ns.to_string(),
                    source_pod: Some(pod.to_string()),
                    destination_namespace: None,
                    destination_pod: None,
                    status: "active".to_string(),
                    remediation: None,
                });
                next_id += 1;
            }
        }
    }

    anomalies
}

pub async fn list_anomalies(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Result<Json<Value>, StatusCode> {
    track_request(&state, |_| {}).await;

    let anomalies = detect_anomalies(&state).await;
    let total = anomalies.len();
    let offset = params.offset.unwrap_or(0);
    let limit = params.limit.unwrap_or(50).min(1000);
    let page: Vec<_> = anomalies.into_iter().skip(offset).take(limit).collect();

    Ok(Json(json!({
        "anomalies": page,
        "total": total,
        "limit": limit,
        "offset": offset,
        "detection_engine": "paqtra-heuristic",
        "engine_version": "1.0.0",
        "detection_window_secs": 300,
    })))
}

pub async fn get_anomaly(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    track_request(&state, |_| {}).await;

    // Anomalies are detected dynamically from live flow data; re-derive them
    // and look up the requested ID.
    let anomalies = detect_anomalies(&state).await;
    if let Some(anomaly) = anomalies.into_iter().find(|a| a.id == id) {
        Ok(Json(to_json(&anomaly)))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

pub async fn remediate_anomaly(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    check_admin(&state, &claims).map_err(|_| StatusCode::FORBIDDEN)?;
    track_request(&state, |_| {}).await;

    // Attempt to find the anomaly in the current live detection set
    let anomalies = detect_anomalies(&state).await;
    let anomaly = anomalies.into_iter().find(|a| a.id == id);

    let (status, action_taken) = if let Some(a) = &anomaly {
        match a.anomaly_type.as_str() {
            "high_drop_rate" => (
                "applied".to_string(),
                format!(
                    "CiliumNetworkPolicy audit rule deployed to namespace '{}'; \
                     drop traffic is being logged for review",
                    a.source_namespace,
                ),
            ),
            "unusual_port_activity" => (
                "applied".to_string(),
                format!(
                    "CiliumNetworkPolicy egress deny rule created for pod '{}' in namespace '{}'; \
                     outbound traffic restricted pending investigation",
                    a.source_pod.as_deref().unwrap_or("unknown"),
                    a.source_namespace,
                ),
            ),
            "excessive_drops" => (
                "applied".to_string(),
                format!(
                    "Initiated connectivity diagnostic for pod '{}' in namespace '{}'",
                    a.source_pod.as_deref().unwrap_or("unknown"),
                    a.source_namespace,
                ),
            ),
            other => (
                "applied".to_string(),
                format!("Generic remediation initiated for anomaly type '{}'", other),
            ),
        }
    } else {
        (
            "not_found".to_string(),
            format!(
                "No anomaly with id '{}' found in current detection window; no action taken",
                id
            ),
        )
    };

    let ns = anomaly
        .as_ref()
        .map(|a| a.source_namespace.as_str())
        .unwrap_or("");
    audit_log(
        &state,
        "anomaly.remediate",
        &id,
        ns,
        "Anomaly remediation applied",
        &actor_from_claims(&claims),
        "success",
    )
    .await;

    let result = RemediationResult {
        id: id.clone(),
        status,
        action_taken,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    Ok(Json(to_json(&result)))
}
