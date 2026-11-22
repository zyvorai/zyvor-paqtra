// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Compliance endpoints
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

use super::{actor_from_claims, audit_log, to_json, track_request};
use crate::services::compliance::{self, AuditResult};
use crate::AppState;

/// Typed request for the compliance audit endpoint.
#[derive(Debug, Deserialize)]
pub struct RunAuditRequest {
    #[serde(default = "default_framework")]
    pub framework: String,
}

fn default_framework() -> String {
    "pci-dss-4.0".to_string()
}

/// Security posture summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPosture {
    pub score: f64,
    pub trend: String,
    pub policy_coverage: f64,
    pub encryption_coverage: f64,
    pub namespace_isolation: f64,
    pub last_audit: Option<String>,
}

fn error(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": msg })))
}

pub async fn list_frameworks(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, StatusCode> {
    track_request(&state, |_| {}).await;

    let frameworks = compliance::supported_frameworks();
    Ok(Json(json!({
        "frameworks": frameworks,
        "total": frameworks.len(),
    })))
}

/// Run the network checks now, filed under `framework`, and store the result.
pub async fn run_audit(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Json(req): Json<RunAuditRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    super::check_editor(&state, &claims)
        .map_err(|_| error(StatusCode::FORBIDDEN, "Editor or admin role required"))?;
    track_request(&state, |_| {}).await;

    // The framework is echoed into stored results and reports, so it must be one
    // we know rather than free text.
    if compliance::find_framework(&req.framework).is_none() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Unknown framework: see GET /compliance/frameworks for the supported ids",
        ));
    }

    let actor = actor_from_claims(&claims);
    let audit = compliance::run(&state, &req.framework, &actor).await;

    audit_log(
        &state,
        "compliance.audit",
        &req.framework,
        "",
        &format!(
            "Network checks completed: {} passed, {} failed, {} skipped (audit {})",
            audit.passed, audit.failed, audit.skipped, audit.audit_id
        ),
        &actor,
        "success",
    )
    .await;

    Ok(Json(to_json(&audit)))
}

fn summary(a: &AuditResult) -> Value {
    json!({
        "audit_id": a.audit_id,
        "framework": a.framework,
        "completed_at": a.completed_at,
        "requested_by": a.requested_by,
        "total_controls": a.total_controls,
        "passed": a.passed,
        "failed": a.failed,
        "skipped": a.skipped,
        "score": a.score,
    })
}

/// Stored audits, newest first (up to 100), without their findings.
pub async fn list_audits(State(state): State<Arc<AppState>>) -> Json<Value> {
    track_request(&state, |_| {}).await;
    let audits = compliance::list(&state).await;
    let total = audits.len();
    let items: Vec<Value> = audits.iter().take(100).map(summary).collect();
    Json(json!({ "audits": items, "total": total }))
}

pub async fn get_audit(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    track_request(&state, |_| {}).await;
    match compliance::get(&state, &id).await {
        Some(audit) => Ok(Json(to_json(&audit))),
        None => Err(error(StatusCode::NOT_FOUND, "Audit not found")),
    }
}

#[derive(Debug, Deserialize)]
pub struct ReportQuery {
    /// `html` (default), `csv` or `json`.
    pub format: Option<String>,
}

/// Keep only characters that are safe in a download file name.
fn file_stem(audit: &AuditResult) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.')
            .collect()
    };
    format!(
        "paqtra-checks-{}-{}",
        clean(&audit.framework),
        clean(&audit.audit_id.chars().take(8).collect::<String>())
    )
}

