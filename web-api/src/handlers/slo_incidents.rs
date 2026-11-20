// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// SLO targets and incidents.

use super::{
    actor_from_claims, audit_log as emit_audit, paginate_json, track_request, PaginationQuery,
};
use crate::services::incidents;
use crate::services::notifier::is_valid_severity;
use crate::services::slo::{self, Slo, SLOS_PREFIX};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;
type Claims = Option<axum::Extension<crate::middleware::auth::Claims>>;

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": msg })))
}

fn short_id(prefix: &str) -> String {
    format!("{prefix}-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

/// Kubernetes-style DNS label: lowercase alphanumerics and '-', up to 63 chars.
fn valid_namespace(ns: &str) -> bool {
    !ns.is_empty()
        && ns.len() <= 63
        && !ns.starts_with('-')
        && !ns.ends_with('-')
        && ns
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// ── SLOs ────────────────────────────────────────────────────

async fn load_slos(state: &AppState) -> Vec<Slo> {
    let mut slos: Vec<Slo> = state
        .cache
        .list_values(SLOS_PREFIX)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();
    slos.sort_by(|a, b| a.name.cmp(&b.name));
    slos
}

pub async fn list_slos(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<Value> {
    track_request(&state, |_| {}).await;
    let slos = load_slos(&state).await;
    // One flow fetch serves every SLO; skip it when there is nothing to evaluate.
    let flows = if slos.is_empty() {
        Vec::new()
    } else {
        state.hubble.get_flows(500, None).await.unwrap_or_default()
    };
    let evaluated: Vec<Value> = slos.iter().map(|s| slo::evaluate(s, &flows)).collect();
    Json(paginate_json(evaluated, &params, "slos"))
}

#[derive(Debug, Deserialize)]
pub struct CreateSloRequest {
    pub name: String,
    /// Restrict to flows to or from this namespace; omit for all namespaces.
    pub namespace: Option<String>,
    /// Target availability in percent, between 0 and 100 exclusive.
    pub target: f64,
    #[serde(default = "default_window")]
    pub window: String,
}

fn default_window() -> String {
    "30d".to_string()
}

pub async fn create_slo(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Json(req): Json<CreateSloRequest>,
) -> ApiResult {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let name = req.name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "name must be 1-100 characters",
        ));
    }
    if !slo::valid_target(req.target) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "target must be greater than 0 and less than 100",
        ));
    }
    if slo::window_minutes(&req.window).is_none() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "window must be like 12h, 30d or 2w, between 1 hour and 90 days",
        ));
    }
    let namespace = req
        .namespace
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    if namespace.as_deref().is_some_and(|n| !valid_namespace(n)) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "namespace must be a valid Kubernetes name",
        ));
    }
    if load_slos(&state).await.iter().any(|s| s.name == name) {
        return Err(err(
            StatusCode::CONFLICT,
            "An SLO with this name already exists",
        ));
    }

    let created = Slo {
        id: short_id("slo"),
        name: name.to_string(),
        namespace,
        target: req.target,
        window: req.window,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    state
        .cache
        .set_persistent(&format!("{SLOS_PREFIX}{}", created.id), &created)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Failed to store SLO"))?;
    emit_audit(
        &state,
        "slo.create",
        &created.id,
        created.namespace.as_deref().unwrap_or(""),
        &format!(
            "Created SLO '{}' ({}% over {})",
            created.name, created.target, created.window
        ),
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok(Json(serde_json::to_value(&created).unwrap_or_default()))
}

pub async fn delete_slo(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Path(id): Path<String>,
) -> ApiResult {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;
    let key = format!("{SLOS_PREFIX}{id}");
    if state
        .cache
        .get::<Value>(&key)
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return Err(err(StatusCode::NOT_FOUND, "SLO not found"));
    }
    state
        .cache
        .delete(&key)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete SLO"))?;
    emit_audit(
        &state,
        "slo.delete",
        &id,
        "",
        "Deleted SLO",
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok(Json(json!({ "deleted": id })))
}

// ── Incidents ───────────────────────────────────────────────

pub async fn list_incidents(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<Value> {
    track_request(&state, |_| {}).await;
    Json(paginate_json(
        incidents::list(&state).await,
        &params,
        "incidents",
    ))
}

#[derive(Debug, Deserialize)]
pub struct CreateIncidentRequest {
    pub title: String,
    pub severity: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub affected_services: Vec<String>,
}

pub async fn create_incident(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Json(req): Json<CreateIncidentRequest>,
) -> ApiResult {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let title = req.title.trim();
    if title.is_empty() || title.len() > 200 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "title must be 1-200 characters",
        ));
    }
    if !is_valid_severity(&req.severity) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "severity must be one of: critical, high, warning, info",
        ));
    }
    if req.summary.len() > 2000 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "summary must be at most 2000 characters",
        ));
    }
    let services: Vec<String> = req
        .affected_services
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if services.len() > 20 || services.iter().any(|s| s.len() > 100) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "affected_services allows up to 20 entries of at most 100 characters",
        ));
    }

    let actor = actor_from_claims(&claims);
    let summary = if req.summary.trim().is_empty() {
        title
    } else {
        req.summary.trim()
    };
    let incident = incidents::new_incident(
        title,
        &req.severity.to_ascii_lowercase(),
        summary,
        &services,
        None,
        &actor,
        chrono::Utc::now(),
    );
    let incident = incidents::create(&state, incident).await.map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to store incident",
        )
    })?;
    let id = incident["id"].as_str().unwrap_or("").to_string();
    emit_audit(
        &state,
        "incident.create",
        &id,
        "",
        &format!("Opened incident '{title}'"),
        &actor,
        "success",
    )
    .await;
    Ok(Json(incident))
}

