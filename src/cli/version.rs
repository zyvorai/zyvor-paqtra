// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! `paqtra version`: client, installed release, and running server versions,
//! with a warning when they disagree (like `cilium version`).

use anyhow::Result;
use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};

use super::chart::{embedded_version, CLI_VERSION};
use super::global::Global;
use super::helm;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub name: String,
    pub namespace: String,
    pub chart: String,
    pub app_version: String,
    pub status: String,
}

#[derive(Deserialize)]
struct HelmListEntry {
    name: String,
    namespace: String,
    status: String,
    chart: String,
    app_version: String,
}

/// `helm list -o json` -> the release named `name`.
pub fn parse_release(json: &str, name: &str) -> Option<ReleaseInfo> {
    let entries: Vec<HelmListEntry> = serde_json::from_str(json).ok()?;
    entries
        .into_iter()
        .find(|e| e.name == name)
        .map(|e| ReleaseInfo {
            name: e.name,
            namespace: e.namespace,
            chart: e.chart,
            app_version: e.app_version,
            status: e.status,
        })
}

/// `paqtra-2.1.0` -> `2.1.0`.
pub fn chart_version_of(chart: &str) -> &str {
    chart.rsplit_once('-').map(|(_, v)| v).unwrap_or(chart)
}

/// Human-readable notes on version disagreement; empty when consistent.
pub fn skew(client: &str, release: Option<&ReleaseInfo>, server: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(r) = release {
        let chart = chart_version_of(&r.chart);
        if chart != client {
            out.push(format!(
                "client is v{client} but the release runs chart v{chart}: `paqtra upgrade` aligns them"
            ));
        }
        if let Some(s) = server {
            if s != r.app_version {
                out.push(format!(
                    "the API reports v{s} but the release says app v{}: a rollout may be in progress",
                    r.app_version
                ));
            }
        }
    }
    out
}

fn server_version(health_body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(health_body).ok()?;
    v.get("version")?.as_str().map(str::to_string)
}

#[derive(Serialize)]
struct VersionReport {
    client: String,
    embedded_chart: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    release: Option<ReleaseInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server: Option<String>,
    warnings: Vec<String>,
}

pub async fn cmd_version(g: &Global, client_only: bool, output: &str) -> Result<()> {
    let mut report = VersionReport {
        client: CLI_VERSION.into(),
        embedded_chart: embedded_version(),
        release: None,
        server: None,
        warnings: Vec::new(),
    };

    if !client_only {
        // Best effort: `version` must work with no cluster and no helm.
        if let Some(h) = helm::find(g).await {
            let args: Vec<String> = vec![
                "list".into(),
                "--namespace".into(),
                g.namespace.clone(),
                "--filter".into(),
                format!("^{}$", g.release),
                "--output".into(),
                "json".into(),
            ];
            if let Ok(out) = h.run_ok(&args, "helm list").await {
                report.release = parse_release(&out, &g.release);
            }
        }
        if let Ok(client) = super::kube::client(g).await {
            if let Some(Ok(body)) = super::status::api_health_raw(&client, g).await {
                report.server = server_version(&body);
            }
        }
        report.warnings = skew(
            CLI_VERSION,
            report.release.as_ref(),
            report.server.as_deref(),
        );
    }

    if output == "json" {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    println!(
        "{:9} paqtra v{} (chart v{} built in)",
        "Client:".dimmed(),
        report.client,
        report.embedded_chart
    );
    if !client_only {
        match &report.release {
            Some(r) => println!(
                "{:9} {}/{}: chart {}, app v{}, {}",
                "Release:".dimmed(),
                r.namespace,
                r.name,
                r.chart,
                r.app_version,
                r.status
            ),
            None => println!(
                "{:9} {}",
                "Release:".dimmed(),
                format!("none found ({}/{})", g.namespace, g.release).dimmed()
            ),
        }
        match &report.server {
            Some(s) => println!("{:9} Paqtra API v{s}", "Server:".dimmed()),
            None => println!("{:9} {}", "Server:".dimmed(), "API not reachable".dimmed()),
        }
        for w in &report.warnings {
            println!("{} {w}", "⚠".yellow());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"[{"name":"other","namespace":"x","revision":"1","updated":"t","status":"deployed","chart":"other-1.0.0","app_version":"1.0.0"},
        {"name":"paqtra","namespace":"paqtra","revision":"22","updated":"t","status":"deployed","chart":"paqtra-2.1.0","app_version":"2.1.0"}]"#;

    #[test]
    fn finds_our_release_among_others() {
        let r = parse_release(LIST, "paqtra").unwrap();
        assert_eq!(
            (r.chart.as_str(), r.app_version.as_str(), r.status.as_str()),
            ("paqtra-2.1.0", "2.1.0", "deployed")
        );
        assert!(parse_release(LIST, "missing").is_none());
        assert!(parse_release("not json", "paqtra").is_none());
        assert!(parse_release("[]", "paqtra").is_none());
    }

    #[test]
    fn chart_version_is_after_the_last_dash() {
        assert_eq!(chart_version_of("paqtra-2.1.0"), "2.1.0");
        assert_eq!(
            chart_version_of("my-paqtra-2.2.0-rc.1"),
            "rc.1",
            "documented limitation of the helm chart column"
        );
        assert_eq!(chart_version_of("nodash"), "nodash");
    }

    fn rel(chart: &str, app: &str) -> ReleaseInfo {
        ReleaseInfo {
            name: "p".into(),
            namespace: "n".into(),
            chart: chart.into(),
            app_version: app.into(),
            status: "deployed".into(),
        }
    }

    #[test]
    fn skew_is_reported_only_when_versions_disagree() {
        assert!(skew("2.1.0", Some(&rel("paqtra-2.1.0", "2.1.0")), Some("2.1.0")).is_empty());
        assert!(
            skew("2.1.0", None, None).is_empty(),
            "no release, nothing to compare"
        );
        let w = skew("2.2.0", Some(&rel("paqtra-2.1.0", "2.1.0")), Some("2.1.0"));
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("paqtra upgrade"));
        let w = skew("2.1.0", Some(&rel("paqtra-2.1.0", "2.1.0")), Some("2.0.0"));
        assert!(w[0].contains("rollout"));
    }

    #[test]
    fn reads_the_server_version_from_health() {
        assert_eq!(
            server_version(r#"{"status":"healthy","version":"2.1.0"}"#),
            Some("2.1.0".into())
        );
        assert_eq!(server_version("{}"), None);
        assert_eq!(server_version("x"), None);
    }
}
