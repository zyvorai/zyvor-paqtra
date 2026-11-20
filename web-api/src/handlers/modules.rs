// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Intelligence module endpoints
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::{actor_from_claims, audit_log, check_admin, to_json, track_request};
use crate::AppState;

const CHAOS_PREFIX: &str = "cv:chaos:";
const CANARY_PREFIX: &str = "cv:canary:";

/// Typed request for the autopolicy generation endpoint.
#[derive(Debug, Deserialize)]
pub struct GenerateAutopolicyRequest {
    #[serde(default = "default_namespace")]
    pub namespace: String,
}

fn default_namespace() -> String {
    "production".to_string()
}

/// Typed request for the chaos experiment endpoint.
#[derive(Debug, Deserialize)]
pub struct RunChaosRequest {
    #[serde(default = "default_chaos_name")]
    pub name: String,
    #[serde(default = "default_experiment_type")]
    pub experiment_type: String,
    #[serde(default = "default_target_namespace")]
    pub target_namespace: String,
    #[serde(default = "default_duration")]
    pub duration_secs: u64,
}

fn default_chaos_name() -> String {
    "ad-hoc-network-partition".to_string()
}

fn default_experiment_type() -> String {
    "network-partition".to_string()
}

fn default_target_namespace() -> String {
    "staging".to_string()
}

fn default_duration() -> u64 {
    120
}

/// Auto-generated policy suggestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoPolicyResult {
    pub request_id: String,
    pub policies_generated: u32,
    pub confidence: f64,
    pub policies: Vec<GeneratedPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedPolicy {
    pub name: String,
    pub namespace: String,
    pub description: String,
    pub spec: Value,
    pub confidence: f64,
}

/// Chaos experiment definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosExperiment {
    pub id: String,
    pub name: String,
    pub experiment_type: String,
    pub status: String,
    pub target_namespace: String,
    pub created_at: String,
    pub duration_secs: u64,
    pub results: Option<ChaosResults>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosResults {
    pub packets_dropped: u64,
    pub connections_failed: u64,
    pub services_impacted: u32,
    pub recovery_time_secs: Option<f64>,
}

/// Canary deployment status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanaryStatus {
    pub id: String,
    pub status: String,
    pub traffic_split: TrafficSplit,
    pub metrics: CanaryMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficSplit {
    pub stable: u32,
    pub canary: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanaryMetrics {
    pub success_rate: f64,
    pub latency_p99_ms: f64,
    pub error_count: u64,
}

/// Represents a unique traffic pair observed in Hubble flows.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct TrafficPair {
    src_pod: String,
    dst_pod: String,
    dst_port: u16,
    protocol: String,
}

