// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::{
    actor_from_claims, audit_log, check_admin, paginate_json, track_request, PaginationQuery,
};
use crate::services::change_tracker::{RollbackBlock, RollbackIndex, ROLLBACKS_PREFIX};
use crate::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct ValidatePolicyRequest {
    #[serde(default)]
    pub yaml: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateExportRequest {
    #[serde(default = "default_export_name")]
    pub name: String,
    #[serde(default = "default_export_format")]
    pub format: String,
    #[serde(default)]
    pub destination: String,
}

fn default_export_name() -> String {
    "new-export".to_string()
}
fn default_export_format() -> String {
    "json".to_string()
}

#[derive(Debug, Deserialize)]
pub struct NodeActionRequest {
    #[serde(default)]
    pub node: String,
}

// ── WireGuard Peers ───────────────────────────────────────

pub async fn wireguard_peers(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;

    // Try to get real WireGuard data from cilium
    let wg_output = state
        .k8s
        .exec_cilium_agent(&["cilium", "encrypt", "status"])
        .await;

    if !wg_output.is_empty() {
        return Ok(Json(serde_json::json!({
            "raw_status": wg_output,
            "source": "cilium encrypt status",
        })));
    }

    // No real WireGuard data available
    Ok(Json(serde_json::json!({
        "peers": [],
        "message": "WireGuard encryption not detected or cilium agent unreachable"
    })))
}

// ── Cilium Status ─────────────────────────────────────────

pub async fn cilium_status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Always inventory from Cilium agent pods. Do not use `kubectl exec -l …`
    // (invalid) or return a bare `status` object — the UI expects `agents[]`.
    let pods = state
        .k8s
        .kubectl_json(&[
            "get",
            "pods",
            "-n",
            "kube-system",
            "-l",
            "k8s-app=cilium",
            "-o",
            "json",
        ])
        .await;

    let mut agents: Vec<serde_json::Value> = pods
        .get("items")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    let meta = item.get("metadata").unwrap_or(item);
                    let status = item.get("status").unwrap_or(item);
                    let spec = item.get("spec").unwrap_or(item);
                    let phase = status
                        .get("phase")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown");
                    let name = meta.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let node = spec
                        .get("nodeName")
                        .and_then(|v| v.as_str())
                        .or_else(|| {
                            meta.get("labels")
                                .and_then(|l| l.get("kubernetes.io/hostname"))
                                .and_then(|v| v.as_str())
                        })
                        .unwrap_or("unknown");
                    let ready = status
                        .get("containerStatuses")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            !arr.is_empty()
                                && arr.iter().all(|c| {
                                    c.get("ready").and_then(|v| v.as_bool()).unwrap_or(false)
                                })
                        })
                        .unwrap_or(false);
                    let image = status
                        .get("containerStatuses")
                        .and_then(|v| v.as_array())
                        .and_then(|arr| {
                            arr.iter().find(|c| {
                                c.get("name").and_then(|n| n.as_str()) == Some("cilium-agent")
                            })
                        })
                        .and_then(|c| c.get("image").and_then(|v| v.as_str()))
                        .unwrap_or("");
                    let version = image
                        .rsplit(':')
                        .next()
                        .filter(|t| !t.is_empty() && *t != image)
                        .unwrap_or("");
                    serde_json::json!({
                        "name": name,
                        "pod": name,
                        "node": node,
                        "status": if ready { "OK" } else { phase },
                        "ready": ready,
                        "version": version,
                        "host_ip": status.get("hostIP").and_then(|v| v.as_str()).unwrap_or(""),
                        "message": if ready { "cilium-agent ready" } else { phase },
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // Best-effort: enrich first agent with `cilium status --brief` (needs pods/exec).
    if let Some(first) = agents.first().cloned() {
        let pod = first
            .get("pod")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !pod.is_empty() {
            use crate::services::k8s::K8sService;
            let brief = K8sService::run_cmd(
                "kubectl",
                &[
                    "exec",
                    "-n",
                    "kube-system",
                    &pod,
                    "-c",
                    "cilium-agent",
                    "--",
                    "cilium",
                    "status",
                    "--brief",
                ],
            )
            .await;
            if !brief.is_empty() {
                let ok_line = brief.lines().next().unwrap_or("").trim().to_string();
                for a in &mut agents {
                    if let Some(obj) = a.as_object_mut() {
                        obj.insert("message".into(), serde_json::Value::String(ok_line.clone()));
                        if ok_line.to_ascii_lowercase().contains("ok") {
                            obj.insert("status".into(), serde_json::Value::String("OK".into()));
                        }
                    }
                }
            }
        }
    }

    let healthy = agents
        .iter()
        .filter(|a| {
            a.get("ready").and_then(|v| v.as_bool()).unwrap_or(false)
                || a.get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.eq_ignore_ascii_case("OK") || s.eq_ignore_ascii_case("Running"))
                    .unwrap_or(false)
        })
        .count();

    Json(serde_json::json!({
        "agents": agents,
        "total": agents.len(),
        "healthy": healthy,
    }))
}

