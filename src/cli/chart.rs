// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Where the Helm chart comes from. The chart is compiled into the binary, so a
//! downloaded `paqtra` can install itself with no files beside it, and it
//! always matches the CLI's version (CLI vX installs chart vX, image tag vX).
//! Another version is pulled from the OCI registry on request.

use anyhow::{bail, Context, Result};
use include_dir::{include_dir, Dir};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

use super::helm::Helm;

static EMBEDDED: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/chart");

/// The version of this binary, which is also the chart and image version.
pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Where released charts are published (see .github/workflows/release.yml).
pub const OCI_CHART: &str = "oci://ghcr.io/zyvorai/charts/paqtra";

/// A chart on disk plus what the user should be told about where it came from.
pub struct ResolvedChart {
    pub path: PathBuf,
    pub version: String,
    pub origin: String,
    /// Keeps a temporary extraction alive for as long as the chart is used.
    _tmp: Option<TempDir>,
}

/// `version:` of a Chart.yaml.
pub fn chart_version(chart_yaml: &str) -> Option<String> {
    chart_yaml.lines().find_map(|l| {
        l.strip_prefix("version:")
            .map(|v| v.trim().trim_matches('"').to_string())
    })
}

pub fn embedded_version() -> String {
    EMBEDDED
        .get_file("Chart.yaml")
        .and_then(|f| f.contents_utf8())
        .and_then(chart_version)
        .unwrap_or_else(|| CLI_VERSION.to_string())
}

/// The embedded chart's default values (used to reason about defaults, e.g.
/// whether persistence needs a StorageClass).
pub fn embedded_values() -> Option<serde_yaml::Value> {
    let text = EMBEDDED.get_file("values.yaml")?.contents_utf8()?;
    serde_yaml::from_str(text).ok()
}

fn normalize(v: &str) -> &str {
    v.strip_prefix('v').unwrap_or(v)
}

fn has_chart_yaml(dir: &Path) -> bool {
    dir.join("Chart.yaml").is_file()
}

fn local(path: &Path, origin: String) -> Result<ResolvedChart> {
    let version = std::fs::read_to_string(path.join("Chart.yaml"))
        .ok()
        .and_then(|t| chart_version(&t))
        .unwrap_or_else(|| "unknown".into());
    Ok(ResolvedChart {
        path: path.to_path_buf(),
        version,
        origin,
        _tmp: None,
    })
}

fn extract_embedded() -> Result<ResolvedChart> {
    let tmp = TempDir::new().context("cannot create a temporary directory")?;
    let dir = tmp.path().join("paqtra");
    std::fs::create_dir_all(&dir)?;
    EMBEDDED
        .extract(&dir)
        .context("cannot unpack the embedded chart")?;
    Ok(ResolvedChart {
        path: dir,
        version: embedded_version(),
        origin: "built into this paqtra binary".into(),
        _tmp: Some(tmp),
    })
}

/// Pick the chart:
///  1. `--chart-directory` (must exist),
///  2. `PAQTRA_CHART_DIR` (must be valid when set; a typo is an error, not a
///     silent fallback to a different chart),
///  3. `--version X` other than this binary's: pulled from the OCI registry,
///  4. the embedded chart.
pub async fn resolve(
    explicit: Option<&Path>,
    version: Option<&str>,
    helm: &Helm,
) -> Result<ResolvedChart> {
    if let Some(dir) = explicit {
        if !has_chart_yaml(dir) {
            bail!("no Chart.yaml in --chart-directory {}", dir.display());
        }
        return local(dir, format!("directory {}", dir.display()));
    }
    if let Some(env) = std::env::var_os("PAQTRA_CHART_DIR").filter(|v| !v.is_empty()) {
        let dir = PathBuf::from(env);
        if !has_chart_yaml(&dir) {
            bail!(
                "PAQTRA_CHART_DIR={} has no Chart.yaml (unset it to use the built-in chart)",
                dir.display()
            );
        }
        return local(&dir, format!("PAQTRA_CHART_DIR {}", dir.display()));
    }
    match version.map(normalize) {
        Some(v) if v != CLI_VERSION => pull_oci(helm, v).await,
        _ => extract_embedded(),
    }
}

