// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

use super::{has_namespace_access, jstr, track_request};
use crate::AppState;
use axum::{extract::State, Json};
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
pub struct CiliumEndpoint {
    pub id: String,
    pub name: String,
    pub namespace: String,
    pub identity: u32,
    pub ipv4: String,
    pub ipv6: String,
    pub status: String,
    pub policy_enforcement: String,
    pub labels: Vec<String>,
}

pub async fn list_endpoints(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Json<serde_json::Value> {
    track_request(&state, |m| {
        m.k8s_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

    let data = state
        .k8s
        .kubectl_json(&["get", "ciliumendpoints", "--all-namespaces", "-o", "json"])
        .await;

    let endpoints: Vec<CiliumEndpoint> = data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    let meta = item.get("metadata").unwrap_or(item);
                    let status = item.get("status").unwrap_or(item);
                    let identity_obj = status.get("identity").unwrap_or(status);
                    let networking = status.get("networking").unwrap_or(status);

                    let name = jstr(meta, "name");
                    let namespace = jstr(meta, "namespace");
                    let uid = jstr(meta, "uid");

                    let identity_id =
                        identity_obj.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

                    let labels: Vec<String> = identity_obj
                        .get("labels")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|l| l.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();

                    let ipv4 = networking
                        .get("addressing")
                        .and_then(|v| v.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|a| a.get("ipv4"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let ipv6 = networking
                        .get("addressing")
                        .and_then(|v| v.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|a| a.get("ipv6"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let state_str = status
                        .get("state")
                        .and_then(|v| v.as_str())
                        .unwrap_or("ready")
                        .to_string();

                    let policy = status
                        .get("policy")
                        .and_then(|v| v.get("realized"))
                        .and_then(|v| v.get("policy-enabled"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("default")
                        .to_string();

                    CiliumEndpoint {
                        id: uid,
                        name,
                        namespace,
                        identity: identity_id,
                        ipv4,
                        ipv6,
                        status: state_str,
                        policy_enforcement: policy,
                        labels,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // Apply namespace RBAC filter
    let endpoints: Vec<_> = endpoints
        .into_iter()
        .filter(|ep| has_namespace_access(&state, &claims, &ep.namespace))
        .collect();

    let total = endpoints.len();
    Json(serde_json::json!({ "endpoints": endpoints, "total": total }))
}
