// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

use super::{
    actor_from_claims, audit_log, check_admin, paginate_json, track_request, PaginationQuery,
};
use crate::services::healer;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct StartRecordingRequest {
    #[serde(default = "default_recording_name")]
    pub name: String,
    #[serde(default)]
    pub namespace: Option<String>,
}

fn default_recording_name() -> String {
    "recording".to_string()
}

// ── Replay ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ReplayRecording {
    pub id: String,
    pub name: String,
    pub namespace: String,
    pub start_time: String,
    pub end_time: String,
    pub flow_count: u64,
    pub status: String,
    pub size_bytes: u64,
}

const RECORDINGS_PREFIX: &str = "cv:recordings:";

pub async fn list_recordings(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;
    let items = state
        .cache
        .list_values(RECORDINGS_PREFIX)
        .await
        .unwrap_or_default();
    Json(paginate_json(items, &params, "recordings"))
}

pub async fn start_recording(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(body): Json<StartRecordingRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let id = format!("rec-{}", uuid::Uuid::new_v4());
    let ns = body.namespace.as_deref().unwrap_or("default");
    let now = chrono::Utc::now().to_rfc3339();
    let recording = serde_json::json!({
        "id": id,
        "name": body.name,
        "namespace": ns,
        "start_time": now,
        "status": "recording",
        "flow_count": 0,
    });

    let _ = state
        .cache
        .set_persistent(&format!("{}{}", RECORDINGS_PREFIX, id), &recording)
        .await;
    audit_log(
        &state,
        "recording.start",
        &id,
        ns,
        "Recording started",
        &actor_from_claims(&claims),
        "success",
    )
    .await;

    Ok(Json(serde_json::json!({
        "id": id,
        "name": body.name,
        "status": "recording",
        "message": "Recording started",
    })))
}

pub async fn stop_recording(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let key = format!("{}{}", RECORDINGS_PREFIX, id);
    if let Ok(Some(mut rec)) = state.cache.get::<serde_json::Value>(&key).await {
        rec["status"] = serde_json::json!("completed");
        rec["end_time"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
        let _ = state.cache.set_persistent(&key, &rec).await;
        audit_log(
            &state,
            "recording.stop",
            &id,
            "",
            "Recording stopped",
            &actor_from_claims(&claims),
            "success",
        )
        .await;
        Ok(Json(
            serde_json::json!({ "id": id, "status": "stopped", "message": "Recording stopped" }),
        ))
    } else {
        Ok(Json(serde_json::json!({ "id": id, "status": "not_found" })))
    }
}

// ── Healer ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct HealerProblem {
    pub id: String,
    #[serde(rename = "type")]
    pub problem_type: String,
    pub severity: String,
    pub description: String,
    pub affected_pods: Vec<String>,
    pub namespace: String,
    pub detected_at: String,
    pub status: String,
    pub proposed_fix: String,
}

pub async fn list_healer_problems(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    let flows = state.hubble.get_flows(500, None).await.unwrap_or_default();
    let mut problems = Vec::new();
    for p in healer::detect(&flows) {
        let applied = state
            .cache
            .get::<serde_json::Value>(&format!("{}{}", healer::FIXES_PREFIX, p.id))
            .await
            .ok()
            .flatten()
            .is_some();
        problems.push(p.to_json(applied));
    }
    Json(paginate_json(problems, &params, "problems"))
}

#[derive(Debug, Deserialize)]
pub struct ApplyFixQuery {
    /// Return the manifest that would be applied without touching the cluster.
    #[serde(default)]
    pub dry_run: bool,
}

fn healer_error(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": msg })))
}

