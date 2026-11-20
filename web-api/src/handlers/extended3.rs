// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::{actor_from_claims, audit_log, check_admin, track_request};
use crate::AppState;
use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct CreateMirrorRequest {
    #[serde(default = "default_mirror_name")]
    pub name: String,
}

fn default_mirror_name() -> String {
    "new-rule".to_string()
}

#[derive(Debug, Deserialize)]
pub struct TroubleshootRequest {
    #[serde(default = "default_target")]
    pub target: String,
}

fn default_target() -> String {
    "cluster".to_string()
}

// ── Cost Breakdown ─────────────────────────────────────────

pub async fn cost_breakdown(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Query all pods to compute resource requests per namespace
    let data = state
        .k8s
        .kubectl_json(&["get", "pods", "--all-namespaces", "-o", "json"])
        .await;

    let mut ns_resources: std::collections::HashMap<String, (f64, f64, u64)> =
        std::collections::HashMap::new(); // (cpu_millicores, memory_mib, pod_count)

    if let Some(items) = data.get("items").and_then(|v| v.as_array()) {
        for item in items {
            let ns = item
                .get("metadata")
                .and_then(|m| m.get("namespace"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let containers = item
                .get("spec")
                .and_then(|s| s.get("containers"))
                .and_then(|v| v.as_array());
            let mut pod_cpu: f64 = 0.0;
            let mut pod_mem: f64 = 0.0;
            if let Some(containers) = containers {
                for c in containers {
                    let resources = c.get("resources").and_then(|r| r.get("requests"));
                    if let Some(req) = resources {
                        if let Some(cpu_str) = req.get("cpu").and_then(|v| v.as_str()) {
                            pod_cpu += parse_cpu_millis(cpu_str);
                        }
                        if let Some(mem_str) = req.get("memory").and_then(|v| v.as_str()) {
                            pod_mem += parse_memory_mib(mem_str);
                        }
                    }
                }
            }
            let entry = ns_resources.entry(ns).or_insert((0.0, 0.0, 0));
            entry.0 += pod_cpu;
            entry.1 += pod_mem;
            entry.2 += 1;
        }
    }

    let total_cpu: f64 = ns_resources.values().map(|v| v.0).sum();
    let total_mem: f64 = ns_resources.values().map(|v| v.1).sum();

    let mut namespaces: Vec<serde_json::Value> = ns_resources
        .iter()
        .map(|(ns, (cpu, mem, pods))| {
            let cpu_share = if total_cpu > 0.0 {
                cpu / total_cpu
            } else {
                0.0
            };
            let mem_share = if total_mem > 0.0 {
                mem / total_mem
            } else {
                0.0
            };
            serde_json::json!({
                "namespace": ns,
                "pod_count": pods,
                "cpu_request_millicores": *cpu as u64,
                "memory_request_mib": *mem as u64,
                "cpu_share_percent": (cpu_share * 100.0 * 10.0).round() / 10.0,
                "memory_share_percent": (mem_share * 100.0 * 10.0).round() / 10.0,
                "relative_weight": ((cpu_share + mem_share) / 2.0 * 100.0 * 10.0).round() / 10.0,
            })
        })
        .collect();
    namespaces.sort_by(|a, b| {
        let wa = a
            .get("relative_weight")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let wb = b
            .get("relative_weight")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        wb.partial_cmp(&wa).unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_pods: u64 = ns_resources.values().map(|v| v.2).sum();
    Json(serde_json::json!({
        "namespaces": namespaces,
        "summary": {
            "total_namespaces": ns_resources.len(),
            "total_pods": total_pods,
            "total_cpu_request_millicores": total_cpu as u64,
            "total_memory_request_mib": total_mem as u64,
            "note": "Resource-proportional breakdown from pod specs; actual cloud costs require billing API integration",
            "source": "kubernetes pod resource requests",
        }
    }))
}

/// Parse a Kubernetes CPU quantity string (e.g. "100m", "0.5", "2") into millicores.
fn parse_cpu_millis(s: &str) -> f64 {
    if let Some(m) = s.strip_suffix('m') {
        m.parse::<f64>().unwrap_or(0.0)
    } else {
        s.parse::<f64>().unwrap_or(0.0) * 1000.0
    }
}

/// Parse a Kubernetes memory quantity string (e.g. "128Mi", "1Gi", "256000Ki") into MiB.
fn parse_memory_mib(s: &str) -> f64 {
    if let Some(v) = s.strip_suffix("Gi") {
        v.parse::<f64>().unwrap_or(0.0) * 1024.0
    } else if let Some(v) = s.strip_suffix("Mi") {
        v.parse::<f64>().unwrap_or(0.0)
    } else if let Some(v) = s.strip_suffix("Ki") {
        v.parse::<f64>().unwrap_or(0.0) / 1024.0
    } else if let Some(v) = s.strip_suffix('G') {
        v.parse::<f64>().unwrap_or(0.0) * 1000.0 / 1.048576
    } else if let Some(v) = s.strip_suffix('M') {
        v.parse::<f64>().unwrap_or(0.0) * 1000.0 / 1048.576
    } else {
        // Plain bytes
        s.parse::<f64>().unwrap_or(0.0) / (1024.0 * 1024.0)
    }
}

// ── Forecast Metrics ───────────────────────────────────────

pub async fn forecast_metrics(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;
    Json(serde_json::json!({
        "metrics": ["cpu_usage", "memory_usage", "network_throughput", "pod_count"]
    }))
}

// ── Forecast Data ──────────────────────────────────────────

pub async fn forecast_data(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(metric): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    let now = chrono::Utc::now();

    // If Prometheus is configured, use query_range for historical metric data
    if state.prometheus.is_configured() {
        let promql = match metric.as_str() {
            "cpu_usage" => "rate(container_cpu_usage_seconds_total[5m])",
            "memory_usage" => "container_memory_working_set_bytes",
            "network_throughput" => "rate(container_network_transmit_bytes_total[5m])",
            "pod_count" => "count(kube_pod_info)",
            _ => "up",
        };

        let end = now.to_rfc3339();
        let start = (now - chrono::Duration::hours(24)).to_rfc3339();
        let step = "300"; // 5-minute intervals

        if let Ok(result) = state
            .prometheus
            .query_range(promql, &start, &end, step)
            .await
        {
            // Extract data points from the Prometheus range response
            let points: Vec<serde_json::Value> = result
                .get("data")
                .and_then(|d| d.get("result"))
                .and_then(|r| r.as_array())
                .and_then(|arr| arr.first())
                .and_then(|series| series.get("values"))
                .and_then(|v| v.as_array())
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|pair| {
                            let arr = pair.as_array()?;
                            let ts = arr.first()?.as_f64()?;
                            let val = arr.get(1)?.as_str()?.parse::<f64>().ok()?;
                            Some(serde_json::json!({
                                "timestamp": ts,
                                "value": val,
                            }))
                        })
                        .collect()
                })
                .unwrap_or_default();

            return Json(serde_json::json!({
                "metric": metric,
                "data_type": "prometheus_range",
                "source": "prometheus",
                "promql": promql,
                "total_points": points.len(),
                "points": points,
                "observed_at": now.to_rfc3339(),
            }));
        }
        // If the Prometheus query failed, fall through to Hubble-based aggregation
    }

    // Fallback: fetch recent Hubble flows and aggregate counts by hour
    let flows = state.hubble.get_flows(5000, None).await.unwrap_or_default();

    // Bucket flows by hour based on their timestamps
    let mut hourly_counts: std::collections::BTreeMap<String, u64> =
        std::collections::BTreeMap::new();
    let mut hourly_dropped: std::collections::BTreeMap<String, u64> =
        std::collections::BTreeMap::new();

    for flow in &flows {
        // Parse the flow timestamp and truncate to hour
        let hour_key = if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&flow.timestamp) {
            ts.format("%Y-%m-%dT%H:00:00Z").to_string()
        } else if flow.timestamp.len() >= 13 {
            // Try truncating to hour even if not perfectly parseable
            format!("{}:00:00Z", &flow.timestamp[..13])
        } else {
            continue;
        };
        *hourly_counts.entry(hour_key.clone()).or_insert(0) += 1;
        if flow.verdict == "DROPPED" {
            *hourly_dropped.entry(hour_key).or_insert(0) += 1;
        }
    }

    let points: Vec<serde_json::Value> = hourly_counts
        .iter()
        .map(|(hour, count)| {
            let drops = hourly_dropped.get(hour).copied().unwrap_or(0);
            serde_json::json!({
                "timestamp": hour,
                "flow_count": count,
                "dropped_count": drops,
                "forwarded_count": count - drops,
            })
        })
        .collect();

    let total_flows: u64 = hourly_counts.values().sum();
    let total_hours = hourly_counts.len();

    Json(serde_json::json!({
        "metric": metric,
        "data_type": "actual_observations",
        "source": "hubble flow counts aggregated by hour",
        "total_flows_observed": total_flows,
        "time_buckets": total_hours,
        "points": points,
        "observed_at": now.to_rfc3339(),
        "note": "Forecasting (predicted/confidence intervals) requires Prometheus or an external time-series model; showing recent actual flow data",
    }))
}