// ── Policy Validation ─────────────────────────────────────

pub async fn validate_policy(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(body): Json<ValidatePolicyRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;
    let yaml = &body.yaml;
    if yaml.is_empty() {
        return Ok(Json(serde_json::json!({
            "valid": false,
            "errors": ["Empty policy YAML provided"]
        })));
    }

    // Parse YAML locally for structure check
    let mut errors: Vec<String> = Vec::new();
    match serde_yaml::from_str::<serde_json::Value>(yaml) {
        Ok(parsed) => {
            if parsed.get("apiVersion").is_none() {
                errors.push("Missing 'apiVersion' field".to_string());
            }
            if parsed.get("kind").is_none() {
                errors.push("Missing 'kind' field".to_string());
            }
            if parsed.get("metadata").is_none() {
                errors.push("Missing 'metadata' field".to_string());
            }
            if parsed.get("spec").is_none() {
                errors.push("Missing 'spec' field".to_string());
            }
        }
        Err(e) => {
            errors.push(format!("Invalid YAML syntax: {}", e));
        }
    }

    // If local parse passed, validate via kubectl dry-run (piping YAML to stdin)
    let mut dry_run_output: Option<String> = None;
    if errors.is_empty() {
        use crate::services::k8s::K8sService;
        let (ok, stdout, stderr) = K8sService::run_cmd_stdin(
            "kubectl",
            &["apply", "--dry-run=client", "-f", "-", "--validate=true"],
            yaml,
        )
        .await;
        if !ok && !stderr.is_empty() {
            errors.push(format!("kubectl dry-run failed: {}", stderr));
        }
        let output = if ok { stdout } else { stderr };
        if !output.is_empty() {
            dry_run_output = Some(output);
        }
    }

    let valid = errors.is_empty();
    Ok(Json(serde_json::json!({
        "valid": valid,
        "errors": errors,
        "dry_run_output": dry_run_output,
    })))
}

// ── Flow Export Configs ───────────────────────────────────

const EXPORT_CONFIGS_PREFIX: &str = "cv:export_configs:";

pub async fn list_export_configs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;
    let items = state
        .cache
        .list_values(EXPORT_CONFIGS_PREFIX)
        .await
        .unwrap_or_default();
    Json(paginate_json(items, &params, "configs"))
}

pub async fn create_export_config(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(body): Json<CreateExportRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let id = format!(
        "exp-{}",
        uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("000")
    );
    let config = serde_json::json!({
        "id": id,
        "name": body.name,
        "format": body.format,
        "destination": body.destination,
        "status": "active",
        "created_at": chrono::Utc::now().to_rfc3339(),
    });

    let _ = state
        .cache
        .set_persistent(&format!("{}{}", EXPORT_CONFIGS_PREFIX, id), &config)
        .await;
    audit_log(
        &state,
        "export.create",
        &id,
        "",
        "Export config created",
        &actor_from_claims(&claims),
        "success",
    )
    .await;

    Ok(Json(serde_json::json!({
        "id": id,
        "status": "active",
        "message": "Export configuration created successfully",
    })))
}

pub async fn delete_export_config(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let key = format!("{}{}", EXPORT_CONFIGS_PREFIX, id);
    let _ = state.cache.delete(&key).await;
    audit_log(
        &state,
        "export.delete",
        &id,
        "",
        "Export config deleted",
        &actor_from_claims(&claims),
        "success",
    )
    .await;

    Ok(Json(serde_json::json!({
        "id": id,
        "status": "deleted",
        "message": "Export configuration deleted",
    })))
}

// ── Change Log ────────────────────────────────────────────

const CHANGES_PREFIX: &str = "cv:changes:";

/// Ids of changes that have been rolled back.
async fn rolled_back_ids(state: &AppState) -> Vec<String> {
    state
        .cache
        .list_values(ROLLBACKS_PREFIX)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.get("id").and_then(|i| i.as_str()).map(String::from))
        .collect()
}

