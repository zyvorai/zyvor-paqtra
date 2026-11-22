// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Policy management endpoints
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::{
    actor_from_claims, audit_log, has_namespace_access, policy_id_namespace, to_json, track_error,
    track_request,
};
use crate::error::ApiError;
use crate::models::policy::{
    self as policy_model, AddRuleRequest, CreatePolicyRequest, DeleteRuleQuery, DryRunQuery,
    EditRuleRequest,
};
use crate::services::k8s::{is_conflict, ApplyOptions};
use crate::AppState;

pub async fn list_policies(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Query(params): Query<super::PaginationQuery>,
) -> Result<Json<Value>, ApiError> {
    tracing::info!("Listing policies");

    track_request(&state, |m| {
        m.k8s_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

    match state.k8s.list_policies().await {
        Ok(policies) => {
            // Apply namespace RBAC filter
            let policies: Vec<_> = policies
                .into_iter()
                .filter(|p| has_namespace_access(&state, &claims, &p.namespace))
                .collect();
            let total = policies.len();
            let offset = params.offset.unwrap_or(0);
            let limit = params.limit.unwrap_or(50).min(1000);
            let page: Vec<_> = policies.into_iter().skip(offset).take(limit).collect();
            Ok(Json(json!({
                "policies": page,
                "total": total,
                "limit": limit,
                "offset": offset,
            })))
        }
        Err(e) => {
            tracing::error!("Failed to list policies: {}", e);
            track_error(&state).await;
            Err(ApiError::InternalError(e.to_string()))
        }
    }
}

pub async fn create_policy(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(req): Json<CreatePolicyRequest>,
) -> Result<Json<Value>, ApiError> {
    super::check_editor(&state, &claims).map_err(|_| ApiError::Forbidden)?;
    if !has_namespace_access(&state, &claims, &req.namespace) {
        return Err(ApiError::Forbidden);
    }

    // Validate spec size and depth
    req.validate_spec().map_err(ApiError::BadRequest)?;

    tracing::info!("Creating policy: {}/{}", req.namespace, req.name);

    track_request(&state, |m| {
        m.k8s_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

    match state.k8s.create_policy(&req).await {
        Ok(policy) => {
            state
                .metrics
                .policies_created
                .fetch_add(1, Ordering::Relaxed);
            audit_log(
                &state,
                "policy.create",
                &req.name,
                &req.namespace,
                "Policy created",
                &actor_from_claims(&claims),
                "success",
            )
            .await;
            Ok(Json(to_json(&policy)))
        }
        Err(e) => {
            tracing::error!("Failed to create policy: {}", e);
            track_error(&state).await;
            Err(ApiError::InternalError(e.to_string()))
        }
    }
}

pub async fn get_policy(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    tracing::info!("Getting policy: {}", id);

    track_request(&state, |m| {
        m.k8s_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

    let (namespace, name) = resolve_policy(&state, &claims, &id).await?;
    let obj = fetch_policy_object(&state, &namespace, &name).await?;
    Ok(Json(policy_detail(&obj, &namespace, &name)))
}

/// Summary fields plus `spec`, `resource_version` and per-direction rule counts.
fn policy_detail(obj: &Value, namespace: &str, name: &str) -> Value {
    let spec = obj.get("spec").cloned().unwrap_or(Value::Null);
    let meta = obj.get("metadata");
    let meta_str = |k: &str| {
        meta.and_then(|m| m.get(k))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    json!({
        "id": meta_str("uid"),
        "name": name,
        "namespace": namespace,
        "created_at": meta_str("creationTimestamp"),
        "status": obj.get("status").and_then(|s| s.get("phase")).and_then(Value::as_str).unwrap_or("active"),
        "resource_version": meta_str("resourceVersion"),
        "rule_counts": policy_model::rule_counts(&spec),
        "spec": spec,
    })
}

/// Map a policy id to `(namespace, name)` among policies the caller may see.
/// `namespace/name` is exact; a bare uid or name is looked up (and must be
/// unambiguous). Invisible and missing policies are indistinguishable.
async fn resolve_policy(
    state: &Arc<AppState>,
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
    id: &str,
) -> Result<(String, String), ApiError> {
    if let Some((ns, name)) = id.split_once('/') {
        if !has_namespace_access(state, claims, ns) {
            return Err(ApiError::NotFound);
        }
        return Ok((ns.to_string(), name.to_string()));
    }
    let policies = state.k8s.list_policies().await.map_err(|e| {
        tracing::error!("Failed to look up policy {}: {}", id, e);
        ApiError::InternalError(e.to_string())
    })?;
    let mut matches = policies
        .into_iter()
        .filter(|p| has_namespace_access(state, claims, &p.namespace))
        .filter(|p| p.id == id || p.name == id);
    let first = matches.next().ok_or(ApiError::NotFound)?;
    if matches.next().is_some() {
        return Err(ApiError::BadRequest(
            "policy name is ambiguous across namespaces; use namespace/name".into(),
        ));
    }
    Ok((first.namespace, first.name))
}

async fn fetch_policy_object(
    state: &Arc<AppState>,
    namespace: &str,
    name: &str,
) -> Result<Value, ApiError> {
    match state.k8s.get_policy_object(namespace, name).await {
        Ok(Some(obj)) => Ok(obj),
        Ok(None) => Err(ApiError::NotFound),
        Err(e) => {
            tracing::error!("Failed to get policy: {}", e);
            track_error(state).await;
            Err(ApiError::InternalError(e.to_string()))
        }
    }
}

pub async fn update_policy(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
    Json(req): Json<CreatePolicyRequest>,
) -> Result<Json<Value>, ApiError> {
    super::check_editor(&state, &claims).map_err(|_| ApiError::Forbidden)?;
    // The body's namespace is what gets applied.
    if !has_namespace_access(&state, &claims, &req.namespace) {
        return Err(ApiError::Forbidden);
    }

    // Validate spec size and depth
    req.validate_spec().map_err(ApiError::BadRequest)?;

    tracing::info!("Updating policy: {}", id);

    track_request(&state, |m| {
        m.k8s_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

    // Verify the policy exists before updating (prevent IDOR)
    // Also validate that the request body name matches the path ID
    match state.k8s.list_policies().await {
        Ok(policies) => {
            // Look only among policies the caller may see: matching by name across
            // all namespaces would let a scoped user probe for other namespaces'
            // policies (not found vs bad request).
            let policies: Vec<_> = policies
                .into_iter()
                .filter(|p| has_namespace_access(&state, &claims, &p.namespace))
                .collect();
            if !policies.iter().any(|p| p.id == id || p.name == id) {
                return Err(ApiError::NotFound);
            }
            // Prevent IDOR: ensure the request body name is consistent with the path ID
            if req.name != id && !policies.iter().any(|p| p.id == id && p.name == req.name) {
                return Err(ApiError::BadRequest(
                    "Policy name does not match path ID".into(),
                ));
            }
        }
        Err(e) => {
            tracing::error!("Failed to look up policy {}: {}", id, e);
            track_error(&state).await;
            return Err(ApiError::InternalError(e.to_string()));
        }
    }

    // kubectl apply is idempotent, so create == update
    match state.k8s.create_policy(&req).await {
        Ok(mut policy) => {
            policy.id = id;
            policy.status = "updated".to_string();
            Ok(Json(to_json(&policy)))
        }
        Err(e) => {
            tracing::error!("Failed to update policy: {}", e);
            track_error(&state).await;
            Err(ApiError::InternalError(e.to_string()))
        }
    }
}

pub async fn delete_policy(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    super::check_editor(&state, &claims).map_err(|_| ApiError::Forbidden)?;
    // The id names the namespace it acts on (`namespace/name`, else `default`).
    if !has_namespace_access(&state, &claims, policy_id_namespace(&id)) {
        return Err(ApiError::Forbidden);
    }
    tracing::info!("Deleting policy: {}", id);

    track_request(&state, |m| {
        m.k8s_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

    match state.k8s.delete_policy(&id).await {
        Ok(()) => {
            state
                .metrics
                .policies_deleted
                .fetch_add(1, Ordering::Relaxed);
            audit_log(
                &state,
                "policy.delete",
                &id,
                "",
                "Policy deleted",
                &actor_from_claims(&claims),
                "success",
            )
            .await;
            Ok(axum::http::StatusCode::NO_CONTENT)
        }
        Err(e) => {
            tracing::error!("Failed to delete policy: {}", e);
            track_error(&state).await;
            Err(ApiError::InternalError(e.to_string()))
        }
    }
}

pub async fn simulate_policy(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(req): Json<CreatePolicyRequest>,
) -> Result<Json<Value>, ApiError> {
    super::check_editor(&state, &claims).map_err(|_| ApiError::Forbidden)?;
    if !has_namespace_access(&state, &claims, &req.namespace) {
        return Err(ApiError::Forbidden);
    }

    // Validate spec size and depth
    req.validate_spec().map_err(ApiError::BadRequest)?;

    tracing::info!("Previewing policy impact: {}", req.name);

    track_request(&state, |m| {
        m.k8s_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;

    let preview =
        crate::services::investigate::preview_policy(&state, &req.name, &req.namespace, &req.spec)
            .await;

    audit_log(
        &state,
        "policy.simulate",
        &req.name,
        &req.namespace,
        "Policy impact previewed (evidence-backed)",
        &actor_from_claims(&claims),
        "success",
    )
    .await;

    Ok(Json(serde_json::to_value(preview).unwrap_or_else(
        |_| serde_json::json!({"error": "serialize failed"}),
    )))
}

/// Whether a `kubectl apply` failure is the cluster rejecting the manifest
/// (schema, CRD validation, admission webhook) rather than kubectl or the
/// cluster being unreachable.
fn is_validation_error(msg: &str) -> bool {
    [
        "is invalid",
        "Invalid value",
        "unknown field",
        "strict decoding error",
        "denied the request",
        "Unsupported value",
        "Required value",
    ]
    .iter()
    .any(|needle| msg.contains(needle))
}

/// A policy loaded for a rule edit; `spec` is what gets applied.
struct LoadedPolicy {
    namespace: String,
    name: String,
    resource_version: String,
    spec: Value,
}

/// What a rule endpoint does to the spec, once the policy is loaded.
struct RuleChange<'a> {
    action: &'static str,
    direction: policy_model::RuleDirection,
    resource_version: Option<&'a str>,
    dry_run: bool,
}

/// Shared tail of the three rule endpoints: concurrency check, size/depth
/// validation, apply (or dry-run), audit, response.
async fn commit_rule_change(
    state: &Arc<AppState>,
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
    policy: LoadedPolicy,
    change: RuleChange<'_>,
    extra: Value,
) -> Result<Json<Value>, ApiError> {
    let LoadedPolicy {
        namespace,
        name,
        resource_version: current_rv,
        spec,
    } = policy;
    let (namespace, name) = (namespace.as_str(), name.as_str());
    if let Some(rv) = change.resource_version {
        if rv != current_rv {
            return Err(ApiError::Conflict(format!(
                "policy changed since it was read (resource_version {} != {})",
                rv, current_rv
            )));
        }
    }

    let req = CreatePolicyRequest {
        name: name.to_string(),
        namespace: namespace.to_string(),
        spec,
    };
    req.validate_spec().map_err(ApiError::BadRequest)?;

    let opts = ApplyOptions {
        dry_run: change.dry_run,
        resource_version: Some(current_rv),
    };
    if let Err(e) = state.k8s.apply_policy(&req, &opts).await {
        if is_conflict(&e) {
            return Err(ApiError::Conflict(
                "policy changed while applying; re-read and retry".into(),
            ));
        }
        tracing::warn!("Failed to {} policy rule: {}", change.action, e);
        track_error(state).await;
        // The API server's reason is what the caller needs to fix a bad rule.
        let msg = e.to_string();
        return Err(if is_validation_error(&msg) {
            ApiError::BadRequest(msg)
        } else {
            ApiError::BadGateway(msg)
        });
    }

    if !change.dry_run {
        audit_log(
            state,
            &format!("policy.rule.{}", change.action),
            name,
            namespace,
            &format!("{} rule ({})", change.action, change.direction.key()),
            &actor_from_claims(claims),
            "success",
        )
        .await;
    }

    Ok(Json(json!({
        "policy": format!("{}/{}", namespace, name),
        "action": change.action,
        "direction": change.direction.key(),
        "dry_run": change.dry_run,
        "rule_counts": policy_model::rule_counts(&req.spec),
        "spec": req.spec,
        "result": extra,
    })))
}

/// Load a policy for a rule edit: editor role, namespace access, full object,
/// and a spec we can mutate.
async fn load_for_rule_edit(
    state: &Arc<AppState>,
    claims: &Option<axum::Extension<crate::middleware::auth::Claims>>,
    id: &str,
) -> Result<LoadedPolicy, ApiError> {
    super::check_editor(state, claims).map_err(|_| ApiError::Forbidden)?;
    track_request(state, |m| {
        m.k8s_queries.fetch_add(1, Ordering::Relaxed);
    })
    .await;
    let (namespace, name) = resolve_policy(state, claims, id).await?;
    let obj = fetch_policy_object(state, &namespace, &name).await?;
    let rv = obj
        .pointer("/metadata/resourceVersion")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let spec = obj.get("spec").cloned().unwrap_or(Value::Null);
    Ok(LoadedPolicy {
        namespace,
        name,
        resource_version: rv,
        spec,
    })
}

pub async fn add_policy_rule(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
    Query(q): Query<DryRunQuery>,
    Json(req): Json<AddRuleRequest>,
) -> Result<Json<Value>, ApiError> {
    let mut policy = load_for_rule_edit(&state, &claims, &id).await?;
    let index = policy_model::add_rule(&mut policy.spec, req.direction, req.rule)
        .map_err(ApiError::BadRequest)?;
    commit_rule_change(
        &state,
        &claims,
        policy,
        RuleChange {
            action: "add",
            direction: req.direction,
            resource_version: req.resource_version.as_deref(),
            dry_run: q.dry_run,
        },
        json!({ "index": index }),
    )
    .await
}

pub async fn edit_policy_rule(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
    Query(q): Query<DryRunQuery>,
    Json(req): Json<EditRuleRequest>,
) -> Result<Json<Value>, ApiError> {
    let mut policy = load_for_rule_edit(&state, &claims, &id).await?;
    policy_model::edit_rule(&mut policy.spec, req.direction, req.index, req.rule)
        .map_err(ApiError::BadRequest)?;
    commit_rule_change(
        &state,
        &claims,
        policy,
        RuleChange {
            action: "edit",
            direction: req.direction,
            resource_version: req.resource_version.as_deref(),
            dry_run: q.dry_run,
        },
        json!({ "index": req.index }),
    )
    .await
}

pub async fn delete_policy_rule(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
    Query(q): Query<DeleteRuleQuery>,
    Query(dry): Query<DryRunQuery>,
) -> Result<Json<Value>, ApiError> {
    let mut policy = load_for_rule_edit(&state, &claims, &id).await?;
    let removed = policy_model::delete_rule(&mut policy.spec, q.direction, q.index)
        .map_err(ApiError::BadRequest)?;
    commit_rule_change(
        &state,
        &claims,
        policy,
        RuleChange {
            action: "delete",
            direction: q.direction,
            resource_version: q.resource_version.as_deref(),
            dry_run: dry.dry_run,
        },
        json!({ "index": q.index, "removed": removed }),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::is_validation_error;

    #[test]
    fn schema_and_webhook_rejections_are_validation_errors() {
        for msg in [
            "kubectl apply failed: The CiliumNetworkPolicy \"db\" is invalid: spec.ingress[0]: Invalid value",
            "error: .spec.ingres: field not declared in schema; unknown field \"ingres\"",
            "Error from server: admission webhook \"x\" denied the request: nope",
            "strict decoding error: unknown field \"spec.foo\"",
        ] {
            assert!(is_validation_error(msg), "{msg}");
        }
    }

    #[test]
    fn connectivity_and_timeouts_are_not() {
        for msg in [
            "kubectl apply timed out after 30 seconds",
            "Failed to spawn kubectl",
            "Unable to connect to the server: dial tcp 10.0.0.1:6443: i/o timeout",
            "error: You must be logged in to the server (Unauthorized)",
        ] {
            assert!(!is_validation_error(msg), "{msg}");
        }
    }
}
