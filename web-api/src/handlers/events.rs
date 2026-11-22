// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

use super::{has_namespace_access, jstr, track_request, PaginationQuery};
use crate::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
pub struct K8sEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub reason: String,
    pub object: String,
    pub message: String,
    pub namespace: String,
    pub count: u32,
    pub first_seen: String,
    pub last_seen: String,
}

pub async fn list_events(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Query(params): Query<PaginationQuery>,
) -> Json<serde_json::Value> {
    track_request(&state, |m| {
        m.k8s_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

    let data = state
        .k8s
        .kubectl_json(&[
            "get",
            "events",
            "--all-namespaces",
            "--sort-by=.lastTimestamp",
            "-o",
            "json",
        ])
        .await;

    let all_events: Vec<K8sEvent> = data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    let meta = item.get("metadata").unwrap_or(item);
                    let involved = item.get("involvedObject").unwrap_or(item);

                    let kind = involved.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                    let obj_name = involved.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let object = if kind.is_empty() {
                        obj_name.to_string()
                    } else {
                        format!("{}/{}", kind.to_lowercase(), obj_name)
                    };

                    K8sEvent {
                        id: jstr(meta, "uid"),
                        event_type: jstr(item, "type"),
                        reason: jstr(item, "reason"),
                        object,
                        message: jstr(item, "message"),
                        namespace: jstr(meta, "namespace"),
                        count: item.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as u32,
                        first_seen: jstr(item, "firstTimestamp"),
                        last_seen: jstr(item, "lastTimestamp"),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // Apply namespace RBAC filter
    let all_events: Vec<_> = all_events
        .into_iter()
        .filter(|ev| has_namespace_access(&state, &claims, &ev.namespace))
        .collect();

    let total = all_events.len();
    let offset = params.offset.unwrap_or(0);
    let limit = params.limit.unwrap_or(50).min(500);
    let page: Vec<_> = all_events.into_iter().skip(offset).take(limit).collect();

    Json(serde_json::json!({
        "events": page,
        "total": total,
        "limit": limit,
        "offset": offset,
    }))
}