// ── Encryption Status ──────────────────────────────────────

pub async fn encryption_status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Query Cilium ConfigMap for encryption settings
    let cm = state
        .k8s
        .kubectl_json(&[
            "get",
            "configmap",
            "cilium-config",
            "-n",
            "kube-system",
            "-o",
            "json",
        ])
        .await;

    let empty = serde_json::json!({});
    let data = cm.get("data").unwrap_or(&empty);
    let enc_type = data
        .get("encrypt-node")
        .and_then(|v| v.as_str())
        .or_else(|| data.get("encryption.type").and_then(|v| v.as_str()))
        .unwrap_or("disabled")
        .to_string();
    let enabled = enc_type != "disabled" && !enc_type.is_empty();

    // Count nodes
    let nodes = state
        .k8s
        .kubectl_json(&["get", "nodes", "-o", "json"])
        .await;
    let nodes_total = nodes
        .get("items")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    let nodes_encrypted = if enabled { nodes_total } else { 0 };

    Json(serde_json::json!({
        "enabled": enabled,
        "type": if enabled { &enc_type } else { "none" },
        "nodes_encrypted": nodes_encrypted,
        "nodes_total": nodes_total,
        "interfaces": [],
        "key_rotation": null,
        "stats": {
            "bytes_encrypted": 0,
            "bytes_decrypted": 0,
        },
        "config_source": "cilium-config ConfigMap",
    }))
}

