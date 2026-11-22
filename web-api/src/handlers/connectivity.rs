// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Connectivity path monitoring handlers (observe-only).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::services::connectivity::{self, CreatePathRequest};
use crate::AppState;

pub async fn list_paths(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    super::check_editor(&state, &claims)?;
    // List is cheap: declared paths only. Status is on-demand via
    // GET /connectivity/paths/{id}/status so probes stay responsive.
    let paths = connectivity::list_paths(&state, &claims).await;
    let with_status: Vec<Value> = paths
        .into_iter()
        .take(100)
        .map(|p| json!({ "path": p, "status": { "status": "pending", "confidence": "unavailable" } }))
        .collect();
    Ok(Json(json!({ "paths": with_status })))
}

pub async fn path_status(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    super::check_editor(&state, &claims)?;
    let key = format!("cv:connectivity_path:{id}");
    let path = state
        .cache
        .get::<Value>(&key)
        .await
        .ok()
        .flatten()
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "path not found" })),
            )
        })?;
    let src = path
        .get("src_namespace")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !super::has_namespace_access(&state, &claims, src) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "namespace not in scope" })),
        ));
    }
    let status = connectivity::status_for_path(&state, &path).await;
    Ok(Json(json!({ "path": path, "status": status })))
}

pub async fn create_path(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(req): Json<CreatePathRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    super::check_editor(&state, &claims)?;
    match connectivity::create_path(&state, &claims, req).await {
        Ok(p) => Ok(Json(serde_json::to_value(p).unwrap_or(json!({})))),
        Err((st, msg)) => Err((st, Json(json!({ "error": msg })))),
    }
}

pub async fn delete_path(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    super::check_editor(&state, &claims)?;
    match connectivity::delete_path(&state, &claims, &id).await {
        Ok(()) => Ok(Json(json!({ "id": id, "status": "deleted" }))),
        Err((st, msg)) => Err((st, Json(json!({ "error": msg })))),
    }
}

pub async fn list_alerts(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    super::check_editor(&state, &claims)?;
    Ok(Json(json!({
        "alerts": connectivity::list_alerts(&state, &claims).await
    })))
}

#[derive(Debug, serde::Deserialize)]
pub struct SilenceRequest {
    #[serde(default = "default_silence_minutes")]
    pub minutes: u64,
}

fn default_silence_minutes() -> u64 {
    60
}

pub async fn silence_alert(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
    Json(req): Json<SilenceRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    super::check_editor(&state, &claims)?;
    match connectivity::silence_alert(&state, &claims, &id, req.minutes).await {
        Ok(v) => Ok(Json(v)),
        Err((st, msg)) => Err((st, Json(json!({ "error": msg })))),
    }
}
