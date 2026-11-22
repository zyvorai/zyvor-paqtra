// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Prerequisite checks, shared by `paqtra install` (blocks on failures) and
//! `paqtra doctor`. Each check is a pure function of what was observed, so the
//! judgement (is Cilium old enough? is Hubble usable?) is unit-tested without a
//! cluster; `run` only gathers the observations.

use anyhow::Result;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
use k8s_openapi::api::authorization::v1::{
    ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec,
};
use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::api::storage::v1::StorageClass;
use kube::api::{ListParams, PostParams};
use kube::{Api, Client};
use owo_colors::OwoColorize;
use serde::Serialize;

use super::global::Global;
use super::helm::parse_version;

/// Kubernetes minor versions: below MIN fails, below RECOMMENDED warns.
pub const MIN_K8S_MINOR: u32 = 25;
pub const RECOMMENDED_K8S_MINOR: u32 = 28;
/// Cilium versions (major, minor), as documented in the README.
pub const MIN_CILIUM: (u32, u32) = (1, 14);
pub const RECOMMENDED_CILIUM: (u32, u32) = (1, 19);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    /// Worth knowing, needs no action.
    Info,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: String,
    pub level: Level,
    pub detail: String,
    /// What to do about it; only for Warn/Fail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl Check {
    fn new(name: &str, level: Level, detail: impl Into<String>, hint: Option<&str>) -> Self {
        Self {
            name: name.into(),
            level,
            detail: detail.into(),
            hint: hint.map(str::to_string),
        }
    }
    pub fn ok(name: &str, detail: impl Into<String>) -> Self {
        Self::new(name, Level::Ok, detail, None)
    }
    pub fn info(name: &str, detail: impl Into<String>) -> Self {
        Self::new(name, Level::Info, detail, None)
    }
    pub fn warn(name: &str, detail: impl Into<String>, hint: &str) -> Self {
        Self::new(name, Level::Warn, detail, Some(hint))
    }
    pub fn fail(name: &str, detail: impl Into<String>, hint: &str) -> Self {
        Self::new(name, Level::Fail, detail, Some(hint))
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    pub fn push(&mut self, c: Check) {
        self.checks.push(c);
    }
    pub fn extend(&mut self, cs: impl IntoIterator<Item = Check>) {
        self.checks.extend(cs);
    }
    pub fn has_failures(&self) -> bool {
        self.checks.iter().any(|c| c.level == Level::Fail)
    }
    pub fn has_warnings(&self) -> bool {
        self.checks.iter().any(|c| c.level == Level::Warn)
    }

    /// `✔ / ⚠ / ✘` lines like cilium's preflight output.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for c in &self.checks {
            let mark = match c.level {
                Level::Ok => "✔".green().to_string(),
                Level::Info => "ℹ".blue().to_string(),
                Level::Warn => "⚠".yellow().to_string(),
                Level::Fail => "✘".red().to_string(),
            };
            out.push_str(&format!("  {mark} {:28} {}\n", c.name, c.detail));
            if let Some(h) = &c.hint {
                out.push_str(&format!("      {} {}\n", "→".dimmed(), h.dimmed()));
            }
        }
        out
    }
}

// ─── Pure judgements ───────────────────────────────────────────────────────

/// `"31+"` (EKS/GKE suffix) -> 31.
pub fn parse_minor(minor: &str) -> Option<u32> {
    minor
        .trim_end_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .ok()
}

pub fn eval_k8s_version(major: &str, minor: &str, git_version: &str) -> Check {
    let name = "Kubernetes version";
    match (major.trim().parse::<u32>().ok(), parse_minor(minor)) {
        (Some(1), Some(m)) if m < MIN_K8S_MINOR => Check::fail(
            name,
            format!("{git_version} is too old"),
            &format!("Paqtra needs Kubernetes 1.{MIN_K8S_MINOR} or newer"),
        ),
        (Some(1), Some(m)) if m < RECOMMENDED_K8S_MINOR => Check::warn(
            name,
            git_version.to_string(),
            &format!("1.{RECOMMENDED_K8S_MINOR}+ is recommended"),
        ),
        (Some(_), Some(_)) => Check::ok(name, git_version.to_string()),
        _ => Check::warn(
            name,
            format!("cannot parse {git_version:?}"),
            "continuing anyway",
        ),
    }
}

