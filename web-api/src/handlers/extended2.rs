// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

use super::{
    actor_from_claims, audit_log as emit_audit, check_admin, paginate_json, track_request,
    PaginationQuery,
};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use std::sync::Arc;

// ── Host Info ───────────────────────────────────────────────

pub async fn host_info(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;

    use crate::services::k8s::K8sService;
    let hostname = K8sService::run_cmd("hostname", &[]).await;
    let kernel = K8sService::run_cmd("uname", &["-r"]).await;
    let arch = K8sService::run_cmd("uname", &["-m"]).await;
    let os_release = K8sService::run_cmd(
        "sh",
        &[
            "-c",
            "grep PRETTY_NAME /etc/os-release | cut -d= -f2 | tr -d '\"'",
        ],
    )
    .await;
    let cpu_count = K8sService::run_cmd("nproc", &[]).await;
    let uptime_str = K8sService::run_cmd("sh", &["-c", "cat /proc/uptime | cut -d' ' -f1"]).await;
    let load_str = K8sService::run_cmd("sh", &["-c", "cat /proc/loadavg"]).await;

    let uptime: f64 = uptime_str.parse().unwrap_or(0.0);
    let load_parts: Vec<f64> = load_str
        .split_whitespace()
        .take(3)
        .map(|s| s.parse().unwrap_or(0.0))
        .collect();

    // Parse /proc/meminfo
    let meminfo = K8sService::run_cmd(
        "sh",
        &["-c", "grep -E '^(MemTotal|MemAvailable):' /proc/meminfo"],
    )
    .await;
    let mut mem_total_kb: f64 = 0.0;
    let mut mem_available_kb: f64 = 0.0;
    for line in meminfo.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            match parts[0] {
                "MemTotal:" => mem_total_kb = parts[1].parse().unwrap_or(0.0),
                "MemAvailable:" => mem_available_kb = parts[1].parse().unwrap_or(0.0),
                _ => {}
            }
        }
    }

    // Parse disk usage from df
    let df_output = K8sService::run_cmd("sh", &["-c", "df -B1 / | tail -1"]).await;
    let df_parts: Vec<&str> = df_output.split_whitespace().collect();
    let disk_total_gb = df_parts
        .get(1)
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
        / 1_073_741_824.0;
    let disk_used_gb = df_parts
        .get(2)
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
        / 1_073_741_824.0;

    // CPU model
    let cpu_model = K8sService::run_cmd(
        "sh",
        &[
            "-c",
            "grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | xargs",
        ],
    )
    .await;

    // CPU usage from /proc/stat (1-second sample)
    let cpu_usage_str = K8sService::run_cmd("sh", &["-c",
        "read c1 u1 n1 s1 i1 < <(head -1 /proc/stat | awk '{print $2,$3,$4,$5,$6}') && sleep 1 && \
         read c2 u2 n2 s2 i2 < <(head -1 /proc/stat | awk '{print $2,$3,$4,$5,$6}') && \
         echo $(( (c2+u2+n2+s2 - c1-u1-n1-s1) * 100 / (c2+u2+n2+s2+i2 - c1-u1-n1-s1-i1) ))"
    ]).await;
    let cpu_usage: f64 = cpu_usage_str.trim().parse().unwrap_or(0.0);

    // Network interfaces
    let ip_output =
        K8sService::run_cmd("sh", &["-c", "ip -j addr show 2>/dev/null || echo '[]'"]).await;
    let net_interfaces: Vec<serde_json::Value> =
        serde_json::from_str::<Vec<serde_json::Value>>(&ip_output)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|iface| {
                let name = iface.get("ifname")?.as_str()?.to_string();
                let state = iface
                    .get("operstate")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_lowercase();
                let mac = iface
                    .get("address")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let ip = iface
                    .get("addr_info")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| {
                        arr.iter()
                            .find(|a| a.get("family").and_then(|f| f.as_str()) == Some("inet"))
                    })
                    .and_then(|a| a.get("local").and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string();
                Some(serde_json::json!({
                    "name": name,
                    "status": if state == "up" { "up" } else { "down" },
                    "ip": ip,
                    "mac": mac,
                }))
            })
            .collect();

    Ok(Json(serde_json::json!({
        "hostname": hostname,
        "os": if os_release.is_empty() { "Linux".to_string() } else { os_release },
        "kernel": kernel,
        "arch": arch,
        "cpu_cores": cpu_count.parse::<u32>().unwrap_or(0),
        "cpu_model": if cpu_model.is_empty() { serde_json::Value::Null } else { serde_json::json!(cpu_model) },
        "cpu_usage": cpu_usage,
        "memory_total_gb": (mem_total_kb / 1_048_576.0 * 10.0).round() / 10.0,
        "memory_used_gb": ((mem_total_kb - mem_available_kb) / 1_048_576.0 * 10.0).round() / 10.0,
        "disk_total_gb": (disk_total_gb * 10.0).round() / 10.0,
        "disk_used_gb": (disk_used_gb * 10.0).round() / 10.0,
        "uptime_seconds": uptime as u64,
        "load_average": load_parts,
        "network_interfaces": net_interfaces,
    })))
}