async fn pull_oci(helm: &Helm, version: &str) -> Result<ResolvedChart> {
    let tmp = TempDir::new().context("cannot create a temporary directory")?;
    let args: Vec<String> = vec![
        "pull".into(),
        OCI_CHART.into(),
        "--version".into(),
        version.into(),
        "--untar".into(),
        "--untardir".into(),
        tmp.path().display().to_string(),
    ];
    let out = helm.run_offline(&args).await?;
    if !out.status.success() {
        bail!(
            "cannot pull chart version {version} from {OCI_CHART}: {}\n\
             (see `paqtra install --list-versions`; a released version must exist and be public)",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let dir = tmp.path().join("paqtra");
    if !has_chart_yaml(&dir) {
        bail!("pulled {OCI_CHART}:{version} but it holds no chart");
    }
    Ok(ResolvedChart {
        path: dir,
        version: version.to_string(),
        origin: format!("{OCI_CHART}:{version}"),
        _tmp: Some(tmp),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_chart_matches_this_binary() {
        // The whole version model rests on this: CLI vX installs chart vX.
        assert_eq!(
            embedded_version(),
            CLI_VERSION,
            "chart/Chart.yaml version must equal Cargo.toml version"
        );
        let app = EMBEDDED
            .get_file("Chart.yaml")
            .and_then(|f| f.contents_utf8())
            .unwrap();
        assert!(app.contains(&format!("appVersion: \"{CLI_VERSION}\"")));
    }

    #[test]
    fn the_embedded_chart_is_complete() {
        for f in [
            "Chart.yaml",
            "values.yaml",
            "templates/_helpers.tpl",
            "templates/rbac.yaml",
        ] {
            assert!(EMBEDDED.get_file(f).is_some(), "missing {f}");
        }
        let v = embedded_values().expect("values.yaml parses");
        assert!(v.get("api").is_some());
    }

    #[test]
    fn extraction_yields_a_helm_chart() {
        let c = extract_embedded().unwrap();
        assert!(c.path.join("Chart.yaml").is_file());
        assert!(c.path.join("templates").is_dir());
        assert_eq!(c.version, CLI_VERSION);
    }

    #[test]
    fn reads_chart_versions() {
        assert_eq!(
            chart_version("name: x\nversion: 2.1.0\n"),
            Some("2.1.0".into())
        );
        assert_eq!(
            chart_version("version: \"1.0.0-rc.1\"\n"),
            Some("1.0.0-rc.1".into())
        );
        assert_eq!(chart_version("appVersion: 3\n"), None);
    }

    #[test]
    fn v_prefix_is_ignored() {
        assert_eq!(normalize("v2.1.0"), "2.1.0");
        assert_eq!(normalize("2.1.0"), "2.1.0");
    }

    fn fake_helm() -> Helm {
        Helm::fake("helm-does-not-exist", (3, 16, 4))
    }

    // PAQTRA_CHART_DIR is process-global; these tests take it in turn.
    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[tokio::test]
    async fn an_explicit_directory_must_hold_a_chart() {
        let _g = ENV.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let err = resolve(Some(dir.path()), None, &fake_helm())
            .await
            .err()
            .unwrap();
        assert!(err.to_string().contains("no Chart.yaml"), "{err}");

        std::fs::write(dir.path().join("Chart.yaml"), "version: 9.9.9\n").unwrap();
        let ok = resolve(Some(dir.path()), None, &fake_helm()).await.unwrap();
        assert_eq!(ok.version, "9.9.9");
        assert!(ok.origin.contains("directory"));
    }

    #[tokio::test]
    async fn an_invalid_chart_dir_env_is_an_error_not_a_silent_fallback() {
        let _g = ENV.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("PAQTRA_CHART_DIR", dir.path());
        let r = resolve(None, None, &fake_helm()).await;
        std::env::remove_var("PAQTRA_CHART_DIR");
        let err = r.err().expect("must fail");
        assert!(err.to_string().contains("PAQTRA_CHART_DIR"), "{err}");
    }

    #[tokio::test]
    async fn the_default_and_the_cli_version_use_the_embedded_chart() {
        let _g = ENV.lock().unwrap();
        std::env::remove_var("PAQTRA_CHART_DIR");
        let a = resolve(None, None, &fake_helm()).await.unwrap();
        let b = resolve(None, Some(&format!("v{CLI_VERSION}")), &fake_helm())
            .await
            .unwrap();
        assert!(a.origin.contains("built into"));
        assert!(b.origin.contains("built into"));
    }

    #[tokio::test]
    async fn another_version_is_pulled_and_a_failed_pull_says_how_to_list_versions() {
        let _g = ENV.lock().unwrap();
        std::env::remove_var("PAQTRA_CHART_DIR");
        // The fake helm binary does not exist, so the pull fails at spawn.
        let err = resolve(None, Some("0.0.1"), &fake_helm())
            .await
            .err()
            .unwrap();
        assert!(!err.to_string().is_empty());
    }
}
