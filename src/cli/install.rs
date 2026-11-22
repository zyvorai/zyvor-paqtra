// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! `paqtra install | upgrade | uninstall`: the Helm release lifecycle, like
//! `cilium install | upgrade | uninstall`.

use anyhow::{bail, Context, Result};
use k8s_openapi::api::core::v1::{PersistentVolumeClaim, Pod};
use kube::api::{DeleteParams, ListParams};
use kube::Api;
use owo_colors::OwoColorize;
use regex::Regex;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use super::chart::{self, CLI_VERSION};
use super::cilium;
use super::global::Global;
use super::helm::{self, Caps};
use super::preflight;
use super::registry;

const DEFAULT_CILIUM_NAMESPACE: &str = "kube-system";
/// Namespaces `--purge` will never delete.
const PROTECTED_NAMESPACES: &[&str] = &["kube-system", "kube-public", "kube-node-lease", "default"];

#[derive(Debug, Clone)]
pub struct InstallOpts {
    /// Helm chart directory (default: the chart built into this binary)
    pub chart_directory: Option<PathBuf>,
    /// Chart/app version to install (default: this CLI's version)
    pub version: Option<String>,
    pub values: Vec<PathBuf>,
    pub set: Vec<String>,
    pub set_string: Vec<String>,
    pub set_file: Vec<String>,
    /// Mirror prefix for the three images, e.g. `registry.corp/zyvorai`
    pub registry: Option<String>,
    pub wait: bool,
    /// Helm duration, e.g. `5m`
    pub wait_duration: String,
    pub dry_run: bool,
    pub atomic: bool,
    pub skip_preflight: bool,
    pub list_versions: bool,
    /// upgrade: discard previous values instead of reusing them
    pub reset_values: bool,
    /// Install Cilium (with Hubble + Relay) first when none is running
    pub with_cilium: bool,
    pub cilium_version: String,
    pub cilium_set: Vec<String>,
}

impl Default for InstallOpts {
    fn default() -> Self {
        Self {
            chart_directory: None,
            version: None,
            values: Vec::new(),
            set: Vec::new(),
            set_string: Vec::new(),
            set_file: Vec::new(),
            registry: None,
            wait: true,
            wait_duration: "5m".into(),
            dry_run: false,
            atomic: false,
            skip_preflight: false,
            list_versions: false,
            reset_values: false,
            with_cilium: false,
            cilium_version: cilium::DEFAULT_VERSION.into(),
            cilium_set: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Install,
    Upgrade,
}

static DURATION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+[smh])+$").unwrap());
static REGISTRY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9._:/-]+$").unwrap());

/// `--registry mirror.corp/zyvorai` -> the three image repositories. The `,` and
/// `=` that `helm --set` treats specially are rejected up front by the regex.
/// `90s`, `5m`, `1h30m` -> a Duration (the same grammar helm's --timeout takes).
pub fn parse_duration(s: &str) -> Result<std::time::Duration> {
    if !DURATION_RE.is_match(s) {
        bail!("{s:?} is not a duration like 90s, 5m or 1h30m");
    }
    let mut total = 0u64;
    let mut digits = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            let n: u64 = digits.parse().context("duration too large")?;
            digits.clear();
            total = total.saturating_add(n.saturating_mul(match ch {
                's' => 1,
                'm' => 60,
                _ => 3600,
            }));
        }
    }
    Ok(std::time::Duration::from_secs(total))
}

pub fn registry_overrides(registry: &str) -> Result<Vec<String>> {
    let prefix = registry.trim_end_matches('/');
    if prefix.is_empty() || !REGISTRY_RE.is_match(prefix) {
        bail!(
            "--registry {registry:?} is not a valid image prefix (use e.g. registry.corp/zyvorai)"
        );
    }
    Ok([
        ("api", "paqtra-api"),
        ("ui", "paqtra-ui"),
        ("agent", "paqtra"),
    ]
    .iter()
    .map(|(component, image)| format!("{component}.image.repository={prefix}/{image}"))
    .collect())
}

