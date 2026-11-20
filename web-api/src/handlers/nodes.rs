// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::{jstr, track_request};
use crate::AppState;
use axum::{extract::State, Json};
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
pub struct K8sNode {
    pub name: String,
    pub status: String,
    pub roles: Vec<String>,
    pub version: String,
    pub os: String,
    pub kernel: String,
    pub cpu_capacity: u32,
    pub memory_capacity_gb: f64,
    pub pods_count: u32,
    pub pods_capacity: u32,
    pub age: String,
}

pub async fn list_nodes(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |m| {
        m.k8s_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

    let data = state
        .k8s
        .kubectl_json(&["get", "nodes", "-o", "json"])
        .await;

    let nodes: Vec<K8sNode> = data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    let meta = item.get("metadata").unwrap_or(item);
                    let status = item.get("status").unwrap_or(item);
                    let node_info = status.get("nodeInfo").unwrap_or(status);
                    let capacity = status.get("capacity").unwrap_or(status);

                    let name = jstr(meta, "name");
                    let created = jstr(meta, "creationTimestamp");

                    // Parse roles from labels
                    let roles: Vec<String> = meta
                        .get("labels")
                        .and_then(|v| v.as_object())
                        .map(|labels| {
                            labels
                                .keys()
                                .filter_map(|k| {
                                    k.strip_prefix("node-role.kubernetes.io/").map(String::from)
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    // Parse conditions for Ready status
                    let ready = status
                        .get("conditions")
                        .and_then(|v| v.as_array())
                        .and_then(|conds| {
                            conds
                                .iter()
                                .find(|c| c.get("type").and_then(|v| v.as_str()) == Some("Ready"))
                        })
                        .and_then(|c| c.get("status"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown");
                    let status_str = if ready == "True" { "Ready" } else { "NotReady" };

                    let cpu = capacity
                        .get("cpu")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0);

                    let mem_str = capacity
                        .get("memory")
                        .and_then(|v| v.as_str())
                        .unwrap_or("0");
                    let mem_gb = parse_k8s_memory(mem_str);

                    let pods_cap = capacity
                        .get("pods")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0);

                    K8sNode {
                        name,
                        status: status_str.to_string(),
                        roles,
                        version: jstr(node_info, "kubeletVersion"),
                        os: jstr(node_info, "osImage"),
                        kernel: jstr(node_info, "kernelVersion"),
                        cpu_capacity: cpu,
                        memory_capacity_gb: mem_gb,
                        pods_count: 0, // Would need separate pod listing per node
                        pods_capacity: pods_cap,
                        age: created,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    Json(serde_json::json!({ "nodes": nodes, "total": nodes.len() }))
}

/// Parse Kubernetes memory strings like "16384Ki", "8Gi" to GB
fn parse_k8s_memory(s: &str) -> f64 {
    if let Some(ki) = s.strip_suffix("Ki") {
        ki.parse::<f64>().unwrap_or(0.0) / (1024.0 * 1024.0)
    } else if let Some(mi) = s.strip_suffix("Mi") {
        mi.parse::<f64>().unwrap_or(0.0) / 1024.0
    } else if let Some(gi) = s.strip_suffix("Gi") {
        gi.parse::<f64>().unwrap_or(0.0)
    } else {
        s.parse::<f64>().unwrap_or(0.0) / (1024.0 * 1024.0 * 1024.0)
    }
}