// ── Policy Templates ────────────────────────────────────────

pub async fn list_policy_templates(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;
    let items: Vec<serde_json::Value> = vec![
        serde_json::json!({ "id": "tpl-001", "name": "Default Deny All", "category": "security", "description": "Deny all ingress and egress traffic by default", "tags": ["zero-trust", "baseline"],
          "yaml": "apiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy\nmetadata:\n  name: default-deny\nspec:\n  endpointSelector: {}\n  ingress: []\n  egress: []" }),
        serde_json::json!({ "id": "tpl-002", "name": "Allow DNS", "category": "connectivity", "description": "Allow DNS resolution to kube-dns", "tags": ["dns", "essential"],
          "yaml": "apiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy\nmetadata:\n  name: allow-dns\nspec:\n  endpointSelector: {}\n  egress:\n  - toEndpoints:\n    - matchLabels:\n        k8s:io.kubernetes.pod.namespace: kube-system\n        k8s-app: kube-dns\n    toPorts:\n    - ports:\n      - port: \"53\"\n        protocol: UDP" }),
        serde_json::json!({ "id": "tpl-003", "name": "Allow HTTP Ingress", "category": "connectivity", "description": "Allow HTTP/HTTPS ingress from any source", "tags": ["http", "ingress"],
          "yaml": "apiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy\nmetadata:\n  name: allow-http\nspec:\n  endpointSelector:\n    matchLabels:\n      app: web\n  ingress:\n  - toPorts:\n    - ports:\n      - port: \"80\"\n      - port: \"443\"" }),
        serde_json::json!({ "id": "tpl-004", "name": "Namespace Isolation", "category": "security", "description": "Restrict traffic to same namespace only", "tags": ["isolation", "namespace"],
          "yaml": "apiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy\nmetadata:\n  name: namespace-isolation\nspec:\n  endpointSelector: {}\n  ingress:\n  - fromEndpoints:\n    - matchLabels:\n        io.kubernetes.pod.namespace: ${NAMESPACE}" }),
        serde_json::json!({ "id": "tpl-005", "name": "Allow Monitoring", "category": "observability", "description": "Allow Prometheus scraping on metrics port", "tags": ["prometheus", "monitoring"],
          "yaml": "apiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy\nmetadata:\n  name: allow-monitoring\nspec:\n  endpointSelector: {}\n  ingress:\n  - fromEndpoints:\n    - matchLabels:\n        app: prometheus\n    toPorts:\n    - ports:\n      - port: \"9090\"" }),
        serde_json::json!({ "id": "tpl-006", "name": "L7 HTTP Policy", "category": "l7", "description": "L7 policy allowing only GET and POST methods", "tags": ["l7", "http", "advanced"],
          "yaml": "apiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy\nmetadata:\n  name: l7-http\nspec:\n  endpointSelector:\n    matchLabels:\n      app: api\n  ingress:\n  - toPorts:\n    - ports:\n      - port: \"8080\"\n      rules:\n        http:\n        - method: GET\n        - method: POST" }),
    ];
    Json(paginate_json(items, &params, "templates"))
}

