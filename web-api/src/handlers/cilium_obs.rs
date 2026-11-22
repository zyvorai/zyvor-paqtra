// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Cilium / Hubble observability endpoints: Hubble node health, metrics read
// from Prometheus, and read-only queries against the Cilium agent.
//
// Nothing here writes to Cilium. Agent queries run from a fixed allow-list of
// read-only subcommands and take no caller input.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    Json,
};
use serde_json::{json, Value};

use super::track_request;
use crate::error::ApiError;
use crate::services::{cilium_features, cilium_metrics, hubble_grpc};
use crate::AppState;

/// Largest agent output returned to a caller. `endpoint list` on a big node is
/// megabytes; past this the caller should filter at the source instead.
const MAX_AGENT_OUTPUT: usize = 8 * 1024 * 1024;

/// `GET /api/v1/hubble/nodes`: what Hubble Relay (or a single agent) says about
/// its nodes, including ring-buffer fill. A full buffer (`num_flows ==
/// max_flows`) means older flows are already being overwritten.
pub async fn hubble_nodes(State(state): State<Arc<AppState>>) -> Json<Value> {
    track_request(&state, |m| {
        m.hubble_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

    let mut nodes = Vec::new();
    let mut errors = Vec::new();
    for (cluster, address) in &state.config.hubble_addresses {
        match hubble_grpc::nodes(address).await {
            Ok(list) => nodes.extend(list.into_iter().map(|n| {
                let mut v = serde_json::to_value(&n).unwrap_or(Value::Null);
                v["cluster"] = json!(cluster);
                v
            })),
            Err(e) => errors.push(json!({ "cluster": cluster, "error": e.to_string() })),
        }
    }
    Json(json!({
        "available": errors.is_empty(),
        "total": nodes.len(),
        "nodes": nodes,
        "errors": errors,
    }))
}

/// `GET /api/v1/hubble/metrics`
pub async fn hubble_metrics(State(state): State<Arc<AppState>>) -> Json<Value> {
    track_request(&state, |_| {}).await;
    Json(cilium_metrics::collect(&state.prometheus, cilium_metrics::HUBBLE_METRICS).await)
}

/// `GET /api/v1/cilium/metrics`
pub async fn cilium_agent_metrics(State(state): State<Arc<AppState>>) -> Json<Value> {
    track_request(&state, |_| {}).await;
    Json(cilium_metrics::collect(&state.prometheus, cilium_metrics::CILIUM_METRICS).await)
}

/// `GET /api/v1/cilium/features`: which Cilium features `cilium-config` turns on,
/// each with the Paqtra page that shows it. Read-only.
pub async fn cilium_features(State(state): State<Arc<AppState>>) -> Json<Value> {
    track_request(&state, |m| {
        m.k8s_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

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
    let data: std::collections::BTreeMap<String, String> = cm
        .get("data")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                .collect()
        })
        .unwrap_or_default();
    if data.is_empty() {
        return Json(json!({
            "available": false,
            "reason": "could not read the cilium-config ConfigMap in kube-system",
            "features": [],
        }));
    }
    Json(json!({
        "available": true,
        "features": cilium_features::features_from_config(&data),
    }))
}

/// Cilium custom resources `GET /cilium/resources/{kind}` may list, read-only.
const RESOURCE_KINDS: &[(&str, &str)] = &[
    ("nodes", "ciliumnodes"),
    ("egress-gateway-policies", "ciliumegressgatewaypolicies"),
    ("bgp-peering-policies", "ciliumbgppeeringpolicies"),
    ("bgp-cluster-configs", "ciliumbgpclusterconfigs"),
    ("bgp-peer-configs", "ciliumbgppeerconfigs"),
    ("bgp-advertisements", "ciliumbgpadvertisements"),
    ("bgp-node-configs", "ciliumbgpnodeconfigs"),
    ("lb-ip-pools", "ciliumloadbalancerippools"),
    ("l2-announcement-policies", "ciliuml2announcementpolicies"),
    ("pod-ip-pools", "ciliumpodippools"),
    ("cidr-groups", "ciliumcidrgroups"),
    (
        "gateway-classes",
        "gatewayclasses.gateway.networking.k8s.io",
    ),
    ("gateways", "gateways.gateway.networking.k8s.io"),
    ("http-routes", "httproutes.gateway.networking.k8s.io"),
];

fn resource_for_kind(kind: &str) -> Option<&'static str> {
    RESOURCE_KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, r)| *r)
}