/// Apply the fix for a detected problem. The problem is re-detected from live
/// flows and matched by its stable ID, so the fix always targets what the
/// caller saw. Problems with no automated remediation are refused rather than
/// reported as applied.
pub async fn apply_healer_fix(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
    Query(q): Query<ApplyFixQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;
    let actor = actor_from_claims(&claims);

    let flows = state.hubble.get_flows(500, None).await.unwrap_or_default();
    let problem = healer::detect(&flows)
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| {
            healer_error(
                StatusCode::NOT_FOUND,
                "Problem not found: it is no longer detected in live flows. Refresh the problem list.",
            )
        })?;

    if !problem.auto_fixable() {
        let msg = if problem.kind == healer::ProblemKind::DnsBlocked {
            "Namespace could not be determined for these flows, so no fix was applied"
        } else {
            "No automated fix for this problem type: review the namespace's policies and add the allow rules you intend"
        };
        return Err(healer_error(StatusCode::UNPROCESSABLE_ENTITY, msg));
    }

    let request = crate::models::policy::CreatePolicyRequest {
        name: healer::DNS_POLICY_NAME.to_string(),
        namespace: problem.namespace.clone(),
        spec: healer::dns_allow_spec(),
    };

    if q.dry_run {
        return Ok(Json(serde_json::json!({
            "id": id,
            "status": "dry_run",
            "policy": format!("{}/{}", request.namespace, request.name),
            "spec": request.spec,
        })));
    }

    match state.k8s.create_policy(&request).await {
        Ok(_) => {
            let policy = format!("{}/{}", request.namespace, request.name);
            let record = serde_json::json!({
                "id": id,
                "policy": policy,
                "applied_by": actor,
                "applied_at": chrono::Utc::now().to_rfc3339(),
            });
            if let Err(e) = state
                .cache
                .set_persistent(&format!("{}{}", healer::FIXES_PREFIX, id), &record)
                .await
            {
                tracing::warn!("Failed to record healer fix {}: {}", id, e);
            }
            audit_log(
                &state,
                "healer.apply",
                &id,
                "",
                &format!("Applied CiliumNetworkPolicy {}", policy),
                &actor,
                "success",
            )
            .await;
            Ok(Json(serde_json::json!({
                "id": id,
                "status": "applied",
                "policy": policy,
                "message": format!("CiliumNetworkPolicy {} applied", policy),
            })))
        }
        Err(e) => {
            tracing::warn!("Healer fix {} failed: {}", id, e);
            audit_log(
                &state,
                "healer.apply",
                &id,
                "",
                &format!("Fix failed: {}", e),
                &actor,
                "failure",
            )
            .await;
            Err(healer_error(
                StatusCode::BAD_GATEWAY,
                &format!("Failed to apply fix: {}", e),
            ))
        }
    }
}

// ── RootCause ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct PacketDrop {
    pub id: String,
    pub timestamp: String,
    pub source: String,
    pub destination: String,
    pub protocol: String,
    pub drop_reason: String,
    pub root_cause: String,
    pub remediation: String,
    pub namespace: String,
    pub count: u32,
}

pub async fn list_packet_drops(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Get real dropped flows from Hubble
    let flows = state.hubble.get_flows(500, None).await.unwrap_or_default();

    let items: Vec<serde_json::Value> = flows
        .iter()
        .filter(|f| f.verdict == "DROPPED")
        .enumerate()
        .map(|(i, f)| {
            serde_json::json!({
                "id": format!("drop-{:03}", i + 1),
                "timestamp": f.timestamp,
                "source": format!("{}/{}", f.source.namespace, f.source.pod),
                "destination": format!("{}/{}", f.destination.namespace, f.destination.pod),
                "protocol": f.protocol,
                "port": f.port,
                "drop_reason": "POLICY_DENIED",
                "namespace": f.source.namespace,
            })
        })
        .collect();

    Json(paginate_json(items, &params, "drops"))
}

pub async fn analyze_drops(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let flows = state.hubble.get_flows(1000, None).await.unwrap_or_default();
    let dropped: Vec<_> = flows.iter().filter(|f| f.verdict == "DROPPED").collect();
    let total_drops = dropped.len();

    // Count by source namespace
    let mut by_ns: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for f in &dropped {
        *by_ns.entry(&f.source.namespace).or_default() += 1;
    }
    let top_ns = by_ns
        .iter()
        .max_by_key(|(_, c)| *c)
        .map(|(ns, _)| *ns)
        .unwrap_or("none");

    Ok(Json(serde_json::json!({
        "analysis": {
            "total_flows": flows.len(),
            "total_drops": total_drops,
            "drop_rate": if flows.is_empty() { 0.0 } else { total_drops as f64 / flows.len() as f64 * 100.0 },
            "top_source_namespace": top_ns,
            "namespaces_affected": by_ns.len(),
            "recommendation": if total_drops > 0 {
                format!("Review CiliumNetworkPolicies in {} namespace ({} drops)", top_ns, by_ns.get(top_ns).unwrap_or(&0))
            } else {
                "No drops detected in recent flows".to_string()
            }
        }
    })))
}

// ── MultiCluster ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ClusterInfo {
    pub name: String,
    pub status: String,
    pub endpoint: String,
    pub region: String,
    pub nodes: u32,
    pub pods: u32,
    pub latency_ms: f64,
    pub last_sync: String,
    pub cilium_version: String,
}