pub async fn apply_template(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;
    emit_audit(
        &state,
        "template.apply",
        &id,
        "",
        "Template applied",
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok(Json(
        serde_json::json!({ "id": id, "status": "applied", "message": "Template applied successfully" }),
    ))
}

// ── Diagnostics ─────────────────────────────────────────────

pub async fn run_diagnostics(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    use crate::services::k8s::K8sService;
    let mut tests = Vec::new();

    // Test 1: K8s API health
    let k8s_ok = state.k8s.is_healthy().await;
    tests.push(serde_json::json!({
        "name": "Kubernetes API",
        "status": if k8s_ok { "pass" } else { "fail" },
        "message": if k8s_ok { "API server reachable" } else { "API server unreachable" },
    }));

    // Test 2: Cilium agents
    let cilium_pods = state
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
    let agent_count = cilium_pods
        .get("items")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    tests.push(serde_json::json!({
        "name": "Cilium Agents",
        "status": if agent_count > 0 { "pass" } else { "fail" },
        "message": format!("{} cilium agent pods found", agent_count),
    }));

    // Test 3: Hubble relay
    let hubble_ok = state.hubble.is_healthy().await;
    tests.push(serde_json::json!({
        "name": "Hubble Relay",
        "status": if hubble_ok { "pass" } else { "warn" },
        "message": if hubble_ok { format!("Connected to {}", state.hubble.address()) } else { "Relay unreachable".to_string() },
    }));

    // Test 4: Kernel version
    let kernel = K8sService::run_cmd("uname", &["-r"]).await;
    tests.push(serde_json::json!({
        "name": "Kernel Version",
        "status": "pass",
        "message": format!("Kernel {}", kernel),
    }));

    // Test 5: BPF filesystem
    let bpf_mount = K8sService::run_cmd(
        "sh",
        &[
            "-c",
            "mountpoint -q /sys/fs/bpf && echo mounted || echo not_mounted",
        ],
    )
    .await;
    tests.push(serde_json::json!({
        "name": "BPF Filesystem",
        "status": if bpf_mount.contains("mounted") { "pass" } else { "warn" },
        "message": if bpf_mount.contains("mounted") { "/sys/fs/bpf mounted" } else { "/sys/fs/bpf not mounted" },
    }));

    // Test 6: Network policies count
    let policies = state.k8s.list_policies().await.unwrap_or_default();
    tests.push(serde_json::json!({
        "name": "Network Policies",
        "status": if policies.is_empty() { "warn" } else { "pass" },
        "message": format!("{} CiliumNetworkPolicies found", policies.len()),
    }));

    Ok(Json(
        serde_json::json!({ "tests": tests, "total": tests.len() }),
    ))
}

pub async fn connectivity_test(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    // Test connectivity by checking K8s API and Hubble
    let k8s_ok = state.k8s.is_healthy().await;
    let hubble_ok = state.hubble.is_healthy().await;

    Ok(Json(serde_json::json!({
        "kubernetes_api": k8s_ok,
        "hubble_relay": hubble_ok,
        "hubble_address": state.hubble.address(),
        "overall": k8s_ok,
    })))
}

// ── Audit Log ───────────────────────────────────────────────

const AUDIT_LOG_PREFIX: &str = "cv:audit_log:";

pub async fn audit_log(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;

    // Log this access as an audit entry
    let actor = claims
        .as_ref()
        .map(|c| c.sub.clone())
        .unwrap_or_else(|| "anonymous".to_string());
    let access_id = format!(
        "aud-{}",
        uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("000")
    );
    let access_entry = serde_json::json!({
        "id": access_id,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "action": "audit_log.access",
        "actor": actor,
        "resource": "AuditLog",
        "namespace": "-",
        "details": "Audit log endpoint accessed",
        "outcome": "success",
    });
    let _ = state
        .cache
        .set_persistent(&format!("{}{}", AUDIT_LOG_PREFIX, access_id), &access_entry)
        .await;

    // Retrieve all audit entries from cache
    let entries = state
        .cache
        .list_values(AUDIT_LOG_PREFIX)
        .await
        .unwrap_or_default();
    let total = entries.len();
    Ok(Json(
        serde_json::json!({ "entries": entries, "total": total }),
    ))
}

// ── Alerts ──────────────────────────────────────────────────

const ALERT_RULES_PREFIX: &str = "cv:alert_rules:";
const ALERT_RULES_SEEDED_KEY: &str = "cv:alert_rules_seeded";
/// Ids of the rules seeded by `list_alert_rules`; deleting one needs `?force=true`.
const BUILTIN_ALERT_RULES: [&str; 5] = ["rule-001", "rule-002", "rule-003", "rule-004", "rule-005"];
const ALERT_SEVERITIES: [&str; 4] = ["critical", "high", "warning", "info"];

pub async fn list_alert_rules(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    let items = state
        .cache
        .list_values(ALERT_RULES_PREFIX)
        .await
        .unwrap_or_default();

    // Seed default rules once: after the marker is set, an empty list stays empty.
    let seeded = state
        .cache
        .get::<bool>(ALERT_RULES_SEEDED_KEY)
        .await
        .ok()
        .flatten()
        .unwrap_or(false);
    if items.is_empty() && !seeded {
        let _ = state
            .cache
            .set_persistent(ALERT_RULES_SEEDED_KEY, &true)
            .await;
        let defaults = vec![
            serde_json::json!({ "id": "rule-001", "name": "High Drop Rate", "condition": "drop_rate > 5% for 5m", "severity": "critical", "enabled": true }),
            serde_json::json!({ "id": "rule-002", "name": "DNS Resolution Failure", "condition": "dns_servfail > 10/min", "severity": "high", "enabled": true }),
            serde_json::json!({ "id": "rule-003", "name": "Policy Deny Spike", "condition": "policy_denied > 100/min", "severity": "warning", "enabled": true }),
            serde_json::json!({ "id": "rule-004", "name": "Endpoint Unhealthy", "condition": "endpoint_status != ready", "severity": "high", "enabled": false }),
            serde_json::json!({ "id": "rule-005", "name": "CT Table Near Full", "condition": "ct_entries > 90% max", "severity": "warning", "enabled": true }),
        ];
        for rule in &defaults {
            let id = rule["id"].as_str().unwrap_or("unknown");
            let _ = state
                .cache
                .set_persistent(&format!("{}{}", ALERT_RULES_PREFIX, id), rule)
                .await;
        }
        return Json(paginate_json(defaults, &params, "rules"));
    }

    Json(paginate_json(items, &params, "rules"))
}

const ALERT_HISTORY_PREFIX: &str = "cv:alert_history:";

pub async fn alert_history(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;
    let alerts = state
        .cache
        .list_values(ALERT_HISTORY_PREFIX)
        .await
        .unwrap_or_default();
    Json(serde_json::json!({ "alerts": alerts, "total": alerts.len() }))
}

pub async fn toggle_alert_rule(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let key = format!("{}{}", ALERT_RULES_PREFIX, id);
    if let Ok(Some(mut rule)) = state.cache.get::<serde_json::Value>(&key).await {
        let enabled = rule
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        rule["enabled"] = serde_json::json!(!enabled);
        let _ = state.cache.set_persistent(&key, &rule).await;
        emit_audit(
            &state,
            "alert.toggle",
            &id,
            "",
            &format!("Alert rule toggled to {}", !enabled),
            &actor_from_claims(&claims),
            "success",
        )
        .await;
        Ok(Json(
            serde_json::json!({ "id": id, "enabled": !enabled, "status": "updated" }),
        ))
    } else {
        Ok(Json(serde_json::json!({ "id": id, "status": "not_found" })))
    }
}

#[derive(serde::Deserialize)]
pub struct AlertRuleRequest {
    pub name: String,
    pub condition: String,
    pub severity: String,
    pub enabled: Option<bool>,
}

#[derive(Default, serde::Deserialize)]
pub struct DeleteAlertRuleQuery {
    #[serde(default)]
    pub force: bool,
}

type HandlerError = (StatusCode, Json<serde_json::Value>);

fn bad_request(msg: impl Into<String>) -> HandlerError {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg.into() })),
    )
}