/// `GET /api/v1/cilium/resources/{kind}`: list a Cilium (or Cilium-served
/// Gateway API) custom resource across namespaces. Read-only; `spec` and
/// `status` are passed through. A CRD that is not installed lists as empty with
/// `installed: false`.
pub async fn cilium_resources(
    State(state): State<Arc<AppState>>,
    Path(kind): Path<String>,
) -> Result<Json<Value>, ApiError> {
    // Cluster-wide objects. Namespace-scoped users never reach this route: the
    // auth middleware refuses everything outside `SCOPED_ACCESS`.
    let resource = resource_for_kind(&kind).ok_or(ApiError::NotFound)?;
    track_request(&state, |m| {
        m.k8s_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

    let list = state
        .k8s
        .kubectl_json(&["get", resource, "--all-namespaces", "-o", "json"])
        .await;
    let Some(items) = list.get("items").and_then(Value::as_array) else {
        return Ok(Json(
            json!({ "kind": kind, "installed": false, "total": 0, "items": [] }),
        ));
    };
    let items: Vec<Value> = items
        .iter()
        .map(|i| {
            json!({
                "name": i.pointer("/metadata/name"),
                "namespace": i.pointer("/metadata/namespace"),
                "created_at": i.pointer("/metadata/creationTimestamp"),
                "spec": i.get("spec"),
                "status": i.get("status"),
            })
        })
        .collect();
    Ok(Json(
        json!({ "kind": kind, "installed": true, "total": items.len(), "items": items }),
    ))
}

/// The read-only agent subcommands `GET /cilium/agent/{what}` may run.
const AGENT_QUERIES: &[(&str, &[&str])] = &[
    ("endpoints", &["endpoint", "list", "-o", "json"]),
    ("identities", &["identity", "list", "-o", "json"]),
    ("services", &["service", "list", "-o", "json"]),
    ("fqdn-cache", &["fqdn", "cache", "list", "-o", "json"]),
    ("status", &["status", "-o", "json"]),
    ("policy-selectors", &["policy", "selectors", "-o", "json"]),
];

fn agent_query_args(what: &str) -> Option<&'static [&'static str]> {
    AGENT_QUERIES
        .iter()
        .find(|(name, _)| *name == what)
        .map(|(_, args)| *args)
}

/// `GET /api/v1/cilium/agent/{what}`: one of `endpoints`, `identities`,
/// `services`, `fqdn-cache`, `status`, `policy-selectors`, as reported by a Cilium agent. Only one
/// node's agent answers, so this is a sample, not a cluster inventory. Needs
/// the editor role: it reaches into a pod.
pub async fn agent_query(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(what): Path<String>,
) -> Result<Json<Value>, ApiError> {
    super::check_editor(&state, &claims).map_err(|_| ApiError::Forbidden)?;
    let args = agent_query_args(&what).ok_or(ApiError::NotFound)?;
    track_request(&state, |m| {
        m.k8s_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

    let out = state.k8s.cilium_dbg(args).await.map_err(|e| {
        tracing::warn!("cilium agent query {what} failed: {e}");
        ApiError::BadGateway(e.to_string())
    })?;
    if out.len() > MAX_AGENT_OUTPUT {
        return Err(ApiError::BadRequest(format!(
            "output is {} bytes (limit {}); query the agent directly",
            out.len(),
            MAX_AGENT_OUTPUT
        )));
    }
    let data = serde_json::from_str::<Value>(&out).unwrap_or(Value::String(out));
    Ok(Json(json!({
        "what": what,
        "scope": "one Cilium agent (the first pod found)",
        "data": data,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_allow_listed_resources_resolve() {
        assert_eq!(
            resource_for_kind("lb-ip-pools"),
            Some("ciliumloadbalancerippools")
        );
        for bad in [
            "",
            "secrets",
            "pods",
            "ciliumnodes",
            "../x",
            "nodes,secrets",
        ] {
            assert!(resource_for_kind(bad).is_none(), "{bad}");
        }
        // Resource names go to kubectl as one argv entry; none may look like a flag.
        assert!(RESOURCE_KINDS
            .iter()
            .all(|(_, r)| !r.starts_with('-') && !r.contains(' ')));
    }

    #[test]
    fn only_allow_listed_agent_queries_resolve() {
        assert_eq!(
            agent_query_args("endpoints"),
            Some(&["endpoint", "list", "-o", "json"][..])
        );
        for bad in ["", "config", "bpf", "endpoint list", "../etc", "policy"] {
            assert!(agent_query_args(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn agent_queries_are_read_only_subcommands() {
        for (_, args) in AGENT_QUERIES {
            assert!(
                !args
                    .iter()
                    .any(|a| ["delete", "update", "import", "set"].contains(a)),
                "{args:?}"
            );
        }
    }
}