pub async fn generate_autopolicy(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(req): Json<GenerateAutopolicyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let namespace = &req.namespace;

    // Query real Hubble flows for the target namespace
    let flows = state
        .hubble
        .get_flows(5000, Some(namespace))
        .await
        .unwrap_or_default();

    if flows.is_empty() {
        let result = AutoPolicyResult {
            request_id: uuid::Uuid::new_v4().to_string(),
            policies_generated: 0,
            confidence: 0.0,
            policies: vec![],
        };
        return Ok(Json(to_json(&result)));
    }

    // Analyze traffic patterns: group by destination pod -> set of (source pod, port, protocol)
    let mut dst_to_sources: HashMap<String, Vec<TrafficPair>> = HashMap::new();
    let mut unique_pairs: HashSet<TrafficPair> = HashSet::new();
    let total_flows = flows.len();

    for flow in &flows {
        if flow.verdict != "FORWARDED" {
            continue;
        }
        let src_label = if !flow.source.pod.is_empty() {
            flow.source.pod.clone()
        } else if !flow.source.ip.is_empty() {
            flow.source.ip.clone()
        } else {
            continue;
        };
        let dst_label = if !flow.destination.pod.is_empty() {
            flow.destination.pod.clone()
        } else if !flow.destination.ip.is_empty() {
            flow.destination.ip.clone()
        } else {
            continue;
        };

        let pair = TrafficPair {
            src_pod: src_label,
            dst_pod: dst_label.clone(),
            dst_port: flow.port,
            protocol: flow.protocol.clone(),
        };

        if unique_pairs.insert(pair.clone()) {
            dst_to_sources.entry(dst_label).or_default().push(pair);
        }
    }

    // Generate one CiliumNetworkPolicy per destination that received traffic
    let mut policies = Vec::new();
    for (dst, pairs) in &dst_to_sources {
        // Extract the app label from the pod name (strip trailing hash segments)
        let dst_app = extract_app_label(dst);

        // Collect unique source labels and ports
        let mut src_labels: HashSet<String> = HashSet::new();
        let mut port_protos: HashSet<(u16, String)> = HashSet::new();
        let mut pair_flow_count: usize = 0;

        for pair in pairs {
            src_labels.insert(extract_app_label(&pair.src_pod));
            if pair.dst_port > 0 {
                port_protos.insert((pair.dst_port, pair.protocol.clone()));
            }
            // Count how many total flows match this pair
            pair_flow_count += flows
                .iter()
                .filter(|f| {
                    (f.destination.pod == *dst || f.destination.ip == *dst)
                        && f.verdict == "FORWARDED"
                })
                .count();
        }

        // Confidence: ratio of flows backing this policy vs total observed flows,
        // capped at 0.99 and floored at 0.1
        let raw_confidence = pair_flow_count as f64 / total_flows as f64;
        // Not `clamp`: `min`/`max` map a NaN ratio to 0.99, `clamp` would return NaN.
        #[allow(clippy::manual_clamp)]
        let confidence = raw_confidence.min(0.99).max(0.1);

        let policy_name = format!("allow-ingress-to-{}-{}", dst_app, namespace);

        // Build ingress from-endpoints
        let from_endpoints: Vec<Value> = src_labels
            .iter()
            .map(|src| {
                json!({
                    "matchLabels": {
                        "app": src
                    }
                })
            })
            .collect();

        // Build toPorts
        let ports: Vec<Value> = port_protos
            .iter()
            .map(|(port, proto)| json!({ "port": port.to_string(), "protocol": proto }))
            .collect();

        let ingress_rule = if ports.is_empty() {
            json!([{
                "fromEndpoints": from_endpoints
            }])
        } else {
            json!([{
                "fromEndpoints": from_endpoints,
                "toPorts": [{
                    "ports": ports
                }]
            }])
        };

        let description = format!(
            "Allow traffic from {} source(s) to {} on {} port(s). \
             Derived from {} observed flows in namespace '{}'.",
            src_labels.len(),
            dst_app,
            port_protos.len(),
            pair_flow_count,
            namespace
        );

        policies.push(GeneratedPolicy {
            name: policy_name.clone(),
            namespace: namespace.to_string(),
            description,
            spec: json!({
                "apiVersion": "cilium.io/v2",
                "kind": "CiliumNetworkPolicy",
                "metadata": {
                    "name": policy_name,
                    "namespace": namespace
                },
                "spec": {
                    "endpointSelector": {
                        "matchLabels": {
                            "app": dst_app
                        }
                    },
                    "ingress": ingress_rule
                }
            }),
            confidence,
        });
    }

    // Sort by confidence descending so most confident policies come first
    policies.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let avg_confidence = if policies.is_empty() {
        0.0
    } else {
        policies.iter().map(|p| p.confidence).sum::<f64>() / policies.len() as f64
    };

    let result = AutoPolicyResult {
        request_id: uuid::Uuid::new_v4().to_string(),
        policies_generated: policies.len() as u32,
        confidence: (avg_confidence * 100.0).round() / 100.0,
        policies,
    };

    audit_log(
        &state,
        "autopolicy.generate",
        &result.request_id,
        namespace,
        "Autopolicy generated",
        &actor_from_claims(&claims),
        "success",
    )
    .await;

    Ok(Json(to_json(&result)))
}

/// Extract a short app label from a Kubernetes pod name.
/// E.g. "frontend-7b4d9c8f5-x2k9z" -> "frontend"
fn extract_app_label(pod_name: &str) -> String {
    let parts: Vec<&str> = pod_name.split('-').collect();
    // Kubernetes pods typically end with replicaset hash + pod hash
    // Strip trailing segments that look like hashes (all alphanumeric, len <= 10)
    let meaningful: Vec<&str> = parts
        .iter()
        .take_while(|p| {
            // Keep segments that are not purely hash-like
            p.len() > 10
                || !p.chars().all(|c| c.is_ascii_alphanumeric())
                || parts.iter().position(|x| x == *p) == Some(0)
        })
        .copied()
        .collect();

    if meaningful.is_empty() {
        parts.first().unwrap_or(&"unknown").to_string()
    } else {
        meaningful.join("-")
    }
}