/// Validate a rule body. `keep_condition` lets an edit leave a stored
/// condition unchanged even if the engine cannot evaluate it (seeded
/// `ct_entries` rule).
fn validate_alert_rule(
    req: &AlertRuleRequest,
    keep_condition: Option<&str>,
) -> Result<(), HandlerError> {
    let name = req.name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(bad_request("name must be 1-100 characters"));
    }
    if req.condition.len() > 200 {
        return Err(bad_request("condition must be at most 200 characters"));
    }
    if !ALERT_SEVERITIES.contains(&req.severity.as_str()) {
        return Err(bad_request(format!(
            "severity must be one of {}",
            ALERT_SEVERITIES.join(", ")
        )));
    }
    if keep_condition.is_some_and(|c| c == req.condition.trim()) {
        return Ok(());
    }
    crate::services::alerting::validate_condition(&req.condition).map_err(bad_request)
}

pub async fn create_alert_rule(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(req): Json<AlertRuleRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), HandlerError> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;
    validate_alert_rule(&req, None)?;

    let id = format!("rule-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let rule = serde_json::json!({
        "id": id,
        "name": req.name.trim(),
        "condition": req.condition.trim(),
        "severity": req.severity,
        "enabled": req.enabled.unwrap_or(true),
    });
    state
        .cache
        .set_persistent(&format!("{}{}", ALERT_RULES_PREFIX, id), &rule)
        .await
        .map_err(|e| {
            tracing::error!("Failed to save alert rule: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to save alert rule" })),
            )
        })?;
    emit_audit(
        &state,
        "alert.rule.create",
        &id,
        "",
        &format!("Alert rule '{}' created", req.name.trim()),
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok((StatusCode::CREATED, Json(rule)))
}