pub async fn list_clusters(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    let configured_clusters = state.hubble.clusters();

    // If we have configured clusters, build info from them first
    if !configured_clusters.is_empty() {
        let mut items = Vec::new();

        for (name, addr) in configured_clusters {
            let healthy = state.hubble.is_address_healthy(addr).await;

            // Query K8s node/pod counts (best-effort)
            let node_count = {
                let data = state
                    .k8s
                    .kubectl_json(&["get", "nodes", "-o", "json"])
                    .await;
                data.get("items")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0) as u64
            };
            let pod_count = {
                let data = state
                    .k8s
                    .kubectl_json(&["get", "pods", "--all-namespaces", "-o", "json"])
                    .await;
                data.get("items")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0) as u64
            };

            items.push(serde_json::json!({
                "name": name,
                "status": if healthy { "connected" } else { "unreachable" },
                "endpoint": addr,
                "nodes": node_count,
                "pods": pod_count,
                "hubble_healthy": healthy,
                "last_sync": chrono::Utc::now().to_rfc3339(),
            }));
        }

        if !items.is_empty() {
            return Json(paginate_json(items, &params, "clusters"));
        }
    }

    // Try to get cluster mesh status from cilium agent
    let mesh_output = state
        .k8s
        .exec_cilium_agent(&["cilium", "clustermesh", "status", "-o", "json"])
        .await;

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&mesh_output) {
        if let Some(clusters) = parsed.get("clusters").and_then(|v| v.as_array()) {
            let items: Vec<serde_json::Value> = clusters
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "name": c.get("name").and_then(|v| v.as_str()).unwrap_or("unknown"),
                        "status": c.get("status").and_then(|v| v.as_str()).unwrap_or("unknown"),
                        "endpoint": c.get("endpoint").and_then(|v| v.as_str()).unwrap_or(""),
                        "nodes": c.get("nodes").and_then(|v| v.as_u64()).unwrap_or(0),
                        "last_sync": c.get("last_change").and_then(|v| v.as_str()).unwrap_or(""),
                    })
                })
                .collect();
            if !items.is_empty() {
                return Json(paginate_json(items, &params, "clusters"));
            }
        }
    }

    // Fallback: query CiliumClusterMeshConfig CRDs
    let data = state
        .k8s
        .kubectl_json(&[
            "get",
            "ciliumclustermeshconfigs",
            "--all-namespaces",
            "-o",
            "json",
        ])
        .await;
    if let Some(items) = data.get("items").and_then(|v| v.as_array()) {
        if !items.is_empty() {
            let clusters: Vec<serde_json::Value> = items
                .iter()
                .map(|item| {
                    let name = item
                        .pointer("/metadata/name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let ns = item
                        .pointer("/metadata/namespace")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    serde_json::json!({
                        "name": name,
                        "namespace": ns,
                        "status": "configured",
                    })
                })
                .collect();
            return Json(paginate_json(clusters, &params, "clusters"));
        }
    }

    // No remote clusters found — return empty list
    Json(paginate_json(
        Vec::<serde_json::Value>::new(),
        &params,
        "clusters",
    ))
}

pub async fn sync_cluster(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(_name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;
    Err(super::not_implemented(
        "Cross-cluster policy sync",
        "no policies were copied. Apply policies to each cluster directly for now",
    ))
}

// ── Heatmap ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct HeatmapCell {
    pub source_namespace: String,
    pub destination_namespace: String,
    pub flow_count: u64,
    pub dropped_count: u64,
    pub avg_latency_ms: f64,
}