/// The whole `helm` argument list for an install or upgrade. Pure, so the
/// flag handling is tested without running helm. User `--set`s come after the
/// `--registry` ones, so an explicit value always wins.
pub fn build_helm_args(
    mode: Mode,
    g: &Global,
    chart_path: &Path,
    o: &InstallOpts,
    caps: Caps,
) -> Result<Vec<String>> {
    if !DURATION_RE.is_match(&o.wait_duration) {
        bail!(
            "--wait-duration {:?} must look like 5m, 300s or 1h30m",
            o.wait_duration
        );
    }
    let mut a: Vec<String> = vec!["upgrade".into()];
    match mode {
        Mode::Install => a.push("--install".into()),
        Mode::Upgrade => {}
    }
    a.extend([
        g.release.clone(),
        chart_path.display().to_string(),
        "--namespace".into(),
        g.namespace.clone(),
    ]);
    match mode {
        Mode::Install => {
            a.push("--create-namespace".into());
            a.extend(["--set".into(), format!("global.namespace={}", g.namespace)]);
        }
        Mode::Upgrade if o.reset_values => a.push("--reset-values".into()),
        // New chart defaults + the values the release already had. Older helm can
        // only reuse (which keeps stale defaults for keys the new chart added).
        Mode::Upgrade if caps.reset_then_reuse => a.push("--reset-then-reuse-values".into()),
        Mode::Upgrade => a.push("--reuse-values".into()),
    }

    if let Some(reg) = &o.registry {
        for s in registry_overrides(reg)? {
            a.extend(["--set".into(), s]);
        }
    }
    for f in &o.values {
        a.extend(["--values".into(), f.display().to_string()]);
    }
    for s in &o.set {
        a.extend(["--set".into(), s.clone()]);
    }
    for s in &o.set_string {
        a.extend(["--set-string".into(), s.clone()]);
    }
    for s in &o.set_file {
        a.extend(["--set-file".into(), s.clone()]);
    }

    if o.atomic {
        a.push("--atomic".into());
    }
    if o.wait || o.atomic {
        a.extend(["--wait".into(), "--timeout".into(), o.wait_duration.clone()]);
    }
    if o.dry_run {
        a.push(
            if caps.dry_run_server {
                "--dry-run=server"
            } else {
                "--dry-run"
            }
            .into(),
        );
    }
    Ok(a)
}

/// Will the release ask for a PersistentVolume? The chart default, overridden
/// by `-f` files and `--set api.persistence.enabled=`, later ones winning.
pub fn persistence_wanted(chart_dir: &Path, values: &[PathBuf], sets: &[String]) -> bool {
    fn from_yaml(text: &str) -> Option<bool> {
        let v: serde_yaml::Value = serde_yaml::from_str(text).ok()?;
        v.get("api")?.get("persistence")?.get("enabled")?.as_bool()
    }
    let mut wanted = std::fs::read_to_string(chart_dir.join("values.yaml"))
        .ok()
        .and_then(|t| from_yaml(&t))
        .unwrap_or(true);
    for f in values {
        if let Some(b) = std::fs::read_to_string(f).ok().and_then(|t| from_yaml(&t)) {
            wanted = b;
        }
    }
    for s in sets {
        if let Some(v) = s.strip_prefix("api.persistence.enabled=") {
            wanted = !matches!(v.trim(), "false" | "0");
        }
    }
    wanted
}

fn print_versions_table(versions: &[String]) {
    println!("Available Paqtra versions (newest first):");
    for v in versions {
        let mark = if v == CLI_VERSION {
            format!("  {}", "← this CLI".dimmed())
        } else {
            String::new()
        };
        println!("  {v}{mark}");
    }
}

pub async fn cmd_install(g: &Global, o: InstallOpts) -> Result<()> {
    if o.list_versions {
        print_versions_table(&registry::list_versions().await?);
        return Ok(());
    }
    run_release(g, o, Mode::Install).await
}

pub async fn cmd_upgrade(g: &Global, o: InstallOpts) -> Result<()> {
    run_release(g, o, Mode::Upgrade).await
}