pub async fn update_alert_rule(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
    Json(req): Json<AlertRuleRequest>,
) -> Result<Json<serde_json::Value>, HandlerError> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let key = format!("{}{}", ALERT_RULES_PREFIX, id);
    let Ok(Some(mut rule)) = state.cache.get::<serde_json::Value>(&key).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Alert rule not found" })),
        ));
    };
    let stored_condition = rule
        .get("condition")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    validate_alert_rule(&req, Some(&stored_condition))?;

    // Edit only the definition; keep id, trigger history and (unless given) enabled.
    rule["name"] = serde_json::json!(req.name.trim());
    rule["condition"] = serde_json::json!(req.condition.trim());
    rule["severity"] = serde_json::json!(req.severity);
    if let Some(enabled) = req.enabled {
        rule["enabled"] = serde_json::json!(enabled);
    }
    state.cache.set_persistent(&key, &rule).await.map_err(|e| {
        tracing::error!("Failed to save alert rule: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Failed to save alert rule" })),
        )
    })?;
    emit_audit(
        &state,
        "alert.rule.update",
        &id,
        "",
        &format!("Alert rule '{}' updated", req.name.trim()),
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok(Json(rule))
}

pub async fn delete_alert_rule(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
    Query(q): Query<DeleteAlertRuleQuery>,
) -> Result<StatusCode, HandlerError> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    if BUILTIN_ALERT_RULES.contains(&id.as_str()) && !q.force {
        return Err(bad_request(
            "built-in rule: disable it instead, or pass force=true to delete",
        ));
    }
    let key = format!("{}{}", ALERT_RULES_PREFIX, id);
    if !matches!(
        state.cache.get::<serde_json::Value>(&key).await,
        Ok(Some(_))
    ) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Alert rule not found" })),
        ));
    }
    let _ = state.cache.delete(&key).await;
    emit_audit(
        &state,
        "alert.rule.delete",
        &id,
        "",
        "Alert rule deleted",
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ── Service Map ─────────────────────────────────────────────

pub async fn service_map(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Build service map from real Hubble flows
    let flows = state.hubble.get_flows(500, None).await.unwrap_or_default();

    let mut svc_set = std::collections::HashSet::new();
    // (protocol, flow_count, dropped_count)
    let mut edge_map: std::collections::HashMap<(String, String), (String, u64, u64)> =
        std::collections::HashMap::new();

    for flow in &flows {
        let src = if !flow.source.pod.is_empty() {
            flow.source
                .pod
                .split('-')
                .take(2)
                .collect::<Vec<_>>()
                .join("-")
        } else {
            continue;
        };
        let dst = if !flow.destination.pod.is_empty() {
            flow.destination
                .pod
                .split('-')
                .take(2)
                .collect::<Vec<_>>()
                .join("-")
        } else {
            continue;
        };
        if src == dst {
            continue;
        }

        svc_set.insert((src.clone(), flow.source.namespace.clone()));
        svc_set.insert((dst.clone(), flow.destination.namespace.clone()));

        let key = (src, dst);
        let proto = if flow.http_method.is_some() {
            "HTTP".to_string()
        } else {
            flow.protocol.clone()
        };
        let entry = edge_map.entry(key).or_insert((proto, 0, 0));
        entry.1 += 1;
        if flow.verdict == "DROPPED" {
            entry.2 += 1;
        }
    }

    let nodes: Vec<serde_json::Value> = svc_set
        .iter()
        .map(|(name, ns)| serde_json::json!({ "name": name, "namespace": ns, "status": "healthy" }))
        .collect();

    let edges: Vec<serde_json::Value> = edge_map
        .iter()
        .map(|((src, dst), (proto, count, dropped))| {
            let error_rate = if *count > 0 {
                (*dropped as f64 / *count as f64) * 100.0
            } else {
                0.0
            };
            serde_json::json!({
                "source": src,
                "target": dst,
                "protocol": proto,
                "flow_count": count,
                "dropped_count": dropped,
                "error_rate": (error_rate * 100.0).round() / 100.0
            })
        })
        .collect();

    Json(serde_json::json!({ "nodes": nodes, "edges": edges }))
}

// ── Packet Capture ──────────────────────────────────────────

const CAPTURES_PREFIX: &str = "cv:captures:";

pub async fn list_capture_sessions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;
    let items = state
        .cache
        .list_values(CAPTURES_PREFIX)
        .await
        .unwrap_or_default();
    Json(paginate_json(items, &params, "sessions"))
}

