// HTTP request handlers
pub mod anomalies;
pub mod auth;
pub mod cilium_obs;
pub mod compliance;
pub mod connectivity;
pub mod ebpf;
pub mod endpoints;
pub mod events;
pub mod extended;
pub mod extended2;
pub mod extended3;
pub mod extended4;
pub mod flow_history;
pub mod flows;
pub mod health;
pub mod investigate;
pub mod metrics;
pub mod metrics_platform;
pub mod modules;
pub mod nodes;
pub mod notifications;
pub mod policies;
pub mod slo_incidents;
pub mod users;

use crate::{AppMetrics, AppState};
use axum::{http::StatusCode, Json};
use serde::Serialize;
use serde_json::Value;
use std::sync::atomic::Ordering;

/// RBAC check: require admin role for destructive operations.
/// Returns Ok(()) if auth is disabled or the caller has role == "admin".
/// Returns Err(403) otherwise.
///
/// `claims` comes from `Option<axum::Extension<Claims>>` extractors injected
/// by the auth middleware.
pub fn check_admin(
    state: &AppState,
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if state.config.auth_disabled {
        return Ok(());
    }
    match claims.as_ref().map(|c| c.role.as_str()) {
        Some("admin") => Ok(()),
        _ => Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Admin role required"})),
        )),
    }
}

/// Allows admin and editor. Use only for actions listed in
/// `middleware::auth::EDITOR_WRITES` (the middleware enforces that list for
/// editors; this check keeps viewers and unknown roles out of reads as well).
pub fn check_editor(
    state: &AppState,
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if state.config.auth_disabled {
        return Ok(());
    }
    match claims.as_ref().map(|c| c.role.as_str()) {
        Some("admin") | Some("editor") => Ok(()),
        _ => Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Editor or admin role required"})),
        )),
    }
}

/// Increment total_requests and run an extra closure on the metrics.
pub async fn track_request(state: &AppState, f: impl FnOnce(&AppMetrics)) {
    state.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
    f(&state.metrics);
}

/// Response for endpoints whose feature is not built yet. Used instead of
/// reporting success for an action that does nothing: the caller learns the
/// truth and no record of a phantom action is created.
pub fn not_implemented(feature: &str, detail: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": format!("{feature} is not implemented yet: {detail}"),
            "code": "not_implemented",
        })),
    )
}

/// Increment total_errors.
pub async fn track_error(state: &AppState) {
    state.metrics.total_errors.fetch_add(1, Ordering::Relaxed);
}

/// Serialize to JSON with empty-object fallback.
pub fn to_json<T: Serialize>(val: &T) -> Value {
    serde_json::to_value(val).unwrap_or_else(|e| {
        tracing::warn!("JSON serialization failed: {}", e);
        serde_json::json!({})
    })
}

/// Log an audit event to the in-memory cache.
///
/// If a `request_id` is provided (from the correlation middleware), it is
/// included in the audit entry for end-to-end traceability.
pub async fn audit_log(
    state: &AppState,
    action: &str,
    resource: &str,
    namespace: &str,
    details: &str,
    actor: &str,
    outcome: &str,
) {
    audit_log_with_request_id(
        state, action, resource, namespace, details, actor, outcome, None,
    )
    .await;
}

/// Like [`audit_log`] but accepts an optional correlation `request_id`.
#[allow(clippy::too_many_arguments)]
pub async fn audit_log_with_request_id(
    state: &AppState,
    action: &str,
    resource: &str,
    namespace: &str,
    details: &str,
    actor: &str,
    outcome: &str,
    request_id: Option<&str>,
) {
    let id = format!(
        "aud-{}",
        uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("000")
    );
    let mut entry = serde_json::json!({
        "id": id,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "action": action,
        "resource": resource,
        "namespace": namespace,
        "details": details,
        "actor": actor,
        "outcome": outcome,
    });
    if let Some(rid) = request_id {
        if let Some(obj) = entry.as_object_mut() {
            obj.insert("request_id".to_string(), serde_json::json!(rid));
        }
    }
    let _ = state
        .cache
        .set_persistent(&format!("cv:audit_log:{}", id), &entry)
        .await;
}

/// Extract the actor (subject) from JWT claims, defaulting to "anonymous".
pub fn actor_from_claims(
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> String {
    claims
        .as_ref()
        .map(|c| c.sub.clone())
        .unwrap_or_else(|| "anonymous".to_string())
}

/// Shared pagination query params for list endpoints.
#[derive(Debug, serde::Deserialize)]
pub struct PaginationQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Extract a string field from a JSON value, returning empty string if absent.
pub fn jstr(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

/// Check if the caller has access to a specific namespace.
/// Returns true if: auth is disabled, role is admin, namespaces list is empty (all access), or namespace is in the list.
pub fn has_namespace_access(
    state: &AppState,
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
    namespace: &str,
) -> bool {
    if state.config.auth_disabled {
        return true;
    }
    match claims.as_ref() {
        Some(c) if c.role == "admin" => true,
        Some(c) if c.namespaces.is_empty() => true, // empty = all
        Some(c) => c.namespaces.iter().any(|ns| ns == namespace || ns == "*"),
        None => false,
    }
}

/// Whether the caller may see a flow: it is visible if its source *or* its
/// destination is in a namespace the caller can access. Source-only would hide
/// traffic sent to the caller's namespace while exposing the peers of traffic
/// sent from it.
pub fn flow_visible(
    state: &AppState,
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
    flow: &crate::models::flow::Flow,
) -> bool {
    has_namespace_access(state, claims, &flow.source.namespace)
        || has_namespace_access(state, claims, &flow.destination.namespace)
}

/// The namespace named by a policy id: `namespace/name`, or `default` when the
/// id is a bare name. Mirrors `K8sService::delete_policy`, which acts on it.
pub fn policy_id_namespace(id: &str) -> &str {
    id.split_once('/').map(|(ns, _)| ns).unwrap_or("default")
}

/// Filter a list of JSON values by namespace access. Checks "namespace" field on each item.
pub fn filter_by_namespace_access(
    state: &AppState,
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
    items: Vec<serde_json::Value>,
) -> Vec<serde_json::Value> {
    if state.config.auth_disabled {
        return items;
    }
    match claims.as_ref() {
        Some(c) if c.role == "admin" || c.namespaces.is_empty() => items,
        Some(c) => items
            .into_iter()
            .filter(|item| {
                item.get("namespace")
                    .and_then(|v| v.as_str())
                    .map(|ns| {
                        c.namespaces
                            .iter()
                            .any(|allowed| allowed == ns || allowed == "*")
                    })
                    .unwrap_or(true) // keep items without namespace field
            })
            .collect(),
        None => vec![],
    }
}

/// Apply pagination to a JSON array field and return the response with metadata.
/// `items_key` is the JSON field name for the items array.
pub fn paginate_json(items: Vec<Value>, params: &PaginationQuery, items_key: &str) -> Value {
    let total = items.len();
    let offset = params.offset.unwrap_or(0);
    let limit = params.limit.unwrap_or(50).min(1000);
    let page: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
    serde_json::json!({
        items_key: page,
        "total": total,
        "limit": limit,
        "offset": offset,
    })
}