pub async fn list_chaos_experiments(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, StatusCode> {
    track_request(&state, |_| {}).await;

    let items = state
        .cache
        .list_values(CHAOS_PREFIX)
        .await
        .unwrap_or_default();
    let total = items.len();

    Ok(Json(json!({
        "experiments": items,
        "total": total,
    })))
}

pub async fn run_chaos_experiment(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(_req): Json<RunChaosRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;
    // Nothing injects faults yet. Storing a "started" record would make the
    // experiments list claim an experiment ran when the network was untouched.
    Err(super::not_implemented(
        "Chaos fault injection",
        "no fault was injected and no experiment was recorded",
    ))
}

pub async fn canary_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    track_request(&state, |_| {}).await;

    let cache_key = format!("{}{}", CANARY_PREFIX, id);

    // Try to load from cache first
    if let Ok(Some(cached)) = state.cache.get::<Value>(&cache_key).await {
        return Ok(Json(cached));
    }

    // Query Kubernetes for deployment rollout status
    let deploy_json = state
        .k8s
        .kubectl_json(&["get", "deployment", &id, "-o", "json"])
        .await;

    let now = chrono::Utc::now().to_rfc3339();

    // Parse real deployment data if available
    let (status_str, ready_replicas, total_replicas, updated_replicas) =
        if let Some(status_obj) = deploy_json.get("status") {
            let ready = status_obj
                .get("readyReplicas")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let total = status_obj
                .get("replicas")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let updated = status_obj
                .get("updatedReplicas")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            let phase = if updated < total {
                "progressing"
            } else if ready == total {
                "stable"
            } else {
                "progressing"
            };

            (phase.to_string(), ready, total, updated)
        } else {
            // K8s not reachable or deployment not found
            ("unknown".to_string(), 0, 0, 0)
        };

    // Derive traffic split from rollout progress
    let canary_pct = if total_replicas > 0 {
        ((updated_replicas as f64 / total_replicas as f64) * 100.0).round() as u32
    } else {
        0
    };
    let stable_pct = 100u32.saturating_sub(canary_pct);

    // Query recent flows to compute real success metrics for this deployment
    let flows = state.hubble.get_flows(1000, None).await.unwrap_or_default();

    let relevant_flows: Vec<_> = flows
        .iter()
        .filter(|f| f.destination.pod.contains(&id) || f.source.pod.contains(&id))
        .collect();

    let total_relevant = relevant_flows.len() as u64;
    let forwarded = relevant_flows
        .iter()
        .filter(|f| f.verdict == "FORWARDED")
        .count() as u64;
    let dropped = relevant_flows
        .iter()
        .filter(|f| f.verdict == "DROPPED")
        .count() as u64;

    let success_rate = if total_relevant > 0 {
        (forwarded as f64 / total_relevant as f64) * 100.0
    } else {
        0.0
    };

    let status = CanaryStatus {
        id: id.clone(),
        status: status_str.clone(),
        traffic_split: TrafficSplit {
            stable: stable_pct,
            canary: canary_pct,
        },
        metrics: CanaryMetrics {
            success_rate: (success_rate * 100.0).round() / 100.0,
            latency_p99_ms: 0.0,
            error_count: dropped,
        },
    };

    // Determine promotion recommendation based on live metrics
    let recommendation = if success_rate >= 99.5 && dropped == 0 {
        "promote"
    } else if success_rate >= 95.0 {
        "continue"
    } else if total_relevant == 0 {
        "insufficient-data"
    } else {
        "rollback"
    };

    let response = json!({
        "canary": to_json(&status),
        "analysis": {
            "phase": format!("canary-weight-{}", canary_pct),
            "started_at": now,
            "last_checked_at": now,
            "replicas": {
                "total": total_replicas,
                "ready": ready_replicas,
                "updated": updated_replicas
            },
            "flows_analyzed": total_relevant,
            "promotion_threshold": {
                "success_rate_min": 99.5,
                "latency_p99_max_ms": 100.0,
                "error_count_max": 25
            },
            "recommendation": recommendation,
        }
    });

    // Cache the result for 30 seconds to avoid hammering K8s/Hubble
    let _ = state.cache.set(&cache_key, &response, 30).await;

    Ok(Json(response))
}