pub async fn start_capture(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let id = format!(
        "cap-{}",
        uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("000")
    );
    let session = serde_json::json!({
        "id": id,
        "status": "capturing",
        "started_at": chrono::Utc::now().to_rfc3339(),
    });
    let _ = state
        .cache
        .set_persistent(&format!("{}{}", CAPTURES_PREFIX, id), &session)
        .await;
    emit_audit(
        &state,
        "capture.start",
        &id,
        "",
        "Capture started",
        &actor_from_claims(&claims),
        "success",
    )
    .await;

    Ok(Json(
        serde_json::json!({ "id": id, "status": "capturing", "message": "Capture started" }),
    ))
}

pub async fn stop_capture(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    super::check_editor(&state, &claims)?;
    track_request(&state, |_| {}).await;

    let key = format!("{}{}", CAPTURES_PREFIX, id);
    if let Ok(Some(mut session)) = state.cache.get::<serde_json::Value>(&key).await {
        session["status"] = serde_json::json!("completed");
        session["stopped_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
        let _ = state.cache.set_persistent(&key, &session).await;
    }
    emit_audit(
        &state,
        "capture.stop",
        &id,
        "",
        "Capture stopped",
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok(Json(
        serde_json::json!({ "id": id, "status": "completed", "message": "Capture stopped" }),
    ))
}

// ── DNS Monitor ─────────────────────────────────────────────

fn dns_rcode_label(code: Option<u32>, name: Option<&str>) -> String {
    if let Some(n) = name.filter(|s| !s.is_empty()) {
        return n.to_string();
    }
    code.map(crate::models::flow::dns_rcode_name)
        .unwrap_or("UNKNOWN")
        .to_string()
}

pub async fn dns_queries(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;

    // Prefer indexed store (continuous ingest); fall back to a live snapshot.
    let stored = state
        .flow_store
        .query(&crate::services::flow_store::FlowQuery {
            port: Some(53),
            limit: 500,
            ..Default::default()
        })
        .unwrap_or_default();

    let live = if stored.is_empty() {
        state.hubble.get_flows(500, None).await.unwrap_or_default()
    } else {
        Vec::new()
    };

    let mut queries: Vec<serde_json::Value> = Vec::new();

    // L7 DNS from live flows
    for f in &live {
        if f.port != 53 && f.dns_query.is_none() {
            continue;
        }
        let has_l7 = f.dns_query.is_some();
        let response_code = if has_l7 {
            dns_rcode_label(f.dns_rcode, f.dns_rcode_name.as_deref())
        } else {
            // Never invent SERVFAIL from a policy drop — L4-only is incomplete.
            "UNKNOWN".into()
        };
        let policy_corr = if f.verdict.eq_ignore_ascii_case("DROPPED") {
            serde_json::json!({
                "confidence": "observed",
                "detail": "Matching flow was DROPPED by the datapath; may correlate with a policy deny.",
                "drop_reason": f.drop_reason,
            })
        } else {
            serde_json::json!({ "confidence": "unavailable", "detail": null })
        };
        queries.push(serde_json::json!({
            "id": f.id,
            "timestamp": f.timestamp,
            "source_pod": f.source.pod,
            "namespace": f.source.namespace,
            "query_name": f.dns_query.clone().unwrap_or_default(),
            "query_type": f.dns_qtypes.as_ref().and_then(|q| q.first().cloned()).unwrap_or_else(|| {
                if f.protocol == "UDP" { "A".into() } else { "AAAA".into() }
            }),
            "response_code": response_code,
            "response_ips": f.dns_ips.clone().unwrap_or_default(),
            "latency_ms": f.dns_latency_ns.map(|ns| (ns as f64) / 1_000_000.0).unwrap_or(0.0),
            "verdict": f.verdict,
            "confidence": if has_l7 { "observed" } else { "unavailable" },
            "evidence": if has_l7 { "l7_dns" } else { "l4_only" },
            "policy_correlation": policy_corr,
        }));
    }

    // From store: only emit as DNS when we have L7 fields or port 53 with clear incomplete marker
    for s in &stored {
        // StoredFlow does not yet carry DNS L7 — emit L4 port-53 with unavailable confidence
        // unless we can pull matching live L7 (already handled above).
        if s.port != 53 {
            continue;
        }
        if queries.iter().any(|q| q.get("id").and_then(|v| v.as_str()) == Some(&s.id)) {
            continue;
        }
        let policy_corr = if s.verdict.eq_ignore_ascii_case("DROPPED") {
            serde_json::json!({
                "confidence": "observed",
                "detail": "Port-53 flow was DROPPED; DNS rcode unknown without L7 visibility.",
                "drop_reason": s.drop_reason,
            })
        } else {
            serde_json::json!({ "confidence": "unavailable", "detail": null })
        };
        queries.push(serde_json::json!({
            "id": s.id,
            "timestamp": s.ts,
            "source_pod": s.src_pod,
            "namespace": s.src_namespace,
            "query_name": "",
            "query_type": "",
            "response_code": "UNKNOWN",
            "response_ips": [],
            "latency_ms": 0.0,
            "verdict": s.verdict,
            "confidence": "unavailable",
            "evidence": "l4_only",
            "policy_correlation": policy_corr,
        }));
    }

    let total = queries.len();
    Ok(Json(
        serde_json::json!({ "queries": queries, "total": total }),
    ))
}

pub async fn dns_stats(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    let flows = state.hubble.get_flows(1000, None).await.unwrap_or_default();
    let dns_l7: Vec<_> = flows.iter().filter(|f| f.dns_query.is_some()).collect();
    let dns_l4: Vec<_> = flows
        .iter()
        .filter(|f| f.port == 53 && f.dns_query.is_none())
        .collect();

    let total = dns_l7.len() as u64 + dns_l4.len() as u64;
    let failures = dns_l7
        .iter()
        .filter(|f| f.dns_rcode.map(|c| c != 0).unwrap_or(false))
        .count() as u64;
    let dropped = flows
        .iter()
        .filter(|f| f.port == 53 && f.verdict.eq_ignore_ascii_case("DROPPED"))
        .count() as u64;
    let avg_latency_ms = {
        let samples: Vec<f64> = dns_l7
            .iter()
            .filter_map(|f| f.dns_latency_ns.map(|ns| ns as f64 / 1_000_000.0))
            .collect();
        if samples.is_empty() {
            0.0
        } else {
            samples.iter().sum::<f64>() / samples.len() as f64
        }
    };

    Json(serde_json::json!({
        "total_queries": total,
        "total": total,
        "successful": dns_l7.iter().filter(|f| f.dns_rcode == Some(0)).count() as u64,
        "failures": failures,
        "dropped": dropped,
        "l7_observed": dns_l7.len(),
        "l4_only": dns_l4.len(),
        "avg_latency_ms": (avg_latency_ms * 10.0).round() / 10.0,
        "drop_rate": if total > 0 { dropped as f64 / total as f64 * 100.0 } else { 0.0 },
        "source": if dns_l7.is_empty() {
            "hubble L4 flows (port 53); enable Cilium DNS visibility for L7"
        } else {
            "hubble L7 DNS"
        },
    }))
}

// ── Identities ──────────────────────────────────────────────

pub async fn list_identities(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    let data = state
        .k8s
        .kubectl_json(&["get", "ciliumidentities", "-o", "json"])
        .await;

    let items: Vec<serde_json::Value> = data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter().map(|item| {
                let meta = item.get("metadata").unwrap_or(item);
                let id = meta.get("name").and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                let labels: Vec<String> = item
                    .get("security-labels")
                    .and_then(|v| v.as_object())
                    .map(|obj| obj.keys().cloned().collect())
                    .unwrap_or_default();
                let namespace = labels.iter()
                    .find_map(|l| l.strip_prefix("k8s:io.kubernetes.pod.namespace="))
                    .unwrap_or("")
                    .to_string();
                serde_json::json!({
                    "id": id,
                    "labels": labels,
                    "namespace": namespace,
                    "created_at": meta.get("creationTimestamp").and_then(|v| v.as_str()).unwrap_or(""),
                })
            }).collect()
        })
        .unwrap_or_default();

    Json(paginate_json(items, &params, "identities"))
}