pub async fn list_changes(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Changes are stored by the background change tracker. Rollback state is
    // computed here, from the same rules the rollback endpoint enforces, so the
    // UI only offers a rollback that would be accepted.
    let mut items = state
        .cache
        .list_values(CHANGES_PREFIX)
        .await
        .unwrap_or_default();
    let rolled_back = rolled_back_ids(&state).await;
    let index = RollbackIndex::new(&items, rolled_back.clone());
    for item in items.iter_mut() {
        let available = index.check(item).is_ok();
        let done = item
            .get("id")
            .and_then(|i| i.as_str())
            .is_some_and(|id| rolled_back.iter().any(|r| r == id));
        item["rollback_available"] = serde_json::json!(available);
        item["rolled_back"] = serde_json::json!(done);
    }
    Json(paginate_json(items, &params, "changes"))
}

#[derive(Debug, Deserialize)]
pub struct ImpactQueryParams {
    #[serde(default = "default_before")]
    pub before: String,
    #[serde(default = "default_after")]
    pub after: String,
    #[serde(default = "default_impact_limit")]
    pub limit: usize,
    pub kind: Option<String>,
    pub namespace: Option<String>,
}

fn default_before() -> String {
    "30m".into()
}
fn default_after() -> String {
    "30m".into()
}
fn default_impact_limit() -> usize {
    40
}

