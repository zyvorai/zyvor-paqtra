// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Installing and configuring Cilium/Hubble through Cilium's own Helm chart,
//! for `paqtra install --with-cilium` and `paqtra hubble enable|disable`.
//! Paqtra only drives Cilium's public chart; it never touches its datapath.

use anyhow::{bail, Result};
use k8s_openapi::api::apps::v1::DaemonSet;
use kube::api::ListParams;
use kube::{Api, Client};
use regex::Regex;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use super::helm::Helm;
use super::preflight;
use super::version::{chart_version_of, parse_release, ReleaseInfo};

pub const REPO: &str = "https://helm.cilium.io";
/// Cilium version installed by `--with-cilium` (>= the recommended 1.19).
pub const DEFAULT_VERSION: &str = "1.20.2";
pub const NAMESPACE: &str = "kube-system";
/// Hubble metrics Paqtra's Insights page reads (see services/cilium_metrics.rs).
pub const DEFAULT_HUBBLE_METRICS: &str = "dns,drop,tcp,flow,icmp,http,policy";

static METRIC_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9:;._=-]+$").unwrap());
static VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d+\.\d+\.\d+([-.][A-Za-z0-9.]+)?$").unwrap());

/// `dns,drop` -> `{dns,drop}` for `--set`; rejects anything that could break out
/// of the value (braces, quotes, spaces, empty entries).
pub fn metrics_value(list: &str) -> Result<String> {
    let items: Vec<&str> = list.split(',').map(str::trim).collect();
    if items.iter().any(|i| i.is_empty() || !METRIC_RE.is_match(i)) {
        bail!("--metrics {list:?} must be a comma-separated list like dns,drop,tcp");
    }
    Ok(format!("{{{}}}", items.join(",")))
}

pub fn check_version(v: &str) -> Result<()> {
    if VERSION_RE.is_match(v) {
        Ok(())
    } else {
        bail!("{v:?} is not a Cilium version like 1.20.2");
    }
}