// ── Cluster Mesh ────────────────────────────────────────────

pub async fn list_mesh_peers(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Query ClusterMesh status via Cilium
    let mesh_output = state
        .k8s
        .exec_cilium_agent(&["cilium", "clustermesh", "status", "-o", "json"])
        .await;

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&mesh_output) {
        if let Some(clusters) = parsed.get("clusters").and_then(|v| v.as_array()) {
            let items: Vec<serde_json::Value> = clusters.clone();
            return Json(paginate_json(items, &params, "peers"));
        }
    }

    // Fallback: check for clustermesh-apiserver pods
    let data = state
        .k8s
        .kubectl_json(&[
            "get",
            "pods",
            "-n",
            "kube-system",
            "-l",
            "k8s-app=clustermesh-apiserver",
            "-o",
            "json",
        ])
        .await;
    let count = data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    let items = if count > 0 {
        vec![serde_json::json!({
            "message": "ClusterMesh apiserver running but no remote clusters connected",
            "apiserver_pods": count,
        })]
    } else {
        vec![serde_json::json!({
            "message": "ClusterMesh not installed",
            "apiserver_pods": 0,
        })]
    };

    Json(paginate_json(items, &params, "peers"))
}

pub async fn connect_mesh_peer(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;
    Err(super::not_implemented(
        "ClusterMesh peer connection",
        "no peer was connected. Use `cilium clustermesh connect` for now",
    ))
}