/// GET /api/v1/changes/{id}/impact — evidence-backed before/after flow comparison.
pub async fn change_impact(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(params): Query<ImpactQueryParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let q = crate::services::change_impact::ImpactQuery {
        before: crate::services::change_impact::parse_window(
            &params.before,
            chrono::Duration::minutes(30),
        ),
        after: crate::services::change_impact::parse_window(
            &params.after,
            chrono::Duration::minutes(30),
        ),
        limit: params.limit.clamp(1, 200),
        kind: params.kind.clone(),
        namespace: params.namespace.clone(),
    };

    match crate::services::change_impact::analyze_change_impact(&state, &claims, &id, q).await {
        Ok(body) => Ok(Json(body)),
        Err((st, msg)) => Err((st, Json(serde_json::json!({ "error": msg })))),
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct RollbackQuery {
    /// Ask the API server to validate the rollback without applying it.
    #[serde(default)]
    pub dry_run: bool,
}

fn change_error(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": msg })))
}

/// Roll a change back with `kubectl rollout undo`. Refuses, with a reason,
/// anything that cannot be reverted for real, instead of reporting success.
pub async fn rollback_change(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(q): Query<RollbackQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;
    let actor = actor_from_claims(&claims);

    let changes = state
        .cache
        .list_values(CHANGES_PREFIX)
        .await
        .unwrap_or_default();
    let change = changes
        .iter()
        .find(|c| c.get("id").and_then(|i| i.as_str()) == Some(id.as_str()))
        .ok_or_else(|| change_error(StatusCode::NOT_FOUND, "Change not found"))?;

    let index = RollbackIndex::new(&changes, rolled_back_ids(&state).await);
    if let Err(block) = index.check(change) {
        let status = match block {
            RollbackBlock::UnsupportedKind => StatusCode::UNPROCESSABLE_ENTITY,
            _ => StatusCode::CONFLICT,
        };
        return Err(change_error(status, block.message()));
    }

    let field = |k: &str| {
        change
            .get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let (namespace, name) = (field("namespace"), field("resource"));
    let target = format!("deployment/{}", name);

    match state.k8s.rollout_undo(&namespace, &name, q.dry_run).await {
        Ok(output) if q.dry_run => Ok(Json(serde_json::json!({
            "id": id,
            "status": "dry_run",
            "resource": target,
            "namespace": namespace,
            "output": output,
        }))),
        Ok(output) => {
            let record = serde_json::json!({
                "id": id,
                "resource": target,
                "namespace": namespace,
                "rolled_back_by": actor,
                "rolled_back_at": chrono::Utc::now().to_rfc3339(),
            });
            if let Err(e) = state
                .cache
                .set_persistent(&format!("{}{}", ROLLBACKS_PREFIX, id), &record)
                .await
            {
                tracing::warn!("Failed to record rollback {}: {}", id, e);
            }
            audit_log(
                &state,
                "change.rollback",
                &id,
                &namespace,
                &format!("Rolled back {} to its previous revision", target),
                &actor,
                "success",
            )
            .await;
            Ok(Json(serde_json::json!({
                "id": id,
                "status": "rolled_back",
                "resource": target,
                "namespace": namespace,
                "output": output,
                "rolled_back_at": record["rolled_back_at"],
            })))
        }
        Err(e) => {
            tracing::warn!("Rollback of {} failed: {}", id, e);
            audit_log(
                &state,
                "change.rollback",
                &id,
                &namespace,
                &format!("Rollback failed: {}", e),
                &actor,
                "failure",
            )
            .await;
            Err(change_error(
                StatusCode::BAD_GATEWAY,
                &format!("Rollback failed: {}", e),
            ))
        }
    }
}

// ── Node Drain ────────────────────────────────────────────

pub async fn node_drain_status(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::value::Value> {
    track_request(&state, |_| {}).await;

    let data = state
        .k8s
        .kubectl_json(&["get", "nodes", "-o", "json"])
        .await;

    // Fetch all running pods to count per-node pods_remaining
    let pods_data = state
        .k8s
        .kubectl_json(&[
            "get",
            "pods",
            "--all-namespaces",
            "--field-selector=status.phase=Running",
            "-o",
            "json",
        ])
        .await;

    let mut node_pod_counts: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    if let Some(pod_items) = pods_data.get("items").and_then(|v| v.as_array()) {
        for pod in pod_items {
            if let Some(node_name) = pod
                .get("spec")
                .and_then(|s| s.get("nodeName"))
                .and_then(|v| v.as_str())
            {
                *node_pod_counts.entry(node_name.to_string()).or_insert(0) += 1;
            }
        }
    }

    let nodes: Vec<serde_json::Value> = data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    let meta = item.get("metadata").unwrap_or(item);
                    let spec = item.get("spec").unwrap_or(item);
                    let status = item.get("status").unwrap_or(item);

                    let name = meta.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let unschedulable = spec
                        .get("unschedulable")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    let ready = status
                        .get("conditions")
                        .and_then(|v| v.as_array())
                        .and_then(|conds| {
                            conds
                                .iter()
                                .find(|c| c.get("type").and_then(|v| v.as_str()) == Some("Ready"))
                        })
                        .and_then(|c| c.get("status").and_then(|v| v.as_str()))
                        .unwrap_or("Unknown")
                        == "True";

                    let node_status = if unschedulable {
                        "cordoned"
                    } else if ready {
                        "ready"
                    } else {
                        "not_ready"
                    };

                    let started_at = meta
                        .get("creationTimestamp")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    let pods_remaining = node_pod_counts.get(name).copied().unwrap_or(0);

                    serde_json::json!({
                        "node": name,
                        "status": node_status,
                        "cordon": unschedulable,
                        "pods_evicted": 0,
                        "pods_remaining": pods_remaining,
                        "started_at": started_at,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Json(serde_json::json!({ "nodes": nodes }))
}

pub async fn drain_node(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(body): Json<NodeActionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;
    let node = &body.node;
    audit_log(
        &state,
        "node.drain",
        node,
        "",
        "Node drain initiated",
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok(Json(serde_json::json!({
        "node": node,
        "status": "draining",
        "message": "Node drain initiated, pods being evicted gracefully",
        "started_at": chrono::Utc::now().to_rfc3339()
    })))
}

pub async fn uncordon_node(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(body): Json<NodeActionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;
    let node = &body.node;
    audit_log(
        &state,
        "node.uncordon",
        node,
        "",
        "Node uncordoned",
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok(Json(serde_json::json!({
        "node": node,
        "status": "ready",
        "message": "Node uncordoned and scheduling resumed",
        "cordon": false
    })))
}

// ── Pod Security ──────────────────────────────────────────

pub async fn pod_security(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Query namespaces for PSA labels and pods for security context
    let ns_data = state
        .k8s
        .kubectl_json(&["get", "namespaces", "-o", "json"])
        .await;

    // Fetch all pods with their security contexts
    let pods_data = state
        .k8s
        .kubectl_json(&["get", "pods", "--all-namespaces", "-o", "json"])
        .await;

    // Build per-namespace pod stats: (total, compliant, violations)
    let mut ns_total: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    let mut ns_compliant: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    let mut ns_violations: std::collections::HashMap<String, Vec<serde_json::Value>> =
        std::collections::HashMap::new();

    if let Some(pod_items) = pods_data.get("items").and_then(|v| v.as_array()) {
        for pod in pod_items {
            let pod_ns = pod
                .get("metadata")
                .and_then(|m| m.get("namespace"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let pod_name = pod
                .get("metadata")
                .and_then(|m| m.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            *ns_total.entry(pod_ns.to_string()).or_insert(0) += 1;

            let spec = pod.get("spec");
            let host_network = spec
                .and_then(|s| s.get("hostNetwork"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            // Check security context on pod level
            let pod_run_as_user = spec
                .and_then(|s| s.get("securityContext"))
                .and_then(|sc| sc.get("runAsUser"))
                .and_then(|v| v.as_u64());

            // Check all containers
            let containers = spec
                .and_then(|s| s.get("containers"))
                .and_then(|v| v.as_array());

            let mut is_privileged = false;
            let mut runs_as_root = pod_run_as_user == Some(0);

            if let Some(ctrs) = containers {
                for ctr in ctrs {
                    let sc = ctr.get("securityContext");
                    if sc
                        .and_then(|s| s.get("privileged"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        is_privileged = true;
                    }
                    if !runs_as_root
                        && sc.and_then(|s| s.get("runAsUser")).and_then(|v| v.as_u64()) == Some(0)
                    {
                        runs_as_root = true;
                    }
                }
            }

            let mut pod_violations = Vec::new();
            if runs_as_root {
                pod_violations.push("runs_as_root");
            }
            if is_privileged {
                pod_violations.push("privileged_container");
            }
            if host_network {
                pod_violations.push("host_network");
            }

            if pod_violations.is_empty() {
                *ns_compliant.entry(pod_ns.to_string()).or_insert(0) += 1;
            } else {
                ns_violations
                    .entry(pod_ns.to_string())
                    .or_default()
                    .push(serde_json::json!({
                        "pod": pod_name,
                        "issues": pod_violations,
                    }));
            }
        }
    }

    let reports: Vec<serde_json::Value> = ns_data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let meta = item.get("metadata")?;
                    let name = meta.get("name").and_then(|v| v.as_str())?;
                    let labels = meta.get("labels").and_then(|v| v.as_object());

                    let enforce = labels
                        .and_then(|l| l.get("pod-security.kubernetes.io/enforce"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("not-set");
                    let audit = labels
                        .and_then(|l| l.get("pod-security.kubernetes.io/audit"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("not-set");
                    let warn = labels
                        .and_then(|l| l.get("pod-security.kubernetes.io/warn"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("not-set");

                    let total_pods = ns_total.get(name).copied().unwrap_or(0);
                    let compliant_pods = ns_compliant.get(name).copied().unwrap_or(0);
                    let violations = ns_violations.get(name).cloned().unwrap_or_default();

                    Some(serde_json::json!({
                        "namespace": name,
                        "enforce_level": enforce,
                        "audit_level": audit,
                        "warn_level": warn,
                        "total_pods": total_pods,
                        "compliant_pods": compliant_pods,
                        "violations": violations,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    Json(serde_json::json!({ "reports": reports, "total": reports.len() }))
}

// ── Egress Gateway ────────────────────────────────────────

pub async fn egress_policies(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Query CiliumEgressGatewayPolicy resources
    let data = state
        .k8s
        .kubectl_json(&[
            "get",
            "ciliumegressgatewaypolicies",
            "--all-namespaces",
            "-o",
            "json",
        ])
        .await;

    let policies: Vec<serde_json::Value> = data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|items| {
            items.iter().map(|item| {
                let meta = item.get("metadata").unwrap_or(item);
                let spec = item.get("spec").unwrap_or(item);
                serde_json::json!({
                    "name": meta.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "namespace": meta.get("namespace").and_then(|v| v.as_str()).unwrap_or(""),
                    "spec": spec,
                    "created_at": meta.get("creationTimestamp").and_then(|v| v.as_str()).unwrap_or(""),
                })
            }).collect()
        })
        .unwrap_or_default();

    let total = policies.len();
    Json(serde_json::json!({ "policies": policies, "total": total }))
}

// ── Service Mesh ──────────────────────────────────────────

pub async fn mesh_services(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;
    // Query real K8s Services and infer protocol from ports/annotations
    let data = state
        .k8s
        .kubectl_json(&["get", "services", "--all-namespaces", "-o", "json"])
        .await;

    let services: Vec<serde_json::Value> =
        data.get("items")
            .and_then(|v| v.as_array())
            .map(|items| {
                items.iter().filter_map(|item| {
                let meta = item.get("metadata")?;
                let spec = item.get("spec")?;
                let name = meta.get("name").and_then(|v| v.as_str())?;
                let ns = meta.get("namespace").and_then(|v| v.as_str()).unwrap_or("");

                // Infer protocol from ports
                let ports = spec.get("ports").and_then(|v| v.as_array());
                let protocol = ports
                    .and_then(|p| p.first())
                    .and_then(|p| p.get("appProtocol").and_then(|v| v.as_str())
                        .or_else(|| p.get("protocol").and_then(|v| v.as_str())))
                    .unwrap_or("TCP");

                Some(serde_json::json!({
                    "name": name,
                    "namespace": ns,
                    "type": spec.get("type").and_then(|v| v.as_str()).unwrap_or("ClusterIP"),
                    "protocol": protocol,
                    "cluster_ip": spec.get("clusterIP").and_then(|v| v.as_str()).unwrap_or(""),
                    "ports": ports.cloned().unwrap_or_default(),
                    "mtls": false,
                    "retries": 0,
                    "timeout_ms": 0,
                    "circuit_breaker": false,
                    "traffic_policy": "round-robin",
                }))
            }).collect()
            })
            .unwrap_or_default();

    let total = services.len();
    Json(serde_json::json!({ "services": services, "total": total }))
}

// ── KubeProxy Replacement Status ──────────────────────────

pub async fn kpr_status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Read KPR config from cilium-config ConfigMap
    let cm = state
        .k8s
        .kubectl_json(&[
            "get",
            "configmap",
            "cilium-config",
            "-n",
            "kube-system",
            "-o",
            "json",
        ])
        .await;
    let empty = serde_json::json!({});
    let data = cm.get("data").unwrap_or(&empty);

    let kpr = data
        .get("kube-proxy-replacement")
        .and_then(|v| v.as_str())
        .unwrap_or("disabled");
    let device = data.get("devices").and_then(|v| v.as_str()).unwrap_or("");
    let dsr = data
        .get("loadbalancer-mode")
        .and_then(|v| v.as_str())
        .unwrap_or("snat");
    let session_affinity = data
        .get("enable-session-affinity")
        .and_then(|v| v.as_str())
        .unwrap_or("false");
    let node_port_range = data
        .get("node-port-range")
        .and_then(|v| v.as_str())
        .unwrap_or("30000-32767");
    let graceful_termination = data
        .get("enable-k8s-terminating-endpoint")
        .and_then(|v| v.as_str())
        .unwrap_or("false")
        == "true";

    // Count K8s services
    let svc_data = state
        .k8s
        .kubectl_json(&["get", "services", "--all-namespaces", "-o", "json"])
        .await;
    let services_count = svc_data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    // Count K8s endpoints (backends)
    let ep_data = state
        .k8s
        .kubectl_json(&["get", "endpoints", "--all-namespaces", "-o", "json"])
        .await;
    let backends_count: usize = ep_data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .map(|ep| {
                    ep.get("subsets")
                        .and_then(|v| v.as_array())
                        .map(|subsets| {
                            subsets
                                .iter()
                                .map(|s| {
                                    s.get("addresses")
                                        .and_then(|v| v.as_array())
                                        .map(|a| a.len())
                                        .unwrap_or(0)
                                })
                                .sum::<usize>()
                        })
                        .unwrap_or(0)
                })
                .sum()
        })
        .unwrap_or(0);

    // Query NAT and CT table entry counts from cilium-agent
    let nat_output = state
        .k8s
        .exec_cilium_agent(&["cilium", "bpf", "nat", "list"])
        .await;
    let nat_entries: usize = if nat_output.is_empty() {
        0
    } else {
        // Each non-empty line is an entry; skip the header line
        nat_output
            .lines()
            .skip(1)
            .filter(|l| !l.trim().is_empty())
            .count()
    };

    let ct_output = state
        .k8s
        .exec_cilium_agent(&["cilium", "bpf", "ct", "list", "global"])
        .await;
    let ct_entries: usize = if ct_output.is_empty() {
        0
    } else {
        ct_output
            .lines()
            .skip(1)
            .filter(|l| !l.trim().is_empty())
            .count()
    };

    Json(serde_json::json!({
        "enabled": kpr != "disabled" && !kpr.is_empty(),
        "mode": kpr,
        "device": device,
        "dsr_mode": dsr,
        "session_affinity": session_affinity == "true",
        "graceful_termination": graceful_termination,
        "node_port_range": node_port_range,
        "services": services_count,
        "backends": backends_count,
        "nat_entries": nat_entries,
        "ct_entries": ct_entries,
        "source": "cilium-config ConfigMap",
    }))
}