/// `quay.io/cilium/cilium:v1.19.0@sha256:...` -> (1, 19, 0).
pub fn cilium_version_from_image(image: &str) -> Option<(u32, u32, u32)> {
    let no_digest = image.split('@').next()?;
    let tag = no_digest.rsplit_once(':')?.1;
    parse_version(tag)
}

#[derive(Debug, Clone)]
pub struct CiliumInfo {
    pub image: String,
    pub desired: i32,
    pub ready: i32,
}

pub fn eval_cilium(info: Option<&CiliumInfo>) -> Check {
    let name = "Cilium";
    let Some(info) = info else {
        return Check::fail(
            name,
            "no Cilium agent DaemonSet found",
            "install Cilium first (`paqtra install --with-cilium` or https://docs.cilium.io), then retry",
        );
    };
    let version = cilium_version_from_image(&info.image);
    let shown = version
        .map(|(a, b, c)| format!("v{a}.{b}.{c}"))
        .unwrap_or_else(|| info.image.clone());
    let detail = format!("{shown}, {}/{} agents ready", info.ready, info.desired);
    match version {
        Some((a, b, _)) if (a, b) < MIN_CILIUM => Check::fail(
            name,
            format!("{detail}: too old"),
            &format!("Paqtra needs Cilium {}.{}+", MIN_CILIUM.0, MIN_CILIUM.1),
        ),
        _ if info.ready < info.desired => Check::warn(
            name,
            detail,
            "some Cilium agents are not ready; flows from those nodes will be missing",
        ),
        Some((a, b, _)) if (a, b) < RECOMMENDED_CILIUM => Check::warn(
            name,
            detail,
            &format!(
                "Cilium {}.{}+ is recommended",
                RECOMMENDED_CILIUM.0, RECOMMENDED_CILIUM.1
            ),
        ),
        _ => Check::ok(name, detail),
    }
}

/// `enable_hubble` is the `cilium-config` value (None = key or ConfigMap
/// absent); `relay` is (ready, desired) replicas of the hubble-relay Deployment.
pub fn eval_hubble(enable_hubble: Option<&str>, relay: Option<(i32, i32)>) -> Vec<Check> {
    let mut out = Vec::new();
    match enable_hubble.map(|v| v.trim().to_ascii_lowercase()) {
        Some(v) if v == "true" => out.push(Check::ok("Hubble", "enabled in cilium-config")),
        Some(_) => out.push(Check::fail(
            "Hubble",
            "disabled in cilium-config",
            "enable it: `paqtra hubble enable` (or Helm hubble.enabled=true)",
        )),
        None => out.push(Check::warn(
            "Hubble",
            "cilium-config has no enable-hubble key",
            "cannot confirm Hubble is on; the Relay check below decides",
        )),
    }
    out.push(match relay {
        Some((ready, desired)) if desired > 0 && ready >= desired => {
            Check::ok("Hubble Relay", format!("{ready}/{desired} ready"))
        }
        Some((ready, desired)) => Check::fail(
            "Hubble Relay",
            format!("{ready}/{desired} ready"),
            "Paqtra reads flows through Hubble Relay; wait for it or check its pods",
        ),
        None => Check::fail(
            "Hubble Relay",
            "no hubble-relay Deployment found",
            "enable it: `paqtra hubble enable --relay` (or Helm hubble.relay.enabled=true)",
        ),
    });
    out
}

/// `hubble-metrics` from cilium-config: empty means no Hubble metrics are
/// exported, so Paqtra's Hubble metrics page has nothing to show.
pub fn eval_hubble_metrics(config: Option<&std::collections::BTreeMap<String, String>>) -> Check {
    let name = "Hubble metrics";
    match config.map(|c| c.get("hubble-metrics").map(|v| v.trim().to_string())) {
        None => Check::info(name, "cilium-config not readable"),
        Some(Some(v)) if !v.is_empty() => Check::ok(name, v),
        Some(_) => Check::warn(
            name,
            "none exported: the Hubble metrics page will be empty",
            "enable some: `paqtra hubble enable --metrics dns,drop,tcp,flow,icmp,http,policy`",
        ),
    }
}

pub fn eval_access(what: &str, allowed: Option<bool>) -> Check {
    let name = format!("RBAC: {what}");
    match allowed {
        Some(true) => Check::ok(&name, "allowed"),
        Some(false) => Check::fail(
            &name,
            "denied for the current user",
            "use a cluster-admin kube-context (the chart creates ClusterRoles and CiliumNetworkPolicy access)",
        ),
        None => Check::warn(&name, "could not be checked", "the API server refused an access review"),
    }
}