/// Render a stored audit as an HTML page (print to PDF from the browser), CSV
/// or JSON. HTML is served with a CSP that forbids scripts, and every value in
/// it is escaped.
pub async fn audit_report(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<String>,
    Query(q): Query<ReportQuery>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    track_request(&state, |_| {}).await;
    let format = q.format.as_deref().unwrap_or("html");
    if !matches!(format, "html" | "csv" | "json") {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "format must be html, csv or json",
        ));
    }
    let audit = compliance::get(&state, &id)
        .await
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Audit not found"))?;
    let actor = actor_from_claims(&claims);
    let stem = file_stem(&audit);

    let response = match format {
        "html" => {
            let framework = compliance::find_framework(&audit.framework);
            let body = compliance::render_html(
                &audit,
                framework.as_ref(),
                &chrono::Utc::now().to_rfc3339(),
                &actor,
                env!("CARGO_PKG_VERSION"),
            );
            (
                [
                    (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
                    (
                        header::CONTENT_SECURITY_POLICY,
                        compliance::REPORT_CSP.to_string(),
                    ),
                    (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
                    (header::CACHE_CONTROL, "no-store".to_string()),
                ],
                body,
            )
                .into_response()
        }
        "csv" => (
            [
                (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{stem}.csv\""),
                ),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
                (header::CACHE_CONTROL, "no-store".to_string()),
            ],
            compliance::render_csv(&audit),
        )
            .into_response(),
        _ => (
            [
                (header::CONTENT_TYPE, "application/json".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{stem}.json\""),
                ),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
                (header::CACHE_CONTROL, "no-store".to_string()),
            ],
            serde_json::to_string_pretty(&audit).unwrap_or_default(),
        )
            .into_response(),
    };

    audit_log(
        &state,
        "compliance.report",
        &audit.audit_id,
        "",
        &format!("Exported {format} report for {}", audit.framework),
        &actor,
        "success",
    )
    .await;
    Ok(response)
}

pub async fn security_posture(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<Value>, StatusCode> {
    if super::check_admin(&state, &claims).is_err() {
        return Err(StatusCode::FORBIDDEN);
    }
    track_request(&state, |_| {}).await;

    // Query real cluster state for posture calculation
    let ns_data = state.k8s.list_policies().await.unwrap_or_default();
    let policies_count = ns_data.len();

    // Get namespace count
    let ns_json = state
        .k8s
        .kubectl_json(&["get", "namespaces", "-o", "json"])
        .await;
    let namespaces_total = ns_json
        .get("items")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    // Find namespaces with policies
    let ns_with_policies: std::collections::HashSet<String> =
        ns_data.iter().map(|p| p.namespace.clone()).collect();
    let all_ns: Vec<String> = ns_json
        .get("items")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|i| {
                    i.get("metadata")
                        .and_then(|m| m.get("name"))
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .collect()
        })
        .unwrap_or_default();
    let ns_without: Vec<&str> = all_ns
        .iter()
        .filter(|ns| !ns_with_policies.contains(ns.as_str()) && !ns.starts_with("kube-"))
        .map(|s| s.as_str())
        .collect();

    let policy_coverage = if namespaces_total > 0 {
        ((namespaces_total - ns_without.len()) as f64 / namespaces_total as f64 * 100.0 * 10.0)
            .round()
            / 10.0
    } else {
        0.0
    };

    let score = policy_coverage; // Score reflects actual policy coverage percentage

    let posture = SecurityPosture {
        score,
        trend: "current".to_string(),
        policy_coverage,
        encryption_coverage: 0.0,
        namespace_isolation: policy_coverage,
        // The completion time of the newest stored audit, or none if none has run.
        last_audit: compliance::list(&state)
            .await
            .first()
            .and_then(|a| a.completed_at.clone()),
    };

    let mut recommendations = Vec::new();
    if !ns_without.is_empty() {
        recommendations.push(format!(
            "Apply network policies to namespaces: {}",
            ns_without.join(", ")
        ));
    }
    if policies_count == 0 {
        recommendations
            .push("No CiliumNetworkPolicies found -- create least-privilege policies".to_string());
    }

    Ok(Json(json!({
        "posture": to_json(&posture),
        "breakdown": {
            "namespaces_total": namespaces_total,
            "namespaces_with_policies": ns_with_policies.len(),
            "namespaces_without_policies": ns_without,
            "total_policies": policies_count,
        },
        "recommendations": recommendations,
    })))
}
