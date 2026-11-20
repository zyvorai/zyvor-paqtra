// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! `paqtra status [--wait]`: are Paqtra, Cilium and Hubble Relay healthy?
//!
//! Reads the Kubernetes API directly (no `kubectl`), waits for real when asked,
//! and reports through the exit code: 0 healthy, 1 not (so scripts and CI can
//! gate on it, like `cilium status --wait`).

use anyhow::Result;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
use k8s_openapi::api::core::v1::Service;
use kube::api::ListParams;
use kube::{Api, Client};
use owo_colors::OwoColorize;
use serde::Serialize;
use std::time::{Duration, Instant};

use super::banner::print_banner;
use super::global::Global;
use crate::ebpf::attachments::{collect_inventory, drift_findings};

const CILIUM_NAMESPACE: &str = "kube-system";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Ready,
    /// Some replicas up, some not.
    Degraded,
    Error,
    /// Not present (e.g. `agent.enabled=false`).
    NotDeployed,
    Info,
    Warning,
}

impl State {
    fn label(self) -> &'static str {
        match self {
            State::Ready => "Ready",
            State::Degraded => "Degraded",
            State::Error => "Error",
            State::NotDeployed => "Not deployed",
            State::Info => "Info",
            State::Warning => "Warning",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Component {
    pub name: String,
    pub state: State,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct StatusReport {
    pub components: Vec<Component>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub drift: Vec<crate::ebpf::attachments::DriftFinding>,
    pub ok: bool,
}

#[derive(Debug, Clone)]
pub struct StatusOpts {
    pub wait: bool,
    pub wait_duration: Duration,
    pub output: String,
    /// Also report this machine's BPF inventory and drift.
    pub local: bool,
}

/// Ready/desired replica counts -> a state.
pub fn workload_state(ready: i32, desired: i32, unit: &str) -> (State, String) {
    let detail = format!("{ready}/{desired} {unit}");
    if desired <= 0 {
        return (State::NotDeployed, "scaled to 0".into());
    }
    if ready >= desired {
        (State::Ready, detail)
    } else if ready > 0 {
        (State::Degraded, detail)
    } else {
        (State::Error, detail)
    }
}

/// The verdict `paqtra status` turns into its exit code.
///  - the API must be Ready;
///  - the agent and UI must be Ready if they are deployed at all;
///  - Cilium and Hubble Relay must exist and not be down (Paqtra reads flows
///    through them); a degraded one is a warning, not a failure.
pub fn overall_ok(components: &[Component]) -> bool {
    let state = |name: &str| components.iter().find(|c| c.name == name).map(|c| c.state);
    let ready_or_absent = |n: &str| {
        matches!(
            state(n),
            Some(State::Ready) | Some(State::NotDeployed) | None
        )
    };
    let usable = |n: &str| matches!(state(n), Some(State::Ready) | Some(State::Degraded));
    state("API") == Some(State::Ready)
        && ready_or_absent("Agent")
        && ready_or_absent("UI")
        && usable("Cilium")
        && usable("Hubble Relay")
}

/// `/health` body -> (state, detail). Anything but "healthy" is a warning: the
/// pods are up, but a subsystem (Hubble ingest, Kubernetes) is not.
pub fn api_health_state(body: &str) -> (State, String) {
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return (State::Warning, "unexpected /health response".into()),
    };
    let status = v
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("unknown");
    let version = v.get("version").and_then(|s| s.as_str()).unwrap_or("?");
    if status == "healthy" {
        return (State::Ready, format!("healthy, v{version}"));
    }
    let bad: Vec<String> = v
        .get("subsystems")
        .and_then(|s| s.as_object())
        .map(|m| {
            m.iter()
                .filter(|(_, val)| {
                    let s = val
                        .as_str()
                        .or_else(|| val.get("status").and_then(|x| x.as_str()));
                    s.is_some_and(|s| s != "ok")
                })
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default();
    let why = if bad.is_empty() {
        String::new()
    } else {
        format!(": {}", bad.join(", "))
    };
    (State::Warning, format!("{status}{why}, v{version}"))
}

fn instance_selector(g: &Global, component: &str) -> String {
    format!(
        "app.kubernetes.io/instance={},app.kubernetes.io/component={component}",
        g.release
    )
}

async fn deployment(client: &Client, ns: &str, selector: &str) -> Option<(i32, i32)> {
    let api: Api<Deployment> = Api::namespaced(client.clone(), ns);
    let d = api
        .list(&ListParams::default().labels(selector))
        .await
        .ok()?
        .items
        .into_iter()
        .next()?;
    let ready = d.status.and_then(|s| s.ready_replicas).unwrap_or(0);
    let desired = d.spec.and_then(|s| s.replicas).unwrap_or(1);
    Some((ready, desired))
}

async fn daemonset(client: &Client, ns: &str, selector: &str) -> Option<(i32, i32)> {
    let api: Api<DaemonSet> = Api::namespaced(client.clone(), ns);
    let d = api
        .list(&ListParams::default().labels(selector))
        .await
        .ok()?
        .items
        .into_iter()
        .next()?;
    let s = d.status?;
    Some((s.number_ready, s.desired_number_scheduled))
}

fn component(name: &str, counts: Option<(i32, i32)>, unit: &str) -> Component {
    match counts {
        None => Component {
            name: name.into(),
            state: State::NotDeployed,
            detail: "not deployed".into(),
        },
        Some((ready, desired)) => {
            let (state, detail) = workload_state(ready, desired, unit);
            Component {
                name: name.into(),
                state,
                detail,
            }
        }
    }
}

/// GET /health through the API server's Service proxy: no port-forward, works
/// from anywhere the kubeconfig does. `None`: no API Service exists;
/// `Some(Err)`: it exists but did not answer.
pub(super) async fn api_health_raw(client: &Client, g: &Global) -> Option<Result<String, String>> {
    let svcs: Api<Service> = Api::namespaced(client.clone(), &g.namespace);
    let svc = svcs
        .list(&ListParams::default().labels(&instance_selector(g, "api")))
        .await
        .ok()?
        .items
        .into_iter()
        .next()?;
    let name = svc.metadata.name?;
    let port = svc.spec?.ports?.first()?.port;
    let uri = format!(
        "/api/v1/namespaces/{}/services/{name}:{port}/proxy/health",
        g.namespace
    );
    let req = http::Request::get(uri).body(Vec::new()).ok()?;
    Some(client.request_text(req).await.map_err(|e| e.to_string()))
}

async fn api_health(client: &Client, g: &Global) -> Option<Component> {
    let (state, detail) = match api_health_raw(client, g).await? {
        Ok(body) => api_health_state(&body),
        Err(e) => (State::Warning, format!("/health not reachable: {e}")),
    };
    Some(Component {
        name: "API health".into(),
        state,
        detail,
    })
}

pub(super) async fn gather(client: &Client, g: &Global) -> Vec<Component> {
    let mut out = vec![
        component(
            "Agent",
            daemonset(client, &g.namespace, &instance_selector(g, "agent")).await,
            "nodes",
        ),
        component(
            "API",
            deployment(client, &g.namespace, &instance_selector(g, "api")).await,
            "pods",
        ),
        component(
            "UI",
            deployment(client, &g.namespace, &instance_selector(g, "ui")).await,
            "pods",
        ),
    ];
    if let Some(h) = api_health(client, g).await {
        out.push(h);
    }
    let mut cilium = component(
        "Cilium",
        daemonset(client, CILIUM_NAMESPACE, "k8s-app=cilium").await,
        "agents",
    );
    let mut relay = component(
        "Hubble Relay",
        deployment(client, CILIUM_NAMESPACE, "k8s-app=hubble-relay").await,
        "pods",
    );
    for c in [&mut cilium, &mut relay] {
        // Paqtra needs both: "not deployed" is an error here, not an option.
        if c.state == State::NotDeployed {
            c.state = State::Error;
            c.detail = "not found in kube-system".into();
        }
    }
    out.push(cilium);
    out.push(relay);
    out
}

fn local_bpf_rows() -> (Vec<Component>, Vec<crate::ebpf::attachments::DriftFinding>) {
    let inv = collect_inventory();
    let findings = drift_findings(&inv);
    let mut rows = vec![Component {
        name: "BPF inventory".into(),
        state: if inv.source == "unavailable" {
            State::Info
        } else {
            State::Ready
        },
        detail: format!(
            "{} via {} (cilium={} netra={} other={})",
            inv.total, inv.source, inv.cilium, inv.netra, inv.other
        ),
    }];
    for f in &findings {
        rows.push(Component {
            name: format!("Drift:{}", f.kind),
            state: if f.severity == "warning" {
                State::Warning
            } else {
                State::Info
            },
            detail: f.message.clone(),
        });
    }
    (rows, findings)
}

fn row(c: &Component) {
    let label = format!("{:14}", c.state.label());
    let colored = match c.state {
        State::Ready => label.green().to_string(),
        State::NotDeployed | State::Info => label.dimmed().to_string(),
        State::Warning | State::Degraded => label.yellow().to_string(),
        State::Error => label.red().to_string(),
    };
    println!("  {:16} {} {}", c.name, colored, c.detail.dimmed());
}

/// Returns whether Paqtra is healthy; the caller turns that into the exit code.
pub async fn cmd_status(g: &Global, o: StatusOpts) -> Result<bool> {
    let client = super::kube::client(g).await?;
    let json = o.output == "json";

    let started = Instant::now();
    if o.wait && !json {
        println!(
            "{} Waiting up to {}s for Paqtra components...",
            "→".cyan(),
            o.wait_duration.as_secs()
        );
    }
    let mut components = gather(&client, g).await;
    while o.wait && !overall_ok(&components) && started.elapsed() < o.wait_duration {
        tokio::time::sleep(Duration::from_secs(2)).await;
        components = gather(&client, g).await;
    }

    let mut drift = Vec::new();
    if o.local {
        let (rows, findings) = local_bpf_rows();
        components.extend(rows);
        drift = findings;
    }
    let ok = overall_ok(&components);
    let report = StatusReport {
        components,
        drift,
        ok,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(ok);
    }
    print_banner();
    for c in &report.components {
        row(c);
    }
    println!();
    if ok {
        println!("{} Paqtra is running", "✔".green());
    } else {
        println!("{} Paqtra has issues: see above", "✖".red());
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str, state: State) -> Component {
        Component {
            name: name.into(),
            state,
            detail: String::new(),
        }
    }
    fn healthy() -> Vec<Component> {
        vec![
            c("Agent", State::Ready),
            c("API", State::Ready),
            c("UI", State::Ready),
            c("Cilium", State::Ready),
            c("Hubble Relay", State::Ready),
        ]
    }
    fn with(mut v: Vec<Component>, name: &str, state: State) -> Vec<Component> {
        v.iter_mut().find(|x| x.name == name).unwrap().state = state;
        v
    }

    #[test]
    fn workload_states() {
        assert_eq!(workload_state(3, 3, "pods").0, State::Ready);
        assert_eq!(workload_state(1, 3, "pods").0, State::Degraded);
        assert_eq!(workload_state(0, 3, "pods").0, State::Error);
        assert_eq!(workload_state(0, 0, "pods").0, State::NotDeployed);
        assert_eq!(workload_state(3, 3, "nodes").1, "3/3 nodes");
    }

    #[test]
    fn a_healthy_install_is_ok() {
        assert!(overall_ok(&healthy()));
    }

    #[test]
    fn the_api_must_be_ready() {
        for bad in [State::Degraded, State::Error, State::NotDeployed] {
            assert!(!overall_ok(&with(healthy(), "API", bad)), "{bad:?}");
        }
    }

    #[test]
    fn optional_components_may_be_absent_but_not_broken() {
        assert!(overall_ok(&with(healthy(), "Agent", State::NotDeployed)));
        assert!(overall_ok(&with(healthy(), "UI", State::NotDeployed)));
        assert!(!overall_ok(&with(healthy(), "Agent", State::Degraded)));
        assert!(!overall_ok(&with(healthy(), "UI", State::Error)));
    }

    #[test]
    fn cilium_and_hubble_must_exist_but_may_be_degraded() {
        assert!(overall_ok(&with(healthy(), "Cilium", State::Degraded)));
        assert!(overall_ok(&with(
            healthy(),
            "Hubble Relay",
            State::Degraded
        )));
        assert!(!overall_ok(&with(healthy(), "Cilium", State::Error)));
        assert!(!overall_ok(&with(healthy(), "Hubble Relay", State::Error)));
        assert!(!overall_ok(&with(
            healthy(),
            "Hubble Relay",
            State::NotDeployed
        )));
    }

    #[test]
    fn local_and_warning_rows_never_change_the_verdict() {
        let mut v = healthy();
        v.push(c("BPF inventory", State::Info));
        v.push(c("Drift:x", State::Warning));
        v.push(c("API health", State::Warning));
        assert!(overall_ok(&v));
    }

    #[test]
    fn health_body_is_interpreted() {
        let ok = api_health_state(r#"{"status":"healthy","version":"2.1.0"}"#);
        assert_eq!(ok, (State::Ready, "healthy, v2.1.0".into()));
        let bad = api_health_state(
            r#"{"status":"degraded","version":"2.1.0","subsystems":{"cache":"ok","hubble_relay":"error","flow_ingest":{"status":"stalled"}}}"#,
        );
        assert_eq!(bad.0, State::Warning);
        assert!(
            bad.1.contains("hubble_relay")
                && bad.1.contains("flow_ingest")
                && !bad.1.contains("cache"),
            "{}",
            bad.1
        );
        assert_eq!(api_health_state("<html>").0, State::Warning);
    }

    #[test]
    fn a_missing_component_reads_as_not_deployed() {
        assert_eq!(component("UI", None, "pods").state, State::NotDeployed);
        assert_eq!(component("UI", Some((1, 1)), "pods").state, State::Ready);
    }
}
