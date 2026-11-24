// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Username/password login → JWT, plus identity and self-service password change.

use axum::{
    extract::{ConnectInfo, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::middleware::auth::Claims;
use crate::services::login_guard::guard;
use crate::services::users::{self, Role};
use crate::AppState;

use super::audit_log;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub role: String,
    pub username: String,
    pub expires_in: u64,
}

/// Token lifetime: 24 hours.
const TOKEN_TTL_SECS: u64 = 24 * 60 * 60;
/// Longer passwords are refused before hashing, so they cannot be used to burn CPU.
const MAX_LOGIN_PASSWORD_BYTES: usize = 1024;

fn client_ip(conn: &Option<Extension<ConnectInfo<SocketAddr>>>) -> IpAddr {
    conn.as_ref()
        .map(|c| c.0 .0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

/// Printable, bounded form of a client-supplied name, safe for logs and audit.
fn sanitize(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).take(64).collect()
}

fn wrong_credentials() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": "Wrong username or password."})),
    )
        .into_response()
}

fn too_many_attempts(retry_secs: u64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, retry_secs.to_string())],
        Json(json!({"error": format!("Too many failed login attempts. Try again in {retry_secs} seconds.")})),
    )
        .into_response()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Check the supplied credentials; returns the account name, role and the
/// earliest `iat` its token may carry (see `users::revocation_cutoff`).
///
/// Every failure path does one Argon2 verification's worth of work, so response
/// time does not reveal whether a username exists or is the configured admin.
async fn authenticate(
    state: &AppState,
    username: &str,
    password: &str,
) -> Option<(String, Role, i64)> {
    let cfg = &state.config;

    if username == cfg.admin_username {
        let by_password = users::secrets_equal(password, &cfg.admin_password);
        // An unset API key is empty and must never match.
        let by_key = !cfg.api_key.is_empty() && users::secrets_equal(password, &cfg.api_key);
        if by_password | by_key {
            return Some((username.to_string(), Role::Admin, 0));
        }
        users::burn_verify(password.to_string()).await;
        return None;
    }

    let local = match users::normalize_username(username) {
        Ok(name) => users::get(state, &name).await,
        Err(_) => None,
    };
    match local {
        Some(user) if user.enabled => {
            if users::verify_password_async(user.password_hash.clone(), password.to_string()).await
            {
                Some((user.username, user.role, user.password_changed_at))
            } else {
                None
            }
        }
        _ => {
            users::burn_verify(password.to_string()).await;
            None
        }
    }
}

// Axum handlers return the `Response` itself as the error, so the large `Err` is by design.
#[allow(clippy::result_large_err)]
pub async fn login(
    State(state): State<Arc<AppState>>,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, Response> {
    let ip = client_ip(&conn);
    if let Err(retry) = guard().check(ip, Instant::now()) {
        tracing::warn!(%ip, "login refused: too many failed attempts");
        return Err(too_many_attempts(retry));
    }

    let user = body.username.trim();
    let pass = body.password;

    if user.is_empty() || pass.is_empty() || pass.len() > MAX_LOGIN_PASSWORD_BYTES {
        return Err(wrong_credentials());
    }

    let shown = sanitize(user);
    let Some((username, role, min_iat)) = authenticate(&state, user, &pass).await else {
        guard().record_failure(ip, Instant::now());
        tracing::warn!(username = %shown, %ip, "login failed");
        audit_log(
            &state,
            "auth.login",
            &shown,
            "",
            "Failed login",
            &shown,
            "failure",
        )
        .await;
        return Err(wrong_credentials());
    };
    guard().clear(ip);

    let now = now_secs();
    let claims = Claims {
        sub: username.clone(),
        exp: (now + TOKEN_TTL_SECS) as usize,
        iat: (now as i64).max(min_iat) as usize,
        role: role.as_str().to_string(),
        namespaces: vec![],
    };

    let encoding_key =
        EncodingKey::from_secret(state.config.jwt_secret.as_bytes()).map_err(|e| {
            tracing::error!("JWT key rejected: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Could not issue session token"})),
            )
                .into_response()
        })?;
    let token = encode(&Header::new(Algorithm::HS256), &claims, &encoding_key).map_err(|e| {
        tracing::error!("JWT encode failed: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Could not issue session token"})),
        )
            .into_response()
    })?;

    audit_log(
        &state,
        "auth.login",
        &username,
        "",
        "Signed in",
        &username,
        "success",
    )
    .await;
    Ok(Json(LoginResponse {
        token,
        role: role.as_str().to_string(),
        username,
        expires_in: TOKEN_TTL_SECS,
    }))
}

/// Who the caller is, as the server currently sees them.
pub async fn me(
    State(state): State<Arc<AppState>>,
    claims: Option<Extension<Claims>>,
) -> Json<Value> {
    match claims {
        Some(Extension(c)) => Json(json!({
            "username": c.sub,
            "role": c.role,
            "namespaces": c.namespaces,
            "source": if c.sub == state.config.admin_username { "config" } else { "local" },
        })),
        // Authentication is disabled (development only).
        None => Json(
            json!({"username": "anonymous", "role": "admin", "namespaces": [], "source": "auth_disabled"}),
        ),
    }
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// Change the caller's own password. Tokens issued before the change stop
/// working, so the caller must sign in again.
#[allow(clippy::result_large_err)]
pub async fn change_password(
    State(state): State<Arc<AppState>>,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    claims: Option<Extension<Claims>>,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<Json<Value>, Response> {
    let err =
        |status: StatusCode, msg: &str| (status, Json(json!({ "error": msg }))).into_response();
    let Some(Extension(claims)) = claims else {
        return Err(err(StatusCode::BAD_REQUEST, "No account is signed in"));
    };
    if claims.sub == state.config.admin_username {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "This account's password is set by ADMIN_PASSWORD in the server configuration",
        ));
    }
    if req.current_password.len() > MAX_LOGIN_PASSWORD_BYTES {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Current password is incorrect",
        ));
    }
    users::validate_password(&req.new_password).map_err(|m| err(StatusCode::BAD_REQUEST, m))?;

    // Guessing the current password here is guessing a password: share the login throttle.
    let ip = client_ip(&conn);
    if let Err(retry) = guard().check(ip, Instant::now()) {
        return Err(too_many_attempts(retry));
    }
    let Some(mut user) = users::get(&state, &claims.sub).await else {
        return Err(err(StatusCode::BAD_REQUEST, "Account not found"));
    };
    if !users::verify_password_async(user.password_hash.clone(), req.current_password.clone()).await
    {
        guard().record_failure(ip, Instant::now());
        // 400, not 401: a 401 makes the UI treat the session as expired.
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Current password is incorrect",
        ));
    }
    if req.new_password == req.current_password {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "New password must differ from the current one",
        ));
    }

    user.password_hash = users::hash_password_async(req.new_password)
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not update password",
            )
        })?;
    user.password_changed_at = users::revocation_cutoff();
    user.updated_at = chrono::Utc::now().to_rfc3339();
    users::save(&state, &user).await.map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not update password",
        )
    })?;

    audit_log(
        &state,
        "auth.password",
        &user.username,
        "",
        "Changed own password",
        &user.username,
        "success",
    )
    .await;
    Ok(Json(json!({ "changed": true, "reauthenticate": true })))
}
