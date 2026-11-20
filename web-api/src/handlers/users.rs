// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// User management (admin only).

use super::{actor_from_claims, audit_log as emit_audit, check_admin, track_request};
use crate::services::users::{self, Role, User};
use crate::AppState;
use axum::{
    extract::{Path, State},
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

pub async fn list_users(State(state): State<Arc<AppState>>, claims: Claims) -> ApiResult {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;
    let users: Vec<Value> = users::list(&state).await.iter().map(User::public).collect();
    Ok(Json(json!({
        "users": users,
        "total": users.len(),
        // The configured account is not stored, but callers need to know it exists.
        "config_admin": state.config.admin_username,
    })))
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: Role,
    /// Limit the user to these namespaces; omit or leave empty for all.
    #[serde(default)]
    pub namespaces: Vec<String>,
}

/// Admins are never limited to namespaces, so a scope on one would only give a
/// false sense of restriction.
fn check_scope_fits_role(
    role: Role,
    namespaces: &[String],
) -> Result<(), (StatusCode, Json<Value>)> {
    if role == Role::Admin && !namespaces.is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Admins are not limited to namespaces: leave the namespace list empty or choose another role",
        ));
    }
    Ok(())
}

pub async fn create_user(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Json(req): Json<CreateUserRequest>,
) -> ApiResult {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let username =
        users::normalize_username(&req.username).map_err(|m| err(StatusCode::BAD_REQUEST, m))?;
    users::validate_password(&req.password).map_err(|m| err(StatusCode::BAD_REQUEST, m))?;
    let namespaces =
        users::normalize_namespaces(req.namespaces).map_err(|m| err(StatusCode::BAD_REQUEST, m))?;
    check_scope_fits_role(req.role, &namespaces)?;
    if username == state.config.admin_username.to_ascii_lowercase() {
        return Err(err(
            StatusCode::CONFLICT,
            "That name belongs to the configured admin account",
        ));
    }
    if users::get(&state, &username).await.is_some() {
        return Err(err(
            StatusCode::CONFLICT,
            "A user with this name already exists",
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let user = User {
        username: username.clone(),
        password_hash: users::hash_password_async(req.password)
            .await
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Could not create user"))?,
        role: req.role,
        enabled: true,
        created_at: now.clone(),
        updated_at: now,
        password_changed_at: users::now_epoch(),
        namespaces,
    };
    users::save(&state, &user)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Could not create user"))?;

    emit_audit(
        &state,
        "user.create",
        &username,
        "",
        &format!(
            "Created {} user{}",
            user.role.as_str(),
            scope_note(&user.namespaces)
        ),
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok(Json(user.public()))
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub role: Option<Role>,
    pub enabled: Option<bool>,
    /// Resetting the password also signs the user out everywhere.
    pub password: Option<String>,
    /// Replace the namespace limit; an empty list removes it.
    pub namespaces: Option<Vec<String>>,
}

fn scope_note(namespaces: &[String]) -> String {
    if namespaces.is_empty() {
        String::new()
    } else {
        format!(" limited to {}", namespaces.join(", "))
    }
}

pub async fn update_user(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Path(username): Path<String>,
    Json(req): Json<UpdateUserRequest>,
) -> ApiResult {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;
    let actor = actor_from_claims(&claims);

    if req.role.is_none()
        && req.enabled.is_none()
        && req.password.is_none()
        && req.namespaces.is_none()
    {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Provide at least one of role, enabled, password or namespaces",
        ));
    }
    let mut user = users::get(&state, &username)
        .await
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "User not found"))?;

    // An admin who demotes or disables themselves would lose access mid-session.
    let is_self = user.username == actor;
    if is_self && (req.enabled == Some(false) || req.role.is_some_and(|r| r != Role::Admin)) {
        return Err(err(
            StatusCode::CONFLICT,
            "You cannot disable or demote your own account",
        ));
    }

    let mut changes = Vec::new();
    if let Some(role) = req.role.filter(|r| *r != user.role) {
        changes.push(format!("role {} -> {}", user.role.as_str(), role.as_str()));
        user.role = role;
    }
    // Validate the namespace change against the role the user will end up with.
    if let Some(raw) = req.namespaces {
        let namespaces =
            users::normalize_namespaces(raw).map_err(|m| err(StatusCode::BAD_REQUEST, m))?;
        check_scope_fits_role(user.role, &namespaces)?;
        if namespaces != user.namespaces {
            changes.push(if namespaces.is_empty() {
                "namespace limit removed".to_string()
            } else {
                format!("namespaces -> {}", namespaces.join(", "))
            });
            user.namespaces = namespaces;
        }
    } else if user.role == Role::Admin && !user.namespaces.is_empty() {
        // Promoted to admin without saying anything about namespaces: admins are
        // not scoped, so drop the old limit rather than keep a meaningless one.
        user.namespaces.clear();
        changes.push("namespace limit removed (admins are not scoped)".to_string());
    }
    if let Some(enabled) = req.enabled.filter(|e| *e != user.enabled) {
        changes.push(if enabled {
            "enabled".to_string()
        } else {
            "disabled".to_string()
        });
        user.enabled = enabled;
    }
    if let Some(password) = req.password {
        users::validate_password(&password).map_err(|m| err(StatusCode::BAD_REQUEST, m))?;
        user.password_hash = users::hash_password_async(password)
            .await
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Could not update user"))?;
        user.password_changed_at = users::revocation_cutoff();
        changes.push("password reset".to_string());
    }
    user.updated_at = chrono::Utc::now().to_rfc3339();
    users::save(&state, &user)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Could not update user"))?;

    let details = if changes.is_empty() {
        "No change".to_string()
    } else {
        changes.join(", ")
    };
    emit_audit(
        &state,
        "user.update",
        &username,
        "",
        &details,
        &actor,
        "success",
    )
    .await;
    Ok(Json(user.public()))
}

pub async fn delete_user(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Path(username): Path<String>,
) -> ApiResult {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;
    let actor = actor_from_claims(&claims);

    if users::get(&state, &username).await.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "User not found"));
    }
    if username == actor {
        return Err(err(
            StatusCode::CONFLICT,
            "You cannot delete your own account",
        ));
    }
    users::delete(&state, &username)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Could not delete user"))?;
    emit_audit(
        &state,
        "user.delete",
        &username,
        "",
        "Deleted user",
        &actor,
        "success",
    )
    .await;
    Ok(Json(json!({ "deleted": username })))
}