/// `helm upgrade --install cilium` with Hubble, Relay and metrics on. The user's
/// `--cilium-set` values come last so they can override anything here.
pub fn install_args(version: &str, sets: &[String], wait_duration: &str) -> Result<Vec<String>> {
    check_version(version)?;
    let mut a: Vec<String> = [
        "upgrade",
        "--install",
        "cilium",
        "cilium",
        "--repo",
        REPO,
        "--version",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.extend([version.to_string(), "--namespace".into(), NAMESPACE.into()]);
    for s in [
        "hubble.enabled=true".to_string(),
        "hubble.relay.enabled=true".to_string(),
        format!(
            "hubble.metrics.enabled={}",
            metrics_value(DEFAULT_HUBBLE_METRICS)?
        ),
    ] {
        a.extend(["--set".into(), s]);
    }
    for s in sets {
        a.extend(["--set".into(), s.clone()]);
    }
    a.extend([
        "--wait".into(),
        "--timeout".into(),
        wait_duration.to_string(),
    ]);
    Ok(a)
}

/// Switch Hubble on or off on an existing Cilium release, keeping everything
/// else (`--reuse-values`) and staying on the installed chart version.
pub fn hubble_args(
    enable: bool,
    relay: bool,
    metrics: Option<&str>,
    chart_version: &str,
    wait_duration: &str,
) -> Result<Vec<String>> {
    check_version(chart_version)?;
    let mut a: Vec<String> = ["upgrade", "cilium", "cilium", "--repo", REPO, "--version"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    a.extend([
        chart_version.to_string(),
        "--namespace".into(),
        NAMESPACE.into(),
        "--reuse-values".into(),
    ]);
    a.extend(["--set".into(), format!("hubble.enabled={enable}")]);
    if enable {
        a.extend(["--set".into(), format!("hubble.relay.enabled={relay}")]);
        if let Some(m) = metrics {
            a.extend([
                "--set".into(),
                format!("hubble.metrics.enabled={}", metrics_value(m)?),
            ]);
        }
    }
    a.extend([
        "--wait".into(),
        "--timeout".into(),
        wait_duration.to_string(),
    ]);
    Ok(a)
}

/// Cilium and Hubble Relay are both up: every scheduled agent ready, and at
/// least one Relay replica. Helm's own `--wait` is not enough (it can return
/// while a DaemonSet is still rolling out, e.g. on nodes with no CNI yet).
pub fn is_ready(agents: Option<(i32, i32)>, relay: Option<(i32, i32)>) -> bool {
    let up = |c: Option<(i32, i32)>| matches!(c, Some((ready, desired)) if desired > 0 && ready >= desired);
    up(agents) && up(relay)
}

/// Poll until [`is_ready`], or fail with what was seen.
pub async fn wait_ready(client: &Client, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    loop {
        let agents = preflight::cilium(client, NAMESPACE)
            .await
            .map(|i| (i.ready, i.desired));
        let relay = preflight::hubble_relay(client, NAMESPACE).await;
        if is_ready(agents, relay) {
            return Ok(());
        }
        if start.elapsed() > timeout {
            bail!(
                "Cilium and Hubble Relay were not ready after {}s (agents ready/desired: {agents:?}, relay: {relay:?}); \
                 check `kubectl -n kube-system get pods`",
                timeout.as_secs()
            );
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

/// Cilium's own Helm release in kube-system, if it was installed with Helm.
pub async fn find_release(helm: &Helm) -> Result<Option<ReleaseInfo>> {
    let args: Vec<String> = [
        "list",
        "--namespace",
        NAMESPACE,
        "--filter",
        "^cilium$",
        "--output",
        "json",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let out = helm.run_ok(&args, "helm list").await?;
    Ok(parse_release(&out, "cilium"))
}

/// The installed chart version of a Cilium release (`cilium-1.20.2` -> `1.20.2`).
pub fn installed_version(r: &ReleaseInfo) -> &str {
    chart_version_of(&r.chart)
}

/// Is a Cilium agent DaemonSet running (installed by anything, not just Helm)?
pub async fn is_present(client: &Client) -> bool {
    let api: Api<DaemonSet> = Api::namespaced(client.clone(), NAMESPACE);
    api.list(&ListParams::default().labels("k8s-app=cilium"))
        .await
        .map(|l| !l.items.is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_pair(a: &[String], flag: &str, val: &str) -> bool {
        a.windows(2).any(|w| w[0] == flag && w[1] == val)
    }

    #[test]
    fn metrics_are_braced_and_validated() {
        assert_eq!(metrics_value("dns,drop").unwrap(), "{dns,drop}");
        assert_eq!(metrics_value(" dns , tcp ").unwrap(), "{dns,tcp}");
        assert_eq!(
            metrics_value("dns:query;ignoreAAAA").unwrap(),
            "{dns:query;ignoreAAAA}"
        );
        for bad in ["", "dns,,drop", "dns}", "{dns", "dns drop", "a\"b", ","] {
            assert!(metrics_value(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn only_real_versions_are_accepted() {
        for ok in ["1.20.2", "1.19.0", "1.19.0-rc.1"] {
            assert!(check_version(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "1.20",
            "v1.20.2",
            "latest",
            "1.20.2; rm -rf /",
            "1.20.2 --set x=y",
        ] {
            assert!(check_version(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn install_turns_hubble_relay_and_metrics_on() {
        let a = install_args("1.20.2", &[], "10m").unwrap();
        assert_eq!(&a[..4], ["upgrade", "--install", "cilium", "cilium"]);
        assert!(has_pair(&a, "--repo", REPO) && has_pair(&a, "--version", "1.20.2"));
        assert!(has_pair(&a, "--namespace", "kube-system"));
        assert!(has_pair(&a, "--set", "hubble.enabled=true"));
        assert!(has_pair(&a, "--set", "hubble.relay.enabled=true"));
        assert!(has_pair(
            &a,
            "--set",
            "hubble.metrics.enabled={dns,drop,tcp,flow,icmp,http,policy}"
        ));
        assert!(has_pair(&a, "--timeout", "10m") && a.contains(&"--wait".to_string()));
    }

    #[test]
    fn user_sets_come_last_so_they_win() {
        let a = install_args(
            "1.20.2",
            &[
                "operator.replicas=1".into(),
                "hubble.relay.enabled=false".into(),
            ],
            "5m",
        )
        .unwrap();
        let ours = a
            .iter()
            .position(|x| x == "hubble.relay.enabled=true")
            .unwrap();
        let theirs = a
            .iter()
            .position(|x| x == "hubble.relay.enabled=false")
            .unwrap();
        assert!(ours < theirs);
        assert!(a.contains(&"operator.replicas=1".to_string()));
        assert!(install_args("nope", &[], "5m").is_err());
    }

    #[test]
    fn hubble_enable_keeps_other_values_and_the_installed_version() {
        let a = hubble_args(true, true, Some("dns,drop"), "1.20.2", "5m").unwrap();
        assert!(a.contains(&"--reuse-values".to_string()));
        assert!(has_pair(&a, "--version", "1.20.2"));
        assert!(has_pair(&a, "--set", "hubble.enabled=true"));
        assert!(has_pair(&a, "--set", "hubble.relay.enabled=true"));
        assert!(has_pair(&a, "--set", "hubble.metrics.enabled={dns,drop}"));
        assert!(
            !a.contains(&"--install".to_string()),
            "never installs Cilium by accident"
        );
    }

    #[test]
    fn hubble_disable_only_turns_hubble_off() {
        let a = hubble_args(false, true, Some("dns"), "1.20.2", "5m").unwrap();
        assert!(has_pair(&a, "--set", "hubble.enabled=false"));
        assert!(!a
            .iter()
            .any(|x| x.starts_with("hubble.relay") || x.starts_with("hubble.metrics")));
        assert!(hubble_args(true, true, Some("dns}"), "1.20.2", "5m").is_err());
        assert!(hubble_args(true, true, None, "v1.20", "5m").is_err());
    }

    #[test]
    fn ready_means_all_agents_and_a_relay() {
        assert!(is_ready(Some((2, 2)), Some((1, 1))));
        assert!(
            !is_ready(Some((0, 2)), Some((1, 1))),
            "agents still starting"
        );
        assert!(!is_ready(Some((1, 2)), Some((1, 1))));
        assert!(
            !is_ready(Some((2, 2)), Some((0, 1))),
            "relay still starting"
        );
        assert!(
            !is_ready(Some((0, 0)), Some((1, 1))),
            "no agents scheduled is not ready"
        );
        assert!(!is_ready(None, Some((1, 1))));
        assert!(!is_ready(Some((2, 2)), None));
    }

    #[test]
    fn reads_the_chart_version_of_a_release() {
        let r = ReleaseInfo {
            name: "cilium".into(),
            namespace: "kube-system".into(),
            chart: "cilium-1.20.2".into(),
            app_version: "1.20.2".into(),
            status: "deployed".into(),
        };
        assert_eq!(installed_version(&r), "1.20.2");
    }
}
