// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Investigation API: path explain + evidence bundles + export.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::services::investigate::{
    self, bundle_namespaces, bundle_to_markdown, InvestigateFlowRequest, InvestigatePathRequest,
};
use crate::AppState;

/// POST /api/v1/investigate/path
pub async fn investigate_path(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(req): Json<InvestigatePathRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    super::check_editor(&state, &claims)?;

    if req.source.namespace.is_empty() || req.destination.namespace.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "source.namespace and destination.namespace are required"})),
        ));
    }
    if !super::has_namespace_access(&state, &claims, &req.source.namespace)
        || !super::has_namespace_access(&state, &claims, &req.destination.namespace)
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "namespace not in scope"})),
        ));
    }
    if req.port == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "port must be non-zero"})),
        ));
    }

    let result = investigate::investigate_path(&state, &req).await;
    Ok(Json(serde_json::to_value(result).unwrap_or(json!({}))))
}

/// POST /api/v1/investigate/flow — why was this connection denied?
pub async fn investigate_flow(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(req): Json<InvestigateFlowRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    super::check_editor(&state, &claims)?;

    if req.flow_id.is_empty() && req.flow.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "flow_id or flow snapshot is required"})),
        ));
    }

    let result = investigate::investigate_flow(&state, &req).await;
    Ok(Json(serde_json::to_value(result).unwrap_or(json!({}))))
}

/// GET /api/v1/investigate/bundles/{id}
pub async fn get_bundle(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    super::check_editor(&state, &claims)?;

    if id.is_empty() || id.contains('/') || id.contains("..") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid bundle id"})),
        ));
    }

    let key = format!("cv:investigate_bundle:{id}");
    match state.cache.get::<Value>(&key).await {
        Ok(Some(v)) => {
            enforce_bundle_ns(&state, &claims, &v)?;
            Ok(Json(v))
        }
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "bundle not found"})),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    /// `json` (default) or `markdown`
    #[serde(default = "default_format")]
    pub format: String,
}

fn default_format() -> String {
    "json".into()
}

/// GET /api/v1/investigate/bundles/{id}/export?format=json|markdown
pub async fn export_bundle(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
    Query(q): Query<ExportQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    super::check_editor(&state, &claims)?;

    if id.is_empty() || id.contains('/') || id.contains("..") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid bundle id"})),
        ));
    }

    let key = format!("cv:investigate_bundle:{id}");
    let bundle = match state.cache.get::<Value>(&key).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "bundle not found"})),
            ))
        }
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            ))
        }
    };
    enforce_bundle_ns(&state, &claims, &bundle)?;

    let fmt = q.format.to_lowercase();
    if fmt == "markdown" || fmt == "md" {
        // For flow-deny bundles nested under `result`, unwrap for readability.
        let src = bundle
            .get("result")
            .cloned()
            .unwrap_or_else(|| bundle.clone());
        let md = if src.get("steps").is_some() {
            bundle_to_markdown(&json!({
                "id": bundle.get("id").or(src.get("id")),
                "created_at": src.get("created_at"),
                "likely_owner": src.get("likely_owner"),
                "request": src.get("request"),
                "steps": src.get("steps"),
                "source_health": bundle.get("source_health").or(src.get("flow_ingest")),
                "cited_flow_ids": bundle.get("cited_flow_ids").cloned().unwrap_or(json!([])),
                "related_change_ids": bundle.get("related_change_ids").cloned().unwrap_or(json!([])),
            }))
        } else {
            bundle_to_markdown(&bundle)
        };
        return Ok(Json(json!({
            "id": id,
            "format": "markdown",
            "content": md,
            "redaction": "No payloads, argv, or Secret contents included.",
        })));
    }

    Ok(Json(json!({
        "id": id,
        "format": "json",
        "bundle": bundle,
        "redaction": "No payloads, argv, or Secret contents included.",
    })))
}

#[derive(Debug, Deserialize)]
pub struct ShareRequest {
    /// Share lifetime in seconds (default 3600, max 86400).
    #[serde(default = "default_share_ttl")]
    pub ttl_secs: u64,
}

fn default_share_ttl() -> u64 {
    3600
}

/// POST /api/v1/investigate/bundles/{id}/share — time-limited redacted share token.
pub async fn share_bundle(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
    Json(req): Json<ShareRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    super::check_editor(&state, &claims)?;
    if id.is_empty() || id.contains('/') || id.contains("..") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid bundle id"})),
        ));
    }
    let key = format!("cv:investigate_bundle:{id}");
    let bundle = match state.cache.get::<Value>(&key).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "bundle not found"})),
            ))
        }
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            ))
        }
    };
    enforce_bundle_ns(&state, &claims, &bundle)?;

    let ttl = req.ttl_secs.clamp(60, 86_400);
    let token = uuid::Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(ttl as i64);
    let record = json!({
        "token": token,
        "bundle_id": id,
        "expires_at": expires_at.to_rfc3339(),
        "namespaces": crate::services::investigate::bundle_namespaces(&bundle),
        "incident_card": {
            "id": bundle.get("id"),
            "likely_owner": bundle.get("likely_owner").or_else(|| bundle.pointer("/result/likely_owner")),
            "created_at": bundle.get("created_at").or_else(|| bundle.pointer("/result/created_at")),
            "request": bundle.get("request").or_else(|| bundle.pointer("/result/request")),
            "related_change_ids": bundle.get("related_change_ids").cloned().unwrap_or(json!([])),
            "cited_flow_ids": bundle.get("cited_flow_ids").cloned().unwrap_or(json!([])),
            "redaction": "No payloads, argv, or Secret contents included.",
        },
    });
    let share_key = format!("cv:investigate_share:{token}");
    let _ = state.cache.set_durable(&share_key, &record, ttl).await;
    Ok(Json(json!({
        "token": token,
        "expires_at": expires_at.to_rfc3339(),
        "ttl_secs": ttl,
        "path": format!("/api/v1/investigate/share/{token}"),
        "incident_card": record.get("incident_card"),
    })))
}

/// GET /api/v1/investigate/share/{token} — redeem a time-limited share (editor+).
pub async fn get_share(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(token): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    super::check_editor(&state, &claims)?;
    if token.is_empty() || token.contains('/') || token.contains("..") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid share token"})),
        ));
    }
    let share_key = format!("cv:investigate_share:{token}");
    match state.cache.get::<Value>(&share_key).await {
        Ok(Some(v)) => {
            if let Some(nss) = v.get("namespaces").and_then(|x| x.as_array()) {
                let allowed = nss
                    .iter()
                    .filter_map(|n| n.as_str())
                    .any(|ns| super::has_namespace_access(&state, &claims, ns));
                if !nss.is_empty() && !allowed {
                    return Err((
                        StatusCode::FORBIDDEN,
                        Json(json!({"error": "share namespaces not in scope"})),
                    ));
                }
            }
            Ok(Json(v))
        }
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "share not found or expired"})),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )),
    }
}

fn enforce_bundle_ns(
    state: &AppState,
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
    bundle: &Value,
) -> Result<(), (StatusCode, Json<Value>)> {
    let namespaces = bundle_namespaces(bundle);
    if namespaces.is_empty() {
        return Ok(());
    }
    if namespaces
        .iter()
        .any(|ns| super::has_namespace_access(state, claims, ns))
    {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "bundle namespaces not in scope"})),
        ))
    }
}