// ── BGP Peering ─────────────────────────────────────────────

pub async fn list_bgp_peers(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Query CiliumBGPPeeringPolicy resources
    let data = state
        .k8s
        .kubectl_json(&["get", "ciliumbgppeeringpolicies", "-o", "json"])
        .await;

    let items: Vec<serde_json::Value> = data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter().map(|item| {
                let meta = item.get("metadata").unwrap_or(item);
                let spec = item.get("spec").unwrap_or(item);
                serde_json::json!({
                    "name": meta.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "spec": spec,
                    "created_at": meta.get("creationTimestamp").and_then(|v| v.as_str()).unwrap_or(""),
                })
            }).collect()
        })
        .unwrap_or_default();

    Json(paginate_json(items, &params, "peers"))
}

// ── Bandwidth ───────────────────────────────────────────────

pub async fn bandwidth_data(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    track_request(&state, |_| {}).await;

    // Aggregate bandwidth from Hubble flow data
    let flows = state.hubble.get_flows(500, None).await.unwrap_or_default();

    let mut pod_stats: std::collections::HashMap<(String, String), (u64, u64)> =
        std::collections::HashMap::new();
    for flow in &flows {
        if !flow.source.pod.is_empty() {
            let key = (flow.source.pod.clone(), flow.source.namespace.clone());
            pod_stats.entry(key).or_default().1 += 1; // tx
        }
        if !flow.destination.pod.is_empty() {
            let key = (
                flow.destination.pod.clone(),
                flow.destination.namespace.clone(),
            );
            pod_stats.entry(key).or_default().0 += 1; // rx
        }
    }

    let entries: Vec<serde_json::Value> = pod_stats
        .into_iter()
        .map(|((pod, ns), (rx, tx))| {
            serde_json::json!({
                "pod": pod,
                "namespace": ns,
                "rx_flows": rx,
                "tx_flows": tx,
                "total_flows": rx + tx,
            })
        })
        .collect();

    Json(
        serde_json::json!({ "entries": entries, "total": entries.len(), "source": "hubble flow counts" }),
    )
}