// ── Load Balancer Services ─────────────────────────────────

pub async fn lb_services(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Query real K8s Service resources of type LoadBalancer and ClusterIP
    let data = state
        .k8s
        .kubectl_json(&["get", "services", "--all-namespaces", "-o", "json"])
        .await;

    let services: Vec<serde_json::Value> = data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    let meta = item.get("metadata").unwrap_or(item);
                    let spec = item.get("spec").unwrap_or(item);
                    let name = meta.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let ns = meta.get("namespace").and_then(|v| v.as_str()).unwrap_or("");
                    let svc_type = spec
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("ClusterIP");
                    let cluster_ip = spec.get("clusterIP").and_then(|v| v.as_str()).unwrap_or("");
                    let ports: Vec<serde_json::Value> = spec
                        .get("ports")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let session_affinity = spec
                        .get("sessionAffinity")
                        .and_then(|v| v.as_str())
                        .unwrap_or("None");

                    serde_json::json!({
                        "name": name,
                        "namespace": ns,
                        "type": svc_type,
                        "cluster_ip": cluster_ip,
                        "ports": ports,
                        "session_affinity": session_affinity,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let total = services.len();
    Json(serde_json::json!({ "services": services, "total": total }))
}

// ── Ingress Routes ─────────────────────────────────────────

pub async fn ingress_routes(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    let data = state
        .k8s
        .kubectl_json(&["get", "ingress", "--all-namespaces", "-o", "json"])
        .await;

    let routes: Vec<serde_json::Value> = data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|items| {
            items.iter().map(|item| {
                let meta = item.get("metadata").unwrap_or(item);
                let spec = item.get("spec").unwrap_or(item);
                let tls = spec.get("tls").cloned().unwrap_or(serde_json::json!([]));
                let rules = spec.get("rules").cloned().unwrap_or(serde_json::json!([]));

                serde_json::json!({
                    "name": meta.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "namespace": meta.get("namespace").and_then(|v| v.as_str()).unwrap_or(""),
                    "rules": rules,
                    "tls": tls,
                    "created_at": meta.get("creationTimestamp").and_then(|v| v.as_str()).unwrap_or(""),
                })
            }).collect()
        })
        .unwrap_or_default();

    let total = routes.len();
    Json(serde_json::json!({ "routes": routes, "total": total }))
}

// ── IPAM Pools ─────────────────────────────────────────────

pub async fn ipam_pools(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Query CiliumNode resources for IPAM pool info
    let data = state
        .k8s
        .kubectl_json(&["get", "ciliumnodes", "-o", "json"])
        .await;

    let pools: Vec<serde_json::Value> = data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let meta = item.get("metadata")?;
                    let spec = item.get("spec")?;
                    let ipam = spec.get("ipam")?;
                    let name = meta.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let cidrs: Vec<String> = ipam
                        .get("podCIDRs")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(serde_json::json!({
                        "node": name,
                        "pod_cidrs": cidrs,
                        "ipam": ipam,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    Json(serde_json::json!({ "pools": pools, "total": pools.len() }))
}

// ── IP Allocations ─────────────────────────────────────────

pub async fn ip_allocations(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Get pod IPs from running pods
    let data = state
        .k8s
        .kubectl_json(&["get", "pods", "--all-namespaces", "-o", "json"])
        .await;

    let allocations: Vec<serde_json::Value> =
        data.get("items")
            .and_then(|v| v.as_array())
            .map(|items| {
                items.iter().filter_map(|item| {
                let meta = item.get("metadata")?;
                let status = item.get("status")?;
                let pod_ip = status.get("podIP").and_then(|v| v.as_str())?;
                if pod_ip.is_empty() { return None; }
                Some(serde_json::json!({
                    "ip": pod_ip,
                    "pod": meta.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "namespace": meta.get("namespace").and_then(|v| v.as_str()).unwrap_or(""),
                    "node": status.get("hostIP").and_then(|v| v.as_str()).unwrap_or(""),
                    "phase": status.get("phase").and_then(|v| v.as_str()).unwrap_or("Unknown"),
                }))
            }).collect()
            })
            .unwrap_or_default();

    let total = allocations.len();
    Json(serde_json::json!({ "allocations": allocations, "total": total }))
}

// ── Latency Analysis ───────────────────────────────────────

pub async fn latency_analysis(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // If Prometheus is configured, query real latency percentiles
    if state.prometheus.is_configured() {
        let p50 = state
            .prometheus
            .get_metric_value(
                "histogram_quantile(0.5, rate(hubble_flows_processed_duration_seconds_bucket[5m]))",
            )
            .await;
        let p95 = state
            .prometheus
            .get_metric_value(
                "histogram_quantile(0.95, rate(hubble_flows_processed_duration_seconds_bucket[5m]))",
            )
            .await;
        let p99 = state
            .prometheus
            .get_metric_value(
                "histogram_quantile(0.99, rate(hubble_flows_processed_duration_seconds_bucket[5m]))",
            )
            .await;

        return Json(serde_json::json!({
            "latency": {
                "p50_seconds": p50,
                "p95_seconds": p95,
                "p99_seconds": p99,
            },
            "source": "prometheus",
            "query_window": "5m",
        }));
    }

    // Fallback: derive per-service traffic volume from Hubble flows
    // Note: L4 flows don't include latency; we report flow counts as a traffic volume proxy
    let flows = state.hubble.get_flows(500, None).await.unwrap_or_default();

    let mut svc_flows: std::collections::HashMap<(String, String), (u64, u64)> =
        std::collections::HashMap::new();
    for flow in &flows {
        let svc = if !flow.destination.pod.is_empty() {
            flow.destination
                .pod
                .split('-')
                .take(2)
                .collect::<Vec<_>>()
                .join("-")
        } else {
            continue;
        };
        let ns = if flow.destination.namespace.is_empty() {
            "unknown"
        } else {
            &flow.destination.namespace
        };
        let entry = svc_flows.entry((svc, ns.to_string())).or_default();
        entry.0 += 1; // total
        if flow.verdict == "DROPPED" {
            entry.1 += 1;
        } // errors
    }

    let services: Vec<serde_json::Value> = svc_flows
        .into_iter()
        .map(|((svc, ns), (total, errors))| {
            serde_json::json!({
                "service": svc,
                "namespace": ns,
                "sample_count": total,
                "error_count": errors,
                "error_rate": if total > 0 { errors as f64 / total as f64 * 100.0 } else { 0.0 },
                "note": "Latency requires L7/Prometheus metrics; showing flow volume",
            })
        })
        .collect();

    Json(serde_json::json!({ "services": services, "source": "hubble L4 flow counts" }))
}

// ── Traffic Mirror Rules ───────────────────────────────────

const MIRROR_RULES_PREFIX: &str = "cv:mirror_rules:";

pub async fn mirror_rules(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;
    let items = state
        .cache
        .list_values(MIRROR_RULES_PREFIX)
        .await
        .unwrap_or_default();
    Json(serde_json::json!({ "rules": items, "total": items.len() }))
}

pub async fn create_mirror_rule(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(_body): Json<CreateMirrorRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;
    Err(super::not_implemented(
        "Traffic mirroring",
        "no mirroring was configured, so no rule was saved",
    ))
}

pub async fn delete_mirror_rule(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let _ = state
        .cache
        .delete(&format!("{}{}", MIRROR_RULES_PREFIX, id))
        .await;
    audit_log(
        &state,
        "mirror.delete",
        &id,
        "",
        "Mirror rule deleted",
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok(Json(serde_json::json!({
        "id": id,
        "status": "deleted",
        "message": "Mirror rule deleted"
    })))
}

// ── Cluster Health ─────────────────────────────────────────

pub async fn cluster_health(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    const CACHE_KEY: &str = "cluster_health";
    const CACHE_TTL_SECS: u64 = 15;

    if let Ok(Some(cached)) = state.cache.get::<serde_json::Value>(CACHE_KEY).await {
        state
            .metrics
            .cache_hits
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return Json(cached);
    }
    state
        .metrics
        .cache_misses
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    // Parallel kubectl probes — sequential calls previously stacked up to ~45s.
    let (cilium_pods, relay, k8s_healthy, nodes) = tokio::join!(
        state.k8s.kubectl_json(&[
            "get",
            "pods",
            "-n",
            "kube-system",
            "-l",
            "k8s-app=cilium",
            "-o",
            "json",
        ]),
        state.k8s.kubectl_json(&[
            "get",
            "pods",
            "-n",
            "kube-system",
            "-l",
            "k8s-app=hubble-relay",
            "-o",
            "json",
        ]),
        state.k8s.is_healthy(),
        state.k8s.kubectl_json(&["get", "nodes", "-o", "json"]),
    );

    let mut components = Vec::new();
    let agents = cilium_pods.get("items").and_then(|v| v.as_array());
    let (total, ready) = agents
        .map(|arr| {
            let t = arr.len();
            let r = arr
                .iter()
                .filter(|p| {
                    p.get("status")
                        .and_then(|s| s.get("containerStatuses"))
                        .and_then(|v| v.as_array())
                        .map(|cs| {
                            cs.iter()
                                .all(|c| c.get("ready").and_then(|v| v.as_bool()).unwrap_or(false))
                        })
                        .unwrap_or(false)
                })
                .count();
            (t, r)
        })
        .unwrap_or((0, 0));

    components.push(serde_json::json!({
        "name": "cilium-agent",
        "status": if total == ready && total > 0 { "healthy" } else { "degraded" },
        "instances": total,
        "ready": ready,
    }));

    let relay_items = relay.get("items").and_then(|v| v.as_array());
    let (rt, rr) = relay_items
        .map(|arr| {
            (
                arr.len(),
                arr.iter()
                    .filter(|p| {
                        p.get("status")
                            .and_then(|s| s.get("phase"))
                            .and_then(|v| v.as_str())
                            == Some("Running")
                    })
                    .count(),
            )
        })
        .unwrap_or((0, 0));
    components.push(serde_json::json!({
        "name": "hubble-relay", "instances": rt, "ready": rr,
        "status": if rt == rr && rt > 0 { "healthy" } else if rt > 0 { "degraded" } else { "not_found" },
    }));

    let node_items = nodes.get("items").and_then(|v| v.as_array());
    let nodes_total = node_items.map(|a| a.len()).unwrap_or(0);
    let nodes_ready = node_items
        .map(|arr| {
            arr.iter()
                .filter(|n| {
                    n.get("status")
                        .and_then(|s| s.get("conditions"))
                        .and_then(|v| v.as_array())
                        .and_then(|conds| {
                            conds
                                .iter()
                                .find(|c| c.get("type").and_then(|v| v.as_str()) == Some("Ready"))
                        })
                        .and_then(|c| c.get("status"))
                        .and_then(|v| v.as_str())
                        == Some("True")
                })
                .count()
        })
        .unwrap_or(0);

    let overall = if nodes_ready == nodes_total && nodes_total > 0 && k8s_healthy {
        "healthy"
    } else {
        "degraded"
    };

    // Simple score for Overview digest (100 when healthy, else proportion ready).
    let score = (nodes_ready * 100).checked_div(nodes_total).unwrap_or(0) as u64;

    let body = serde_json::json!({
        "status": overall,
        "score": score,
        "components": components,
        "kubernetes": {
            "healthy": k8s_healthy,
            "nodes_total": nodes_total,
            "nodes_ready": nodes_ready,
        },
        "last_check": chrono::Utc::now().to_rfc3339(),
        "cached": false,
    });

    let _ = state.cache.set(CACHE_KEY, &body, CACHE_TTL_SECS).await;
    Json(body)
}

// ── RBAC Bindings ──────────────────────────────────────────

pub async fn rbac_bindings(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let data = state
        .k8s
        .kubectl_json(&[
            "get",
            "clusterrolebindings",
            "-o",
            "json",
            "-l",
            "app.kubernetes.io/part-of=cilium",
        ])
        .await;

    let bindings: Vec<serde_json::Value> = data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|items| {
            items.iter().map(|item| {
                let meta = item.get("metadata").unwrap_or(item);
                let role_ref = item.get("roleRef").unwrap_or(item);
                let subjects: Vec<serde_json::Value> = item
                    .get("subjects")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                serde_json::json!({
                    "name": meta.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "type": "ClusterRoleBinding",
                    "role": role_ref.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "role_kind": role_ref.get("kind").and_then(|v| v.as_str()).unwrap_or(""),
                    "subjects": subjects,
                    "created_at": meta.get("creationTimestamp").and_then(|v| v.as_str()).unwrap_or(""),
                })
            }).collect()
        })
        .unwrap_or_default();

    // If no cilium-specific bindings found, get all
    if bindings.is_empty() {
        let all_data = state
            .k8s
            .kubectl_json(&["get", "clusterrolebindings", "-o", "json"])
            .await;
        let all_bindings: Vec<serde_json::Value> = all_data
            .get("items")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .take(20)
                    .map(|item| {
                        let meta = item.get("metadata").unwrap_or(item);
                        let role_ref = item.get("roleRef").unwrap_or(item);
                        serde_json::json!({
                            "name": meta.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            "type": "ClusterRoleBinding",
                            "role": role_ref.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        return Ok(Json(
            serde_json::json!({ "bindings": all_bindings, "total": all_bindings.len() }),
        ));
    }

    let total = bindings.len();
    Ok(Json(
        serde_json::json!({ "bindings": bindings, "total": total }),
    ))
}

// ── Network Interfaces ─────────────────────────────────────

pub async fn net_interfaces(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;

    // Get network interfaces from local system via /proc/net/dev
    use crate::services::k8s::K8sService;
    let proc_net = K8sService::run_cmd("sh", &["-c", "cat /proc/net/dev"]).await;

    let mut interfaces = Vec::new();
    for line in proc_net.lines().skip(2) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 11 {
            continue;
        }
        let name = parts[0].trim_end_matches(':');
        interfaces.push(serde_json::json!({
            "name": name,
            "rx_bytes": parts[1].parse::<u64>().unwrap_or(0),
            "rx_packets": parts[2].parse::<u64>().unwrap_or(0),
            "rx_errors": parts[3].parse::<u64>().unwrap_or(0),
            "rx_dropped": parts[4].parse::<u64>().unwrap_or(0),
            "tx_bytes": parts[9].parse::<u64>().unwrap_or(0),
            "tx_packets": parts[10].parse::<u64>().unwrap_or(0),
            "tx_errors": parts.get(11).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
            "tx_dropped": parts.get(12).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
        }));
    }

    Ok(Json(
        serde_json::json!({ "interfaces": interfaces, "total": interfaces.len(), "source": "local /proc/net/dev" }),
    ))
}

// ── Troubleshoot ───────────────────────────────────────────

pub async fn run_troubleshoot(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(body): Json<TroubleshootRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;
    let target = &body.target;

    use crate::services::k8s::K8sService;
    let mut steps = Vec::new();
    let mut passed = 0u32;
    let mut warnings = 0u32;
    let mut failed = 0u32;

    // Step 1: K8s API
    let k8s_ok = state.k8s.is_healthy().await;
    if k8s_ok {
        passed += 1;
    } else {
        failed += 1;
    }
    steps.push(serde_json::json!({
        "step": 1, "name": "Kubernetes API", "status": if k8s_ok { "pass" } else { "fail" },
        "output": if k8s_ok { "API server reachable" } else { "API server unreachable" },
    }));

    // Step 2: Cilium agents
    let agent_pods = state
        .k8s
        .kubectl_json(&[
            "get",
            "pods",
            "-n",
            "kube-system",
            "-l",
            "k8s-app=cilium",
            "-o",
            "json",
        ])
        .await;
    let agent_count = agent_pods
        .get("items")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    if agent_count > 0 {
        passed += 1;
    } else {
        failed += 1;
    }
    steps.push(serde_json::json!({
        "step": 2, "name": "Cilium Agents", "status": if agent_count > 0 { "pass" } else { "fail" },
        "output": format!("{} cilium agent pods found", agent_count),
    }));

    // Step 3: Hubble
    let hubble_ok = state.hubble.is_healthy().await;
    if hubble_ok {
        passed += 1;
    } else {
        warnings += 1;
    }
    steps.push(serde_json::json!({
        "step": 3, "name": "Hubble Relay", "status": if hubble_ok { "pass" } else { "warn" },
        "output": if hubble_ok { format!("Connected to {}", state.hubble.address()) } else { "Relay unreachable".to_string() },
    }));

    // Step 4: Network policies
    let policies = state.k8s.list_policies().await.unwrap_or_default();
    if !policies.is_empty() {
        passed += 1;
    } else {
        warnings += 1;
    }
    steps.push(serde_json::json!({
        "step": 4, "name": "Network Policies", "status": if !policies.is_empty() { "pass" } else { "warn" },
        "output": format!("{} CiliumNetworkPolicies active", policies.len()),
    }));

    // Step 5: BPF filesystem
    let bpf_mount = K8sService::run_cmd(
        "sh",
        &["-c", "mountpoint -q /sys/fs/bpf && echo yes || echo no"],
    )
    .await;
    let bpf_ok = bpf_mount.trim() == "yes";
    if bpf_ok {
        passed += 1;
    } else {
        warnings += 1;
    }
    steps.push(serde_json::json!({
        "step": 5, "name": "BPF Filesystem", "status": if bpf_ok { "pass" } else { "warn" },
        "output": if bpf_ok { "/sys/fs/bpf mounted" } else { "/sys/fs/bpf not available" },
    }));

    // Step 6: Flows check
    let flows = state.hubble.get_flows(100, None).await.unwrap_or_default();
    let drops = flows.iter().filter(|f| f.verdict == "DROPPED").count();
    if drops == 0 {
        passed += 1;
    } else {
        warnings += 1;
    }
    steps.push(serde_json::json!({
        "step": 6, "name": "Recent Traffic Health", "status": if drops == 0 { "pass" } else { "warn" },
        "output": format!("{} flows observed, {} drops", flows.len(), drops),
    }));

    let total_steps = steps.len() as u32;
    Ok(Json(serde_json::json!({
        "target": target,
        "started_at": chrono::Utc::now().to_rfc3339(),
        "status": "completed",
        "steps": steps,
        "summary": {
            "total_steps": total_steps,
            "passed": passed,
            "warnings": warnings,
            "failed": failed,
        }
    })))
}