pub async fn heatmap_data(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    let multi_cluster = state.hubble.clusters().len() > 1;

    // Collect flows: use multi-cluster aggregation when multiple clusters are configured
    let all_flows: Vec<crate::models::flow::Flow> = if multi_cluster {
        let cluster_flows = state.hubble.get_flows_multi_cluster(1000, None).await;
        cluster_flows
            .into_iter()
            .flat_map(|(_, flows)| flows)
            .collect()
    } else {
        state.hubble.get_flows(1000, None).await.unwrap_or_default()
    };

    let mut ns_set = std::collections::HashSet::new();
    let mut cell_map: std::collections::HashMap<(String, String), (u64, u64)> =
        std::collections::HashMap::new();

    for flow in &all_flows {
        let raw_src_ns = if flow.source.namespace.is_empty() {
            "unknown"
        } else {
            &flow.source.namespace
        };
        let raw_dst_ns = if flow.destination.namespace.is_empty() {
            "unknown"
        } else {
            &flow.destination.namespace
        };

        // In multi-cluster mode, prefix namespace with cluster name (e.g. "us-east/default")
        let src_ns = if multi_cluster {
            if let Some(ref cluster) = flow.cluster {
                format!("{}/{}", cluster, raw_src_ns)
            } else {
                raw_src_ns.to_string()
            }
        } else {
            raw_src_ns.to_string()
        };

        let dst_ns = if multi_cluster {
            if let Some(ref cluster) = flow.cluster {
                format!("{}/{}", cluster, raw_dst_ns)
            } else {
                raw_dst_ns.to_string()
            }
        } else {
            raw_dst_ns.to_string()
        };

        ns_set.insert(src_ns.clone());
        ns_set.insert(dst_ns.clone());
        let entry = cell_map.entry((src_ns, dst_ns)).or_default();
        entry.0 += 1; // flow_count
        if flow.verdict == "DROPPED" {
            entry.1 += 1; // dropped_count
        }
    }

    let cells: Vec<HeatmapCell> = cell_map
        .into_iter()
        .map(|((src, dst), (fc, dc))| HeatmapCell {
            source_namespace: src,
            destination_namespace: dst,
            flow_count: fc,
            dropped_count: dc,
            avg_latency_ms: 0.0,
        })
        .collect();

    let namespaces: Vec<String> = ns_set.into_iter().collect();
    Json(
        serde_json::json!({ "cells": cells, "namespaces": namespaces, "multi_cluster": multi_cluster }),
    )
}

// ── Service Dependencies ────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ServiceDep {
    pub source: String,
    pub destination: String,
    pub protocol: String,
    pub port: u16,
    pub request_rate: f64,
    pub error_rate: f64,
    pub latency_p50: f64,
    pub latency_p99: f64,
}

pub async fn list_dependencies(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Build dependencies from real Hubble flows
    let flows = state.hubble.get_flows(500, None).await.unwrap_or_default();

    let mut dep_map: std::collections::HashMap<(String, String, u16), (String, u64, u64)> =
        std::collections::HashMap::new();
    for flow in &flows {
        let src = if !flow.source.pod.is_empty() {
            flow.source
                .pod
                .split('-')
                .take(2)
                .collect::<Vec<_>>()
                .join("-")
        } else {
            continue;
        };
        let dst = if !flow.destination.pod.is_empty() {
            flow.destination
                .pod
                .split('-')
                .take(2)
                .collect::<Vec<_>>()
                .join("-")
        } else {
            continue;
        };
        if src == dst {
            continue;
        }
        let entry = dep_map
            .entry((src, dst, flow.port))
            .or_insert((flow.protocol.clone(), 0, 0));
        entry.1 += 1; // total
        if flow.verdict == "DROPPED" {
            entry.2 += 1;
        } // errors
    }

    // Collect L7 HTTP info per dependency key
    type DepKey = (String, String, u16);
    type L7Info = (Option<String>, Option<String>);
    let mut l7_map: std::collections::HashMap<DepKey, L7Info> = std::collections::HashMap::new();
    for flow in &flows {
        let src = if !flow.source.pod.is_empty() {
            flow.source
                .pod
                .split('-')
                .take(2)
                .collect::<Vec<_>>()
                .join("-")
        } else {
            continue;
        };
        let dst = if !flow.destination.pod.is_empty() {
            flow.destination
                .pod
                .split('-')
                .take(2)
                .collect::<Vec<_>>()
                .join("-")
        } else {
            continue;
        };
        if src == dst {
            continue;
        }
        if flow.http_method.is_some() || flow.http_url.is_some() {
            l7_map
                .entry((src, dst, flow.port))
                .or_insert((flow.http_method.clone(), flow.http_url.clone()));
        }
    }

    let items: Vec<serde_json::Value> = dep_map
        .into_iter()
        .map(|((src, dst, port), (proto, total, errors))| {
            let error_rate = if total > 0 {
                errors as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            let l7 = l7_map.get(&(src.clone(), dst.clone(), port));
            let mut entry = serde_json::json!({
                "source": src, "destination": dst, "protocol": proto,
                "port": port, "request_count": total, "error_rate": error_rate,
            });
            if let Some((Some(method), _)) = l7 {
                entry["http_method"] = serde_json::json!(method);
            }
            if let Some((_, Some(url))) = l7 {
                entry["http_url"] = serde_json::json!(url);
            }
            entry
        })
        .collect();

    Json(paginate_json(items, &params, "dependencies"))
}

// ── Security Dashboard ──────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct SecurityFinding {
    pub id: String,
    pub category: String,
    pub severity: String,
    pub title: String,
    pub description: String,
    pub resource: String,
    pub namespace: String,
    pub remediation: String,
    pub status: String,
}