async fn run_release(g: &Global, o: InstallOpts, mode: Mode) -> Result<()> {
    let helm = helm::ensure(g).await?;
    let chart = chart::resolve(o.chart_directory.as_deref(), o.version.as_deref(), &helm).await?;
    let verb = if mode == Mode::Install {
        "Installing"
    } else {
        "Upgrading"
    };

    println!(
        "{} {verb} Paqtra {} into namespace {} (release {})",
        "→".cyan(),
        chart.version.bold(),
        g.namespace.bold(),
        g.release
    );
    println!("  Chart: {}", chart.origin.dimmed());
    if chart.version != CLI_VERSION && !chart.version.is_empty() {
        println!(
            "  {} chart {} differs from this CLI ({CLI_VERSION}); image tags follow the chart",
            "⚠".yellow(),
            chart.version
        );
    }

    if mode == Mode::Install && o.with_cilium {
        install_cilium_if_missing(g, &helm, &o).await?;
    }

    if mode == Mode::Install {
        let persistence = persistence_wanted(&chart.path, &o.values, &o.set);
        println!("{} Checking prerequisites...", "→".cyan());
        let report = preflight::run(g, DEFAULT_CILIUM_NAMESPACE, persistence, true).await;
        print!("{}", report.render());
        preflight::ensure_passes(&report, o.skip_preflight).await?;
        println!();
    }

    let args = build_helm_args(mode, g, &chart.path, &o, helm.caps())?;
    let stdout = helm
        .run_ok(
            &args,
            if mode == Mode::Install {
                "helm install"
            } else {
                "helm upgrade"
            },
        )
        .await?;
    print!("{stdout}");

    if o.dry_run {
        println!("{} Dry run only: nothing was changed.", "✔".green());
    } else {
        println!(
            "{} Paqtra {}. Run `{}` to check.",
            "✔".green(),
            if mode == Mode::Install {
                "installed"
            } else {
                "upgraded"
            },
            "paqtra status --wait".cyan()
        );
    }
    Ok(())
}