/// Map a state-transition refusal to a status code.
fn transition_error(reason: &str) -> (StatusCode, Json<Value>) {
    let status = if reason == "Failed to save incident" {
        StatusCode::INTERNAL_SERVER_ERROR
    } else {
        StatusCode::CONFLICT
    };
    err(status, reason)
}

pub async fn ack_incident(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Path(id): Path<String>,
) -> ApiResult {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;
    let actor = actor_from_claims(&claims);
    let incident = incidents::update(&state, &id, |i| {
        incidents::acknowledge(i, chrono::Utc::now(), &actor)
    })
    .await
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Incident not found"))?
    .map_err(transition_error)?;
    emit_audit(
        &state,
        "incident.ack",
        &id,
        "",
        "Acknowledged incident",
        &actor,
        "success",
    )
    .await;
    Ok(Json(incident))
}

#[derive(Debug, Default, Deserialize)]
pub struct ResolveRequest {
    pub root_cause: Option<String>,
}

pub async fn resolve_incident(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> ApiResult {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;
    let actor = actor_from_claims(&claims);
    // The body is optional: an empty one (with or without a JSON content type)
    // means no root cause. Malformed JSON is still rejected.
    let root_cause = if body.is_empty() {
        None
    } else {
        serde_json::from_slice::<ResolveRequest>(&body)
            .map_err(|_| {
                err(
                    StatusCode::BAD_REQUEST,
                    "Request body must be JSON like {\"root_cause\": \"...\"}",
                )
            })?
            .root_cause
    };
    if root_cause.as_deref().is_some_and(|r| r.len() > 2000) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "root_cause must be at most 2000 characters",
        ));
    }
    let incident = incidents::update(&state, &id, |i| {
        incidents::resolve(
            i,
            chrono::Utc::now(),
            &actor,
            root_cause.as_deref(),
            "Incident resolved",
        )
    })
    .await
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Incident not found"))?
    .map_err(transition_error)?;
    emit_audit(
        &state,
        "incident.resolve",
        &id,
        "",
        "Resolved incident",
        &actor,
        "success",
    )
    .await;
    Ok(Json(incident))
}

#[derive(Debug, Deserialize)]
pub struct NoteRequest {
    pub text: String,
}

/// Add a note to the timeline. Allowed on resolved incidents too, for
/// post-incident notes.
pub async fn add_incident_note(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Path(id): Path<String>,
    Json(req): Json<NoteRequest>,
) -> ApiResult {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;
    let text = req.text.trim();
    if text.is_empty() || text.len() > 2000 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "text must be 1-2000 characters",
        ));
    }
    let actor = actor_from_claims(&claims);
    let incident = incidents::update(&state, &id, |i| {
        incidents::add_event(i, chrono::Utc::now(), text, &actor);
        Ok(())
    })
    .await
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Incident not found"))?
    .map_err(transition_error)?;
    Ok(Json(incident))
}