pub async fn list_security_findings(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    let mut findings = Vec::new();
    let mut idx = 0;

    // Check namespaces without policies
    let policies = state.k8s.list_policies().await.unwrap_or_default();
    let ns_with_policies: std::collections::HashSet<String> =
        policies.iter().map(|p| p.namespace.clone()).collect();

    let ns_data = state
        .k8s
        .kubectl_json(&["get", "namespaces", "-o", "json"])
        .await;
    if let Some(items) = ns_data.get("items").and_then(|v| v.as_array()) {
        for item in items {
            let name = item
                .get("metadata")
                .and_then(|m| m.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if name.starts_with("kube-") || name == "cilium" {
                continue;
            }
            if !ns_with_policies.contains(name) {
                idx += 1;
                findings.push(serde_json::json!({
                    "id": format!("sec-{:03}", idx),
                    "category": "network_policy",
                    "severity": "high",
                    "title": "Namespace without network policies",
                    "description": format!("Namespace '{}' has no CiliumNetworkPolicy applied", name),
                    "resource": format!("namespace/{}", name),
                    "namespace": name,
                    "remediation": "Apply default-deny ingress/egress policy",
                    "status": "open",
                }));
            }
        }
    }

    // Check for dropped flows indicating policy issues
    let flows = state.hubble.get_flows(200, None).await.unwrap_or_default();
    let drop_count = flows.iter().filter(|f| f.verdict == "DROPPED").count();
    if drop_count > 0 {
        idx += 1;
        findings.push(serde_json::json!({
            "id": format!("sec-{:03}", idx),
            "category": "traffic_drops",
            "severity": if drop_count > 50 { "critical" } else { "medium" },
            "title": "Dropped traffic detected",
            "description": format!("{} dropped flows observed in recent traffic", drop_count),
            "resource": "hubble/flows",
            "namespace": "",
            "remediation": "Review dropped flows and add missing allow rules",
            "status": "open",
        }));
    }

    Json(paginate_json(findings, &params, "findings"))
}

pub async fn zero_trust_score(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Compute real zero-trust scores from cluster state
    let policies = state.k8s.list_policies().await.unwrap_or_default();
    let ns_data = state
        .k8s
        .kubectl_json(&["get", "namespaces", "-o", "json"])
        .await;
    let ns_count = ns_data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(1);
    let ns_with_policies: std::collections::HashSet<String> =
        policies.iter().map(|p| p.namespace.clone()).collect();

    // Network segmentation: % of namespaces with policies
    let segmentation = (ns_with_policies.len() as f64 / ns_count as f64 * 100.0).min(100.0) as u32;

    // Identity verification: Cilium always uses identities (score based on identity count)
    let id_data = state
        .k8s
        .kubectl_json(&["get", "ciliumidentities", "-o", "json"])
        .await;
    let id_count = id_data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let identity_score = if id_count > 0 {
        80u32.min(50 + id_count as u32)
    } else {
        20
    };

    // Monitoring: check if hubble is running
    let hubble_ok = state.hubble.is_healthy().await;
    let monitoring = if hubble_ok { 90 } else { 30 };

    let overall = (segmentation + identity_score + monitoring) / 3;

    Json(serde_json::json!({
        "overall": overall,
        "network_segmentation": segmentation,
        "identity_verification": identity_score,
        "monitoring": monitoring,
        "total_policies": policies.len(),
        "total_namespaces": ns_count,
        "total_identities": id_count,
    }))
}

// ── Metrics Summary ─────────────────────────────────────────

pub async fn metrics_summary(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    use crate::services::k8s::K8sService;

    let m = &state.metrics;

    let uptime_str = K8sService::run_cmd("sh", &["-c", "cat /proc/uptime | cut -d' ' -f1"]).await;
    let uptime: f64 = uptime_str.parse().unwrap_or(0.0);

    Json(serde_json::json!({
        "total_requests": m.total_requests.load(Ordering::Relaxed),
        "total_errors": m.total_errors.load(Ordering::Relaxed),
        "total_queries": m.hubble_queries.load(Ordering::Relaxed) + m.k8s_queries.load(Ordering::Relaxed),
        "cache_hits": m.cache_hits.load(Ordering::Relaxed),
        "cache_misses": m.cache_misses.load(Ordering::Relaxed),
        "uptime_seconds": uptime as u64
    }))
}