/// `--with-cilium`: idempotent. If any Cilium agent is already running (however
/// it was installed) it is left alone; Paqtra never upgrades a CNI as a side effect.
async fn install_cilium_if_missing(g: &Global, helm: &helm::Helm, o: &InstallOpts) -> Result<()> {
    let client = super::kube::client(g).await?;
    if cilium::is_present(&client).await {
        println!(
            "{} Cilium is already running; leaving it as it is (--with-cilium skipped)",
            "ℹ".blue()
        );
        return Ok(());
    }
    println!(
        "{} Installing Cilium {} with Hubble, Relay and metrics (this can take a few minutes)...",
        "→".cyan(),
        o.cilium_version.bold()
    );
    let args = cilium::install_args(&o.cilium_version, &o.cilium_set, "10m")?;
    if o.dry_run {
        println!("  dry run: would run `helm {}`", args.join(" "));
        return Ok(());
    }
    helm.run_ok(&args, "helm install cilium").await?;
    // Helm can return before the agents are up; the checks that follow need them.
    println!("  Waiting for Cilium and Hubble Relay to be ready...");
    cilium::wait_ready(&client, std::time::Duration::from_secs(600)).await?;
    println!("{} Cilium installed and ready.\n", "✔".green());
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct UninstallOpts {
    pub wait: bool,
    /// Also delete the release's PVCs and, if nothing else lives there, the namespace
    pub purge: bool,
    pub yes: bool,
}

fn confirm(question: &str, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        bail!("{question} needs confirmation, but stdin is not a terminal (pass --yes)");
    }
    print!("{question} [y/N] ");
    std::io::Write::flush(&mut std::io::stdout())?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

pub async fn cmd_uninstall(g: &Global, o: UninstallOpts) -> Result<()> {
    if o.purge {
        if PROTECTED_NAMESPACES.contains(&g.namespace.as_str()) {
            bail!(
                "--purge would touch namespace {:?}, which is protected; run without --purge",
                g.namespace
            );
        }
        if !confirm(
            &format!(
                "--purge deletes the release's PersistentVolumeClaims (flow history) in namespace {:?}. Continue?",
                g.namespace
            ),
            o.yes,
        )? {
            println!("Aborted; nothing was changed.");
            return Ok(());
        }
    }

    let helm = helm::ensure(g).await?;
    println!(
        "{} Uninstalling release {} from {}...",
        "→".cyan(),
        g.release.bold(),
        g.namespace.bold()
    );
    let mut args: Vec<String> = vec![
        "uninstall".into(),
        g.release.clone(),
        "--namespace".into(),
        g.namespace.clone(),
    ];
    if o.wait {
        args.push("--wait".into());
    }
    let out = helm.run(&args).await?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() && !stderr.contains("not found") {
        helm::ensure_ok(&out, "helm uninstall")?;
    }
    if stderr.contains("not found") {
        println!(
            "{} No release {:?} in {:?}; nothing to uninstall.",
            "ℹ".blue(),
            g.release,
            g.namespace
        );
    } else {
        println!("{} Paqtra uninstalled.", "✔".green());
    }

    if o.purge {
        purge(g).await?;
    }
    Ok(())
}

async fn purge(g: &Global) -> Result<()> {
    let client = super::kube::client(g).await?;
    let pvcs: Api<PersistentVolumeClaim> = Api::namespaced(client.clone(), &g.namespace);
    let selector = format!("app.kubernetes.io/instance={}", g.release);
    let list = pvcs
        .list(&ListParams::default().labels(&selector))
        .await
        .context("cannot list PVCs")?;
    for pvc in list.items {
        if let Some(name) = pvc.metadata.name {
            pvcs.delete(&name, &DeleteParams::default())
                .await
                .with_context(|| format!("cannot delete PVC {name}"))?;
            println!("  deleted PVC {name}");
        }
    }

    // The namespace goes only if nothing else is running in it.
    let pods: Api<Pod> = Api::namespaced(client.clone(), &g.namespace);
    let others = pods
        .list(&ListParams::default())
        .await
        .map(|l| l.items.len())
        .unwrap_or(1);
    if others == 0 {
        let ns: Api<k8s_openapi::api::core::v1::Namespace> = Api::all(client);
        ns.delete(&g.namespace, &DeleteParams::default()).await.ok();
        println!("  deleted namespace {}", g.namespace);
    } else {
        println!(
            "  namespace {} kept: {others} pod(s) still there",
            g.namespace
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g() -> Global {
        Global {
            namespace: "obs".into(),
            release: "px".into(),
            ..Default::default()
        }
    }
    fn caps(dry: bool, reset_reuse: bool) -> Caps {
        Caps {
            dry_run_server: dry,
            reset_then_reuse: reset_reuse,
        }
    }
    fn args(mode: Mode, o: &InstallOpts, c: Caps) -> Vec<String> {
        build_helm_args(mode, &g(), Path::new("/tmp/chart"), o, c).unwrap()
    }
    fn has_pair(a: &[String], flag: &str, val: &str) -> bool {
        a.windows(2).any(|w| w[0] == flag && w[1] == val)
    }

    #[test]
    fn durations_parse_like_helms() {
        assert_eq!(parse_duration("90s").unwrap().as_secs(), 90);
        assert_eq!(parse_duration("5m").unwrap().as_secs(), 300);
        assert_eq!(parse_duration("1h30m").unwrap().as_secs(), 5400);
        for bad in ["", "5", "m", "5 m", "-1m", "1d"] {
            assert!(parse_duration(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn install_args_are_the_documented_shape() {
        let a = args(Mode::Install, &InstallOpts::default(), caps(true, true));
        assert_eq!(
            &a[..6],
            [
                "upgrade",
                "--install",
                "px",
                "/tmp/chart",
                "--namespace",
                "obs"
            ]
        );
        assert!(a.contains(&"--create-namespace".to_string()));
        assert!(has_pair(&a, "--set", "global.namespace=obs"));
        assert!(has_pair(&a, "--timeout", "5m") && a.contains(&"--wait".to_string()));
        assert!(!a
            .iter()
            .any(|x| x.starts_with("--dry-run") || x == "--atomic"));
    }

    #[test]
    fn no_wait_drops_the_wait_flags_but_atomic_forces_them() {
        let o = InstallOpts {
            wait: false,
            ..Default::default()
        };
        assert!(!args(Mode::Install, &o, caps(true, true)).contains(&"--wait".to_string()));
        let o = InstallOpts {
            wait: false,
            atomic: true,
            wait_duration: "90s".into(),
            ..Default::default()
        };
        let a = args(Mode::Install, &o, caps(true, true));
        assert!(a.contains(&"--atomic".to_string()) && a.contains(&"--wait".to_string()));
        assert!(has_pair(&a, "--timeout", "90s"));
    }

    #[test]
    fn dry_run_is_server_side_only_where_helm_supports_it() {
        let o = InstallOpts {
            dry_run: true,
            ..Default::default()
        };
        assert!(args(Mode::Install, &o, caps(true, true)).contains(&"--dry-run=server".to_string()));
        assert!(args(Mode::Install, &o, caps(false, false)).contains(&"--dry-run".to_string()));
    }

    #[test]
    fn upgrade_keeps_values_the_best_way_helm_allows() {
        let o = InstallOpts::default();
        let new = args(Mode::Upgrade, &o, caps(true, true));
        assert!(new.contains(&"--reset-then-reuse-values".to_string()));
        assert!(
            !new.contains(&"--install".to_string())
                && !new.contains(&"--create-namespace".to_string())
        );
        assert!(args(Mode::Upgrade, &o, caps(true, false)).contains(&"--reuse-values".to_string()));
        let reset = InstallOpts {
            reset_values: true,
            ..Default::default()
        };
        assert!(
            args(Mode::Upgrade, &reset, caps(true, true)).contains(&"--reset-values".to_string())
        );
    }

    #[test]
    fn values_and_sets_pass_through_with_user_sets_after_registry_sets() {
        let o = InstallOpts {
            registry: Some("mirror.corp/zy/".into()),
            values: vec!["a.yaml".into(), "b.yaml".into()],
            set: vec!["api.image.tag=x".into()],
            set_string: vec!["k=1".into()],
            set_file: vec!["cert=/tmp/c.pem".into()],
            ..Default::default()
        };
        let a = args(Mode::Install, &o, caps(true, true));
        assert!(has_pair(&a, "--values", "a.yaml") && has_pair(&a, "--values", "b.yaml"));
        assert!(
            has_pair(&a, "--set-string", "k=1") && has_pair(&a, "--set-file", "cert=/tmp/c.pem")
        );
        assert!(has_pair(
            &a,
            "--set",
            "api.image.repository=mirror.corp/zy/paqtra-api"
        ));
        assert!(has_pair(
            &a,
            "--set",
            "agent.image.repository=mirror.corp/zy/paqtra"
        ));
        let reg = a
            .iter()
            .position(|x| x == "api.image.repository=mirror.corp/zy/paqtra-api")
            .unwrap();
        let user = a.iter().position(|x| x == "api.image.tag=x").unwrap();
        assert!(reg < user, "explicit --set must win over --registry");
    }

    #[test]
    fn bad_registry_and_duration_are_rejected() {
        for bad in ["", "a b", "reg,istry", "reg=x", "/", "rëg"] {
            assert!(registry_overrides(bad).is_err(), "{bad:?}");
        }
        assert_eq!(registry_overrides("r.io/a").unwrap().len(), 3);
        for bad in ["5", "m5", "5 m", "", "1h 30m"] {
            let o = InstallOpts {
                wait_duration: bad.into(),
                ..Default::default()
            };
            assert!(
                build_helm_args(Mode::Install, &g(), Path::new("c"), &o, caps(true, true)).is_err(),
                "{bad:?}"
            );
        }
        for ok in ["5m", "300s", "1h30m"] {
            let o = InstallOpts {
                wait_duration: ok.into(),
                ..Default::default()
            };
            assert!(
                build_helm_args(Mode::Install, &g(), Path::new("c"), &o, caps(true, true)).is_ok(),
                "{ok:?}"
            );
        }
    }

    #[test]
    fn persistence_follows_defaults_files_and_sets_in_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("values.yaml"),
            "api:\n  persistence:\n    enabled: false\n",
        )
        .unwrap();
        assert!(!persistence_wanted(dir.path(), &[], &[]));
        assert!(persistence_wanted(
            dir.path(),
            &[],
            &["api.persistence.enabled=true".into()]
        ));

        let f = dir.path().join("extra.yaml");
        std::fs::write(&f, "api:\n  persistence:\n    enabled: true\n").unwrap();
        assert!(persistence_wanted(dir.path(), &[f.clone()], &[]));
        assert!(!persistence_wanted(
            dir.path(),
            &[f],
            &["api.persistence.enabled=false".into()]
        ));
        // No values.yaml at all: assume the chart wants a PVC (it does by default).
        assert!(persistence_wanted(&dir.path().join("nope"), &[], &[]));
    }

    #[test]
    fn the_shipped_chart_wants_persistence_by_default() {
        // The real chart: guards the StorageClass preflight against a values rename.
        assert!(persistence_wanted(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("chart")
                .as_path(),
            &[],
            &[]
        ));
    }
}
