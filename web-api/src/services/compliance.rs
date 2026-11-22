// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Network security checks and their reports.
//
// The audit runs a small set of automated network-layer checks against the
// cluster (default-deny policy coverage, Hubble reachability, transparent
// encryption, dropped flows). It is NOT a compliance assessment: the checks are
// Paqtra's own and are not mapped to individual controls of any framework.
// Reports say so. The framework a run is filed under is context only.
//
// Results are stored durably for a year so a report can be reproduced later.
// A check whose input could not be read is `skipped` with the reason, never a
// pass: an empty answer from a failed lookup must not look like a clean bill.

use crate::AppState;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const AUDITS_PREFIX: &str = "cv:audits:";
const AUDIT_RETENTION_SECS: u64 = 365 * 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceFramework {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub control_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditFinding {
    /// Paqtra's check identifier (for example `PCI-NET-1`), not a control ID of
    /// the framework it is filed under.
    pub control_id: String,
    pub title: String,
    /// `passed`, `failed` or `skipped`.
    pub status: String,
    pub severity: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditResult {
    pub audit_id: String,
    pub framework: String,
    pub status: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub total_controls: u32,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    /// Share of the checks that could be evaluated which passed, in percent.
    pub score: f64,
    pub findings: Vec<AuditFinding>,
    #[serde(default)]
    pub requested_by: String,
    #[serde(default)]
    pub cluster: Option<String>,
    /// How many flows the dropped-flow check looked at.
    #[serde(default)]
    pub flows_sampled: u32,
}

pub fn supported_frameworks() -> Vec<ComplianceFramework> {
    let f = |id: &str, name: &str, version: &str, description: &str, control_count: u32| {
        ComplianceFramework {
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            description: description.to_string(),
            control_count,
        }
    };
    vec![
        f(
            "pci-dss-4.0",
            "PCI-DSS",
            "4.0",
            "Payment Card Industry Data Security Standard",
            64,
        ),
        f(
            "soc2-type2",
            "SOC2",
            "Type II",
            "Service Organization Control 2",
            48,
        ),
        f(
            "hipaa",
            "HIPAA",
            "2013",
            "Health Insurance Portability and Accountability Act",
            42,
        ),
        f(
            "gdpr",
            "GDPR",
            "2018",
            "General Data Protection Regulation",
            35,
        ),
        f(
            "iso27001-2022",
            "ISO27001",
            "2022",
            "Information Security Management System",
            93,
        ),
        f(
            "nist-csf-2.0",
            "NIST",
            "CSF 2.0",
            "NIST Cybersecurity Framework",
            108,
        ),
    ]
}

pub fn find_framework(id: &str) -> Option<ComplianceFramework> {
    supported_frameworks().into_iter().find(|f| f.id == id)
}

/// Short prefix for check IDs.
fn framework_prefix(framework: &str) -> &str {
    match framework {
        "pci-dss-4.0" => "PCI",
        "soc2-type2" => "SOC2",
        "hipaa" => "HIPAA",
        "gdpr" => "GDPR",
        "iso27001-2022" => "ISO",
        "nist-csf-2.0" => "NIST",
        _ => "GEN",
    }
}

/// Everything the checks need, gathered up front so evaluation is pure.
#[derive(Debug, Clone, Default)]
pub struct AuditInputs {
    /// Every namespace in the cluster. Empty means the lookup failed.
    pub all_namespaces: Vec<String>,
    pub policy_namespaces: HashSet<String>,
    pub hubble_healthy: bool,
    pub encryption_enabled: bool,
    pub flows_total: usize,
    pub flows_dropped: usize,
}

fn finding(
    id: String,
    title: &str,
    status: &str,
    severity: &str,
    description: String,
) -> AuditFinding {
    AuditFinding {
        control_id: id,
        title: title.to_string(),
        status: status.to_string(),
        severity: severity.to_string(),
        description,
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

pub fn evaluate(
    framework: &str,
    inputs: &AuditInputs,
    started: DateTime<Utc>,
    completed: DateTime<Utc>,
    requested_by: &str,
    cluster: Option<String>,
) -> AuditResult {
    let prefix = framework_prefix(framework);
    let mut findings = Vec::new();

    // 1. Default-deny coverage: every non-system namespace has a policy.
    let uncovered: Vec<&str> = inputs
        .all_namespaces
        .iter()
        .filter(|ns| !inputs.policy_namespaces.contains(ns.as_str()) && !ns.starts_with("kube-"))
        .map(|s| s.as_str())
        .collect();
    let id = format!("{prefix}-NET-1");
    findings.push(if inputs.all_namespaces.is_empty() {
        finding(id, "Default-deny network policies", "skipped", "high",
            "Namespaces could not be listed (kubectl unavailable or not permitted), so policy coverage was not evaluated.".into())
    } else if uncovered.is_empty() {
        finding(id, "Default-deny network policies", "passed", "high",
            "All non-system namespaces have CiliumNetworkPolicy coverage.".into())
    } else {
        finding(id, "Default-deny network policies", "failed", "high",
            format!("The following namespaces lack CiliumNetworkPolicy coverage: {}", uncovered.join(", ")))
    });

    // 2. Flow monitoring reachable.
    let id = format!("{prefix}-MON-1");
    findings.push(if inputs.hubble_healthy {
        finding(
            id,
            "Network flow monitoring",
            "passed",
            "high",
            "Hubble relay is reachable and actively monitoring network flows.".into(),
        )
    } else {
        finding(
            id,
            "Network flow monitoring",
            "failed",
            "critical",
            "Hubble relay is unreachable. Network flow monitoring is not operational.".into(),
        )
    });

    // 3. Transparent encryption.
    let id = format!("{prefix}-ENC-1");
    findings.push(if inputs.encryption_enabled {
        finding(id, "Encryption in transit", "passed", "critical",
            "Transparent encryption (WireGuard or IPsec) is enabled in the Cilium configuration.".into())
    } else {
        finding(id, "Encryption in transit", "failed", "critical",
            "No transparent encryption (WireGuard or IPsec) is enabled in the cilium-config ConfigMap. Inter-pod traffic may be unencrypted.".into())
    });

    // 4. Dropped flows in the sample.
    let id = format!("{prefix}-DRP-1");
    findings.push(if inputs.flows_total == 0 {
        finding(id, "Dropped network flows", "skipped", "medium",
            "No flows were observed, so the dropped-flow rate was not evaluated.".into())
    } else if inputs.flows_dropped == 0 {
        finding(id, "Dropped network flows", "passed", "medium",
            format!("No dropped flows detected in the last {} observed flows.", inputs.flows_total))
    } else {
        let pct = round1(inputs.flows_dropped as f64 / inputs.flows_total as f64 * 100.0);
        finding(id, "Dropped network flows", "failed", "medium",
            format!("{} of {} observed flows were dropped ({:.1}%). Investigate policy denials or misconfigured endpoints.",
                inputs.flows_dropped, inputs.flows_total, pct))
    });

    let count = |s: &str| findings.iter().filter(|f| f.status == s).count() as u32;
    let (passed, failed, skipped) = (count("passed"), count("failed"), count("skipped"));
    let evaluated = passed + failed;
    let score = if evaluated > 0 {
        round1(passed as f64 / evaluated as f64 * 100.0)
    } else {
        0.0
    };

    AuditResult {
        audit_id: uuid::Uuid::new_v4().to_string(),
        framework: framework.to_string(),
        status: "completed".to_string(),
        started_at: started.to_rfc3339(),
        completed_at: Some(completed.to_rfc3339()),
        total_controls: findings.len() as u32,
        passed,
        failed,
        skipped,
        score,
        findings,
        requested_by: requested_by.to_string(),
        cluster,
        flows_sampled: inputs.flows_total as u32,
    }
}

/// Read the cluster. A failed lookup yields empty data, which `evaluate`
/// reports as skipped rather than passed.
pub async fn collect_inputs(state: &AppState) -> AuditInputs {
    let policies = state.k8s.list_policies().await.unwrap_or_default();
    let ns_json = state
        .k8s
        .kubectl_json(&["get", "namespaces", "-o", "json"])
        .await;
    let all_namespaces: Vec<String> = ns_json
        .get("items")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|i| i.get("metadata")?.get("name")?.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let cfg = state
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
    let on = |key: &str| {
        cfg.get("data")
            .and_then(|d| d.get(key))
            .and_then(|v| v.as_str())
            == Some("true")
    };
    let encryption_enabled = on("enable-wireguard") || on("encrypt-node") || on("enable-ipsec");

    let flows = state.hubble.get_flows(500, None).await.unwrap_or_default();
    AuditInputs {
        all_namespaces,
        policy_namespaces: policies.iter().map(|p| p.namespace.clone()).collect(),
        hubble_healthy: state.hubble.is_healthy().await,
        encryption_enabled,
        flows_total: flows.len(),
        flows_dropped: flows.iter().filter(|f| f.verdict == "DROPPED").count(),
    }
}

pub async fn run(state: &AppState, framework: &str, requested_by: &str) -> AuditResult {
    let started = Utc::now();
    let inputs = collect_inputs(state).await;
    let result = evaluate(
        framework,
        &inputs,
        started,
        Utc::now(),
        requested_by,
        state.config.k8s_context.clone(),
    );
    if let Err(e) = save(state, &result).await {
        // The caller still gets the result; it just cannot be fetched again later.
        tracing::warn!("Failed to store audit {}: {}", result.audit_id, e);
    }
    result
}

pub async fn save(state: &AppState, audit: &AuditResult) -> anyhow::Result<()> {
    state
        .cache
        .set_durable(
            &format!("{AUDITS_PREFIX}{}", audit.audit_id),
            audit,
            AUDIT_RETENTION_SECS,
        )
        .await
}

pub async fn get(state: &AppState, id: &str) -> Option<AuditResult> {
    state
        .cache
        .get(&format!("{AUDITS_PREFIX}{id}"))
        .await
        .ok()
        .flatten()
}

/// Newest first.
pub async fn list(state: &AppState) -> Vec<AuditResult> {
    let mut audits: Vec<AuditResult> = state
        .cache
        .list_values(AUDITS_PREFIX)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();
    audits.sort_by(|a, b| b.completed_at.cmp(&a.completed_at));
    audits
}

// ── Reports ─────────────────────────────────────────────────

/// Escape text for an HTML element or attribute.
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Content-Security-Policy for the HTML report: no scripts, no network, no
/// frames. The report is opened from the app's own origin, so even a missed
/// escape must not be able to run script and read the session token.
pub const REPORT_CSP: &str =
    "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

const REPORT_CSS: &str = "body{font:14px/1.5 -apple-system,Segoe UI,Helvetica,Arial,sans-serif;color:#111;margin:2rem auto;max-width:56rem;padding:0 1rem}\
h1{font-size:1.5rem;margin:0 0 .25rem}h2{font-size:1.1rem;margin:1.75rem 0 .5rem;border-bottom:1px solid #ccc;padding-bottom:.25rem}\
table{border-collapse:collapse;width:100%}th,td{border:1px solid #bbb;padding:.4rem .6rem;text-align:left;vertical-align:top}th{background:#f2f2f2}\
.meta td:first-child{width:11rem;font-weight:600;background:#f7f7f7}.status{font-weight:700;text-transform:uppercase;white-space:nowrap}\
.notice{border:2px solid #111;padding:.75rem 1rem;margin:1rem 0;background:#fafafa}footer{margin-top:2rem;color:#555;font-size:12px}\
@media print{body{margin:0;max-width:none}h2{break-after:avoid}tr{break-inside:avoid}}";

pub fn render_html(
    audit: &AuditResult,
    framework: Option<&ComplianceFramework>,
    generated_at: &str,
    generated_by: &str,
    version: &str,
) -> String {
    let e = html_escape;
    let fw_label = framework
        .map(|f| format!("{} {}", f.name, f.version))
        .unwrap_or_else(|| audit.framework.clone());
    let mut h = String::new();
    h.push_str("<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">");
    h.push_str(&format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"{}\">",
        e(REPORT_CSP)
    ));
    h.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">");
    h.push_str(&format!(
        "<title>Network security checks - {}</title><style>{}</style></head><body>",
        e(&fw_label),
        REPORT_CSS
    ));
    h.push_str("<h1>Network security checks report</h1>");
    h.push_str(&format!(
        "<p>Filed under: <strong>{}</strong></p>",
        e(&fw_label)
    ));

    h.push_str("<div class=\"notice\"><strong>This is not a compliance assessment.</strong> ");
    h.push_str("It reports the results of automated network-layer checks and does not attest compliance with any framework. See Scope and limitations below.</div>");

    h.push_str("<h2>Run details</h2><table class=\"meta\">");
    let mut row =
        |k: &str, v: &str| h.push_str(&format!("<tr><td>{}</td><td>{}</td></tr>", e(k), e(v)));
    row("Audit ID", &audit.audit_id);
    row("Started", &audit.started_at);
    row(
        "Completed",
        audit.completed_at.as_deref().unwrap_or("not completed"),
    );
    row(
        "Requested by",
        if audit.requested_by.is_empty() {
            "unknown"
        } else {
            &audit.requested_by
        },
    );
    row(
        "Cluster context",
        audit.cluster.as_deref().unwrap_or("default"),
    );
    row("Flows examined", &audit.flows_sampled.to_string());
    h.push_str("</table>");

    h.push_str("<h2>Summary</h2>");
    h.push_str(&format!(
        "<p><strong>{} of {}</strong> checks passed, <strong>{}</strong> failed, <strong>{}</strong> not evaluated.{}</p>",
        audit.passed,
        audit.total_controls,
        audit.failed,
        audit.skipped,
        if audit.passed + audit.failed > 0 {
            format!(" Of the checks that could be evaluated, {}% passed.", audit.score)
        } else {
            String::new()
        }
    ));

    h.push_str("<h2>Checks</h2><table><thead><tr><th>Check</th><th>Title</th><th>Severity</th><th>Result</th><th>Detail</th></tr></thead><tbody>");
    for f in &audit.findings {
        h.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"status\">{}</td><td>{}</td></tr>",
            e(&f.control_id),
            e(&f.title),
            e(&f.severity),
            e(&f.status),
            e(&f.description)
        ));
    }
    h.push_str("</tbody></table>");

    h.push_str("<h2>Scope and limitations</h2><ul>");
    h.push_str(&format!(
        "<li>These are {} automated network-layer checks written for Paqtra. The check IDs are Paqtra identifiers, not control IDs of {}.</li>",
        audit.total_controls, e(&fw_label)
    ));
    if let Some(f) = framework {
        h.push_str(&format!(
            "<li>{} {} defines {} controls. These checks are not mapped to them, so passing them does not demonstrate compliance.</li>",
            e(&f.name), e(&f.version), f.control_count
        ));
    }
    h.push_str(&format!(
        "<li>The checks read the cluster at one moment. The dropped-flow check looks at the most recent {} flows Hubble returned, not at a period of time.</li>",
        audit.flows_sampled
    ));
    h.push_str("<li>A check marked skipped could not be evaluated, and the reason is given in its detail. It is not a pass.</li>");
    h.push_str("<li>Controls outside network behaviour (access management, data protection, change management, physical security and others) are not measured.</li>");
    h.push_str("</ul>");

    h.push_str(&format!(
        "<footer>Generated by Paqtra {} on {} for {}.</footer></body></html>",
        e(version),
        e(generated_at),
        e(generated_by)
    ));
    h
}

/// One CSV cell: quoted, with a leading quote character added to anything a
/// spreadsheet would treat as a formula.
fn csv_cell(s: &str) -> String {
    let guarded = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    format!("\"{}\"", guarded.replace('"', "\"\""))
}

pub fn render_csv(audit: &AuditResult) -> String {
    let mut out =
        String::from("audit_id,framework,check_id,title,severity,result,detail,completed_at\r\n");
    for f in &audit.findings {
        let cells = [
            &audit.audit_id,
            &audit.framework,
            &f.control_id,
            &f.title,
            &f.severity,
            &f.status,
            &f.description,
            audit.completed_at.as_deref().unwrap_or(""),
        ];
        out.push_str(
            &cells
                .iter()
                .map(|c| csv_cell(c))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn healthy() -> AuditInputs {
        AuditInputs {
            all_namespaces: ns(&["team-a", "team-b", "kube-system"]),
            policy_namespaces: ["team-a", "team-b"].iter().map(|s| s.to_string()).collect(),
            hubble_healthy: true,
            encryption_enabled: true,
            flows_total: 200,
            flows_dropped: 0,
        }
    }

    fn run(inputs: &AuditInputs) -> AuditResult {
        let t = Utc::now();
        evaluate("pci-dss-4.0", inputs, t, t, "alice", Some("ctx".into()))
    }

    fn status<'a>(a: &'a AuditResult, suffix: &str) -> &'a str {
        &a.findings
            .iter()
            .find(|f| f.control_id.ends_with(suffix))
            .unwrap()
            .status
    }

    #[test]
    fn all_checks_pass_on_a_healthy_cluster() {
        let a = run(&healthy());
        assert_eq!(
            (a.total_controls, a.passed, a.failed, a.skipped),
            (4, 4, 0, 0)
        );
        assert_eq!(a.score, 100.0);
        assert_eq!(a.requested_by, "alice");
        assert_eq!(a.flows_sampled, 200);
        assert!(a.findings.iter().all(|f| f.control_id.starts_with("PCI-")));
    }

    #[test]
    fn uncovered_namespaces_fail_and_are_named_but_system_ones_are_ignored() {
        let mut i = healthy();
        i.policy_namespaces.remove("team-b");
        let a = run(&i);
        assert_eq!(status(&a, "NET-1"), "failed");
        let d = &a.findings[0].description;
        assert!(
            d.contains("team-b") && !d.contains("kube-system") && !d.contains("team-a"),
            "{d}"
        );
    }

    #[test]
    fn each_failing_input_fails_only_its_own_check() {
        let mut i = healthy();
        i.hubble_healthy = false;
        let a = run(&i);
        assert_eq!(
            (status(&a, "MON-1"), a.findings[1].severity.as_str()),
            ("failed", "critical")
        );
        assert_eq!((a.passed, a.failed), (3, 1));

        let mut i = healthy();
        i.encryption_enabled = false;
        assert_eq!(status(&run(&i), "ENC-1"), "failed");

        let mut i = healthy();
        i.flows_dropped = 25;
        let a = run(&i);
        assert_eq!(status(&a, "DRP-1"), "failed");
        assert!(
            a.findings[3].description.contains("25 of 200")
                && a.findings[3].description.contains("12.5%")
        );
    }

    #[test]
    fn a_failed_lookup_is_skipped_never_a_pass() {
        // kubectl could not list namespaces: an empty list must not read as "all covered".
        let mut i = healthy();
        i.all_namespaces.clear();
        let a = run(&i);
        assert_eq!(status(&a, "NET-1"), "skipped");
        assert!(a.findings[0].description.contains("not evaluated"));
        // Hubble returned nothing: zero flows must not read as "no drops".
        let mut i = healthy();
        i.flows_total = 0;
        let a = run(&i);
        assert_eq!(status(&a, "DRP-1"), "skipped");
        assert_eq!(a.skipped, 1);
    }

    #[test]
    fn score_counts_only_checks_that_were_evaluated() {
        let mut i = healthy();
        i.all_namespaces.clear();
        i.encryption_enabled = false;
        let a = run(&i);
        // NET-1 skipped; MON pass, ENC fail, DRP pass -> 2 of 3 evaluated.
        assert_eq!((a.passed, a.failed, a.skipped), (2, 1, 1));
        assert_eq!(a.score, 66.7);
        // Nothing readable at all: Hubble down and encryption off still count as
        // evaluated failures; the two lookups that returned nothing are skipped.
        let a = run(&AuditInputs::default());
        assert_eq!((a.passed, a.failed, a.skipped), (0, 2, 2));
        assert_eq!(a.score, 0.0);
    }

    #[test]
    fn unknown_framework_falls_back_to_a_generic_prefix_and_lookup_fails() {
        let t = Utc::now();
        let a = evaluate("made-up", &healthy(), t, t, "x", None);
        assert!(a.findings[0].control_id.starts_with("GEN-"));
        assert!(find_framework("made-up").is_none());
        assert_eq!(find_framework("hipaa").unwrap().control_count, 42);
        assert_eq!(supported_frameworks().len(), 6);
    }

    #[test]
    fn html_escapes_every_dynamic_value() {
        let mut a = run(&healthy());
        let evil = "<script>alert(1)</script>\"'&";
        a.requested_by = evil.into();
        a.cluster = Some(evil.into());
        a.framework = evil.into();
        a.findings[0].description = evil.into();
        a.findings[0].title = evil.into();
        a.findings[0].control_id = evil.into();
        let html = render_html(&a, None, evil, evil, evil);
        assert!(
            !html.contains("<script>"),
            "raw script tag leaked into the report"
        );
        assert!(!html.contains("alert(1)</script>"));
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;&quot;&#39;&amp;"));
    }

    #[test]
    fn html_carries_a_strict_csp_and_no_script_or_external_reference() {
        let html = render_html(
            &run(&healthy()),
            find_framework("pci-dss-4.0").as_ref(),
            "now",
            "alice",
            "1.0",
        );
        assert!(html.contains("http-equiv=\"Content-Security-Policy\""));
        assert!(
            html.contains("default-src &#39;none&#39;"),
            "CSP must forbid everything by default"
        );
        assert!(!html.contains("<script"));
        assert!(
            !html.contains("http://") && !html.contains("https://"),
            "no external references"
        );
        assert!(!html.contains("<img") && !html.contains("<link"));
    }

    #[test]
    fn html_states_what_it_is_not_and_lists_every_check() {
        let a = run(&healthy());
        let html = render_html(
            &a,
            find_framework("pci-dss-4.0").as_ref(),
            "now",
            "alice",
            "1.0",
        );
        assert!(html.contains("This is not a compliance assessment"));
        assert!(html.contains("defines 64 controls") && html.contains("not mapped to them"));
        assert!(html.contains("most recent 200 flows"));
        for f in &a.findings {
            assert!(html.contains(&f.control_id), "{}", f.control_id);
        }
        assert!(html.contains("<strong>4 of 4</strong> checks passed"));
        assert!(html.contains("Generated by Paqtra 1.0"));
    }

    #[test]
    fn html_reports_skipped_checks_as_not_evaluated() {
        let mut i = healthy();
        i.flows_total = 0;
        let html = render_html(&run(&i), None, "now", "a", "1");
        assert!(html.contains("<strong>1</strong> not evaluated"));
        assert!(html.contains(">skipped<"));
    }

    #[test]
    fn csv_has_a_header_one_row_per_check_and_quotes_cells() {
        let mut a = run(&healthy());
        a.findings[0].description = "has \"quotes\", commas\nand a newline".into();
        let csv = render_csv(&a);
        let mut lines = csv.split("\r\n");
        assert_eq!(
            lines.next().unwrap(),
            "audit_id,framework,check_id,title,severity,result,detail,completed_at"
        );
        assert!(csv.contains("\"has \"\"quotes\"\", commas\nand a newline\""));
        assert_eq!(csv.matches("\r\n").count(), 5, "header + 4 checks");
    }

    #[test]
    fn csv_neutralises_spreadsheet_formulas() {
        for evil in ["=1+1", "+cmd", "-2+3", "@SUM(A1)", "\t=x", "\r=x"] {
            let mut a = run(&healthy());
            a.findings[0].title = evil.into();
            let csv = render_csv(&a);
            assert!(
                csv.contains(&format!("\"'{}\"", evil.replace('"', "\"\""))),
                "{evil:?} not guarded"
            );
        }
        assert_eq!(csv_cell("safe"), "\"safe\"");
        assert_eq!(
            csv_cell("a=b"),
            "\"a=b\"",
            "only a leading formula character is guarded"
        );
    }
}