/// Only relevant when the release will ask for a PersistentVolume.
pub fn eval_storage(persistence: bool, has_default_class: bool) -> Option<Check> {
    if !persistence {
        return None;
    }
    Some(if has_default_class {
        Check::ok("Default StorageClass", "present")
    } else {
        Check::warn(
            "Default StorageClass",
            "none: the API's PVC may stay Pending",
            "set a class (--set api.persistence.storageClass=NAME) or disable persistence (--set api.persistence.enabled=false)",
        )
    })
}

// ─── Gathering observations ────────────────────────────────────────────────

async fn can_i(
    client: &Client,
    group: &str,
    resource: &str,
    namespace: Option<&str>,
) -> Option<bool> {
    let review = SelfSubjectAccessReview {
        spec: SelfSubjectAccessReviewSpec {
            resource_attributes: Some(ResourceAttributes {
                verb: Some("create".into()),
                group: Some(group.into()),
                resource: Some(resource.into()),
                namespace: namespace.map(str::to_string),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let api: Api<SelfSubjectAccessReview> = Api::all(client.clone());
    let resp = api.create(&PostParams::default(), &review).await.ok()?;
    resp.status.map(|s| s.allowed)
}

/// The `cilium-config` ConfigMap's data, if readable.
pub async fn cilium_config(
    client: &Client,
    ns: &str,
) -> Option<std::collections::BTreeMap<String, String>> {
    let cm: Api<ConfigMap> = Api::namespaced(client.clone(), ns);
    cm.get_opt("cilium-config")
        .await
        .ok()
        .flatten()
        .and_then(|c| c.data)
}

/// Run the checks against the cluster `g` points at. `access` adds the RBAC
/// checks for creating what the chart installs (needed to install, not to diagnose).
pub async fn run(g: &Global, cilium_namespace: &str, persistence: bool, access: bool) -> Report {
    let mut report = Report::default();
    let client = match super::kube::client(g).await {
        Ok(c) => c,
        Err(e) => {
            report.push(Check::fail(
                "Cluster access",
                // Top-level message only: the cause chain is a wall of paths.
                e.to_string(),
                "check your kube-context (`kubectl config current-context`) or pass --context/--kubeconfig",
            ));
            return report;
        }
    };
    match client.apiserver_version().await {
        Ok(v) => {
            report.push(Check::ok(
                "Cluster access",
                format!(
                    "{} reachable",
                    g.context.as_deref().unwrap_or("current context")
                ),
            ));
            report.push(eval_k8s_version(&v.major, &v.minor, &v.git_version));
        }
        Err(e) => {
            report.push(Check::fail(
                "Cluster access",
                format!("API server unreachable: {e}"),
                "check your kube-context and network access",
            ));
            return report;
        }
    }

    report.push(eval_cilium(
        cilium(&client, cilium_namespace).await.as_ref(),
    ));
    let config = cilium_config(&client, cilium_namespace).await;
    let relay = hubble_relay(&client, cilium_namespace).await;
    let enable = config
        .as_ref()
        .and_then(|c| c.get("enable-hubble").cloned());
    report.extend(eval_hubble(enable.as_deref(), relay));
    report.push(eval_hubble_metrics(config.as_ref()));

    for (group, resource, ns, label) in [
        (
            "rbac.authorization.k8s.io",
            "clusterroles",
            None,
            "create ClusterRoles",
        ),
        (
            "rbac.authorization.k8s.io",
            "clusterrolebindings",
            None,
            "create ClusterRoleBindings",
        ),
        (
            "cilium.io",
            "ciliumnetworkpolicies",
            Some(g.namespace.as_str()),
            "create CiliumNetworkPolicies",
        ),
    ] {
        // Diagnosing needs no rights to create things; installing does.
        if access {
            report.push(eval_access(
                label,
                can_i(&client, group, resource, ns).await,
            ));
        }
    }

    if let Some(c) = eval_storage(persistence, has_default_storage_class(&client).await) {
        report.push(c);
    }
    report
}

pub(super) async fn cilium(client: &Client, ns: &str) -> Option<CiliumInfo> {
    let api: Api<DaemonSet> = Api::namespaced(client.clone(), ns);
    let list = api
        .list(&ListParams::default().labels("k8s-app=cilium"))
        .await
        .ok()?;
    let ds = list.items.into_iter().next()?;
    let image = ds
        .spec
        .as_ref()?
        .template
        .spec
        .as_ref()?
        .containers
        .iter()
        .find(|c| c.name == "cilium-agent")
        .or_else(|| ds.spec.as_ref()?.template.spec.as_ref()?.containers.first())?
        .image
        .clone()
        .unwrap_or_default();
    let status = ds.status?;
    Some(CiliumInfo {
        image,
        desired: status.desired_number_scheduled,
        ready: status.number_ready,
    })
}

pub(super) async fn hubble_relay(client: &Client, ns: &str) -> Option<(i32, i32)> {
    let dep: Api<Deployment> = Api::namespaced(client.clone(), ns);
    dep.list(&ListParams::default().labels("k8s-app=hubble-relay"))
        .await
        .ok()
        .and_then(|l| l.items.into_iter().next())
        .map(|d| {
            let s = d.status.unwrap_or_default();
            (
                s.ready_replicas.unwrap_or(0),
                d.spec.and_then(|s| s.replicas).unwrap_or(1),
            )
        })
}

async fn has_default_storage_class(client: &Client) -> bool {
    let api: Api<StorageClass> = Api::all(client.clone());
    api.list(&ListParams::default())
        .await
        .map(|l| {
            l.items.iter().any(|sc| {
                sc.metadata.annotations.as_ref().is_some_and(|a| {
                    a.get("storageclass.kubernetes.io/is-default-class")
                        .map(String::as_str)
                        == Some("true")
                        || a.get("storageclass.beta.kubernetes.io/is-default-class")
                            .map(String::as_str)
                            == Some("true")
                })
            })
        })
        .unwrap_or(false)
}

pub async fn ensure_passes(report: &Report, skip: bool) -> Result<()> {
    if report.has_failures() && !skip {
        anyhow::bail!(
            "preflight checks failed (fix the ✘ items above, or pass --skip-preflight to install anyway)"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cilium(image: &str, ready: i32, desired: i32) -> CiliumInfo {
        CiliumInfo {
            image: image.into(),
            ready,
            desired,
        }
    }

    #[test]
    fn kubernetes_version_thresholds() {
        assert_eq!(eval_k8s_version("1", "24", "v1.24.0").level, Level::Fail);
        assert_eq!(eval_k8s_version("1", "25", "v1.25.0").level, Level::Warn);
        assert_eq!(
            eval_k8s_version("1", "27+", "v1.27.3-eks").level,
            Level::Warn
        );
        assert_eq!(eval_k8s_version("1", "28", "v1.28.0").level, Level::Ok);
        assert_eq!(eval_k8s_version("1", "31+", "v1.31.2-gke").level, Level::Ok);
        assert_eq!(eval_k8s_version("x", "y", "??").level, Level::Warn);
    }

    #[test]
    fn minor_suffixes_are_stripped() {
        assert_eq!(parse_minor("31+"), Some(31));
        assert_eq!(parse_minor("9"), Some(9));
        assert_eq!(parse_minor("+"), None);
    }

    #[test]
    fn reads_the_cilium_version_from_an_image_reference() {
        assert_eq!(
            cilium_version_from_image("quay.io/cilium/cilium:v1.19.0"),
            Some((1, 19, 0))
        );
        assert_eq!(
            cilium_version_from_image("quay.io/cilium/cilium:v1.20.2@sha256:abcdef"),
            Some((1, 20, 2))
        );
        assert_eq!(
            cilium_version_from_image("quay.io/cilium/cilium:v1.18.0-rc.1"),
            Some((1, 18, 0))
        );
        assert_eq!(cilium_version_from_image("quay.io/cilium/cilium"), None);
        assert_eq!(
            cilium_version_from_image("quay.io/cilium/cilium:latest"),
            None
        );
    }

    #[test]
    fn cilium_verdicts() {
        assert_eq!(eval_cilium(None).level, Level::Fail);
        assert_eq!(
            eval_cilium(Some(&cilium("quay.io/cilium/cilium:v1.13.9", 3, 3))).level,
            Level::Fail
        );
        assert_eq!(
            eval_cilium(Some(&cilium("quay.io/cilium/cilium:v1.16.0", 3, 3))).level,
            Level::Warn
        );
        assert_eq!(
            eval_cilium(Some(&cilium("quay.io/cilium/cilium:v1.19.1", 3, 3))).level,
            Level::Ok
        );
        // Not all agents ready: a warning even on a current version.
        let c = eval_cilium(Some(&cilium("quay.io/cilium/cilium:v1.19.1", 2, 3)));
        assert_eq!(c.level, Level::Warn);
        assert!(c.detail.contains("2/3"));
        // Unknown tag: cannot judge the version, but the agents are up.
        assert_eq!(
            eval_cilium(Some(&cilium("registry/cilium:dev", 1, 1))).level,
            Level::Ok
        );
    }

    #[test]
    fn hubble_needs_to_be_on_and_relay_ready() {
        let levels = |c: Vec<Check>| c.iter().map(|c| c.level).collect::<Vec<_>>();
        assert_eq!(
            levels(eval_hubble(Some("true"), Some((1, 1)))),
            [Level::Ok, Level::Ok]
        );
        assert_eq!(
            levels(eval_hubble(Some("false"), Some((1, 1)))),
            [Level::Fail, Level::Ok]
        );
        assert_eq!(
            levels(eval_hubble(Some("true"), Some((0, 1)))),
            [Level::Ok, Level::Fail]
        );
        assert_eq!(
            levels(eval_hubble(Some("true"), None)),
            [Level::Ok, Level::Fail]
        );
        // Missing key: unknown, not off: only a warning; the relay decides.
        assert_eq!(
            levels(eval_hubble(None, Some((2, 2)))),
            [Level::Warn, Level::Ok]
        );
        // A relay scaled to zero is not "ready".
        assert_eq!(
            eval_hubble(Some("true"), Some((0, 0)))[1].level,
            Level::Fail
        );
    }

    #[test]
    fn hubble_metrics_verdicts() {
        let cfg = |v: &str| {
            let mut m = std::collections::BTreeMap::new();
            m.insert("hubble-metrics".to_string(), v.to_string());
            m
        };
        assert_eq!(
            eval_hubble_metrics(Some(&cfg("dns drop tcp"))).level,
            Level::Ok
        );
        assert_eq!(eval_hubble_metrics(Some(&cfg("  "))).level, Level::Warn);
        assert_eq!(
            eval_hubble_metrics(Some(&std::collections::BTreeMap::new())).level,
            Level::Warn
        );
        assert_eq!(eval_hubble_metrics(None).level, Level::Info);
    }

    #[test]
    fn info_is_not_a_warning() {
        let mut r = Report::default();
        r.push(Check::info("x", "fyi"));
        assert!(!r.has_warnings() && !r.has_failures());
        assert!(r.render().contains("fyi"));
    }

    #[test]
    fn access_and_storage_checks() {
        assert_eq!(eval_access("x", Some(true)).level, Level::Ok);
        assert_eq!(eval_access("x", Some(false)).level, Level::Fail);
        assert_eq!(eval_access("x", None).level, Level::Warn);
        assert!(eval_storage(false, false).is_none(), "no PVC, no check");
        assert_eq!(eval_storage(true, true).unwrap().level, Level::Ok);
        assert_eq!(eval_storage(true, false).unwrap().level, Level::Warn);
    }

    #[test]
    fn report_flags_and_rendering() {
        let mut r = Report::default();
        r.push(Check::ok("a", "fine"));
        assert!(!r.has_failures() && !r.has_warnings());
        r.push(Check::warn("b", "meh", "do x"));
        assert!(r.has_warnings() && !r.has_failures());
        r.push(Check::fail("c", "bad", "do y"));
        assert!(r.has_failures());
        let text = r.render();
        assert!(text.contains("do x") && text.contains("do y") && text.contains("fine"));
        // A passing check prints no hint line.
        assert_eq!(text.matches('→').count(), 2);
    }

    #[tokio::test]
    async fn failures_block_unless_skipped() {
        let mut r = Report::default();
        r.push(Check::fail("c", "bad", "fix"));
        assert!(ensure_passes(&r, false).await.is_err());
        assert!(ensure_passes(&r, true).await.is_ok());
        let mut w = Report::default();
        w.push(Check::warn("c", "meh", "fix"));
        assert!(
            ensure_passes(&w, false).await.is_ok(),
            "warnings never block"
        );
    }
}
