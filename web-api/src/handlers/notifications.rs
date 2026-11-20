// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Alert notification channels and silences (admin only).

use super::{actor_from_claims, audit_log as emit_audit, check_admin, track_request};
use crate::services::notifier::{
    self, AlertEvent, Channel, ChannelKind, EventKind, CHANNELS_PREFIX, SILENCES_PREFIX,
};
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

type ApiResult = Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>;
type Claims = Option<axum::Extension<crate::middleware::auth::Claims>>;

fn bad_request(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg })),
    )
}

fn not_found(what: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": format!("{what} not found") })),
    )
}

fn internal(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": msg })),
    )
}

// ── Channels ────────────────────────────────────────────────

pub async fn list_channels(State(state): State<Arc<AppState>>, claims: Claims) -> ApiResult {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;
    let channels: Vec<_> = notifier::list_channels(&state)
        .await
        .iter()
        .map(Channel::masked)
        .collect();
    Ok(Json(
        serde_json::json!({ "channels": channels, "total": channels.len() }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    pub kind: ChannelKind,
    pub target: String,
    pub min_severity: Option<String>,
    pub enabled: Option<bool>,
}

pub async fn create_channel(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Json(req): Json<CreateChannelRequest>,
) -> ApiResult {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let name = req.name.trim();
    let target = req.target.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(bad_request("name must be 1-100 characters"));
    }
    if target.is_empty() {
        return Err(bad_request("target is required"));
    }
    if req.kind != ChannelKind::Pagerduty
        && !(target.starts_with("https://") || target.starts_with("http://"))
    {
        return Err(bad_request("target must be an http(s) URL"));
    }
    if let Some(sev) = req.min_severity.as_deref() {
        if !notifier::is_valid_severity(sev) {
            return Err(bad_request(
                "min_severity must be one of: critical, high, warning, info",
            ));
        }
    }

    let channel = Channel {
        id: format!("ch-{}", &uuid::Uuid::new_v4().to_string()[..8]),
        name: name.to_string(),
        kind: req.kind,
        target: target.to_string(),
        min_severity: req.min_severity.map(|s| s.to_ascii_lowercase()),
        enabled: req.enabled.unwrap_or(true),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    state
        .cache
        .set_persistent(&format!("{}{}", CHANNELS_PREFIX, channel.id), &channel)
        .await
        .map_err(|_| internal("failed to store channel"))?;

    emit_audit(
        &state,
        "alert.channel.create",
        &channel.id,
        "",
        &format!("Created {:?} channel '{}'", channel.kind, channel.name),
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok(Json(channel.masked()))
}

pub async fn delete_channel(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Path(id): Path<String>,
) -> ApiResult {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let key = format!("{}{}", CHANNELS_PREFIX, id);
    if state
        .cache
        .get::<serde_json::Value>(&key)
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return Err(not_found("channel"));
    }
    state
        .cache
        .delete(&key)
        .await
        .map_err(|_| internal("failed to delete channel"))?;
    emit_audit(
        &state,
        "alert.channel.delete",
        &id,
        "",
        "Deleted notification channel",
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok(Json(serde_json::json!({ "deleted": id })))
}

/// Send a synthetic event through one channel so the setup can be verified.
pub async fn test_channel(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Path(id): Path<String>,
) -> ApiResult {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let channel: Channel = state
        .cache
        .get(&format!("{}{}", CHANNELS_PREFIX, id))
        .await
        .ok()
        .flatten()
        .ok_or_else(|| not_found("channel"))?;

    let result = notifier::deliver(
        &channel,
        &AlertEvent {
            kind: EventKind::Test,
            rule_id: "paqtra-test".to_string(),
            rule_name: "Paqtra test notification".to_string(),
            severity: "info".to_string(),
            message: "This is a test notification from Paqtra.".to_string(),
            at: chrono::Utc::now().to_rfc3339(),
        },
    )
    .await;

    emit_audit(
        &state,
        "alert.channel.test",
        &id,
        "",
        if result.delivered {
            "Test notification delivered"
        } else {
            "Test notification failed"
        },
        &actor_from_claims(&claims),
        if result.delivered {
            "success"
        } else {
            "failure"
        },
    )
    .await;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}

// ── Silences ────────────────────────────────────────────────

const MAX_SILENCE_MINUTES: u64 = 7 * 24 * 60;

pub async fn list_silences(State(state): State<Arc<AppState>>, claims: Claims) -> ApiResult {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;
    let silences = state
        .cache
        .list_values(SILENCES_PREFIX)
        .await
        .unwrap_or_default();
    Ok(Json(
        serde_json::json!({ "silences": silences, "total": silences.len() }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct CreateSilenceRequest {
    /// Rule to silence; omit to silence every rule.
    pub rule_id: Option<String>,
    pub duration_minutes: u64,
    pub comment: Option<String>,
}

pub async fn create_silence(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Json(req): Json<CreateSilenceRequest>,
) -> ApiResult {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    if req.duration_minutes == 0 || req.duration_minutes > MAX_SILENCE_MINUTES {
        return Err(bad_request("duration_minutes must be between 1 and 10080"));
    }
    let now = chrono::Utc::now();
    let until = now + chrono::Duration::minutes(req.duration_minutes as i64);
    let id = format!("sil-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let silence = serde_json::json!({
        "id": id,
        "rule_id": req.rule_id.filter(|r| !r.trim().is_empty()),
        "comment": req.comment.unwrap_or_default(),
        "created_by": actor_from_claims(&claims),
        "created_at": now.to_rfc3339(),
        "until": until.to_rfc3339(),
    });
    // The cache entry expires with the silence, so no cleanup is needed.
    state
        .cache
        .set_durable(
            &format!("{}{}", SILENCES_PREFIX, id),
            &silence,
            req.duration_minutes * 60,
        )
        .await
        .map_err(|_| internal("failed to store silence"))?;

    emit_audit(
        &state,
        "alert.silence.create",
        &id,
        "",
        &format!("Silenced alerts for {} minute(s)", req.duration_minutes),
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok(Json(silence))
}

pub async fn delete_silence(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Path(id): Path<String>,
) -> ApiResult {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let key = format!("{}{}", SILENCES_PREFIX, id);
    if state
        .cache
        .get::<serde_json::Value>(&key)
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return Err(not_found("silence"));
    }
    state
        .cache
        .delete(&key)
        .await
        .map_err(|_| internal("failed to delete silence"))?;
    emit_audit(
        &state,
        "alert.silence.delete",
        &id,
        "",
        "Removed silence",
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok(Json(serde_json::json!({ "deleted": id })))
}
