// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Running Helm. cilium-cli embeds the Helm SDK; Rust has none, so Paqtra runs
//! the `helm` binary, and (like a package manager bootstrapping its tool)
//! downloads a pinned, checksum-verified copy when none is installed.
//!
//! Resolution order: `--helm-path`/`PAQTRA_HELM` > `helm` on PATH (>= 3.8, the
//! first version with OCI charts) > a previous download in `~/.paqtra/bin` >
//! a fresh download (unless `--no-download-helm`).

use anyhow::{bail, Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use tokio::process::Command;

use super::global::Global;

/// The helm we download when none is installed.
pub const PINNED_HELM: &str = "3.16.4";
/// Oldest helm that can pull OCI charts without an experimental flag.
pub const MIN_HELM: (u32, u32, u32) = (3, 8, 0);

/// sha256 of `helm-v<PINNED_HELM>-<platform>.tar.gz`, from get.helm.sh's
/// published `.sha256sum` files. Pinned here so a compromised mirror cannot
/// swap the binary.
const HELM_SHA256: &[(&str, &str)] = &[
    (
        "linux-amd64",
        "fc307327959aa38ed8f9f7e66d45492bb022a66c3e5da6063958254b9767d179",
    ),
    (
        "linux-arm64",
        "d3f8f15b3d9ec8c8678fbf3280c3e5902efabe5912e2f9fcf29107efbc8ead69",
    ),
    (
        "darwin-amd64",
        "8dc25671120a4af197afe7ad9041fb8e1dd71bc01e5ef73dba1139cbc9e9f44b",
    ),
    (
        "darwin-arm64",
        "e2442d8f05d53d84c39b869bc5fe5affad247ee2f4c706a040919c146edb1f94",
    ),
];

pub type Version = (u32, u32, u32);

#[derive(Debug, Clone)]
pub struct Helm {
    path: PathBuf,
    pub version: Version,
    context: Option<String>,
    kubeconfig: Option<PathBuf>,
}

/// What this helm can do, so argument building stays a pure function.
#[derive(Debug, Clone, Copy, Default)]
pub struct Caps {
    /// `--dry-run=server` (validates against the cluster): helm 3.13+.
    pub dry_run_server: bool,
    /// `--reset-then-reuse-values` (new chart defaults + last user values): 3.14+.
    pub reset_then_reuse: bool,
}

impl Helm {
    pub fn supports_dry_run_server(&self) -> bool {
        self.version >= (3, 13, 0)
    }

    pub fn caps(&self) -> Caps {
        Caps {
            dry_run_server: self.supports_dry_run_server(),
            reset_then_reuse: self.version >= (3, 14, 0),
        }
    }

    #[cfg(test)]
    pub(crate) fn fake(path: &str, version: Version) -> Self {
        Self {
            path: path.into(),
            version,
            context: None,
            kubeconfig: None,
        }
    }

    /// Append `--kube-context` / `--kubeconfig` so helm talks to the same cluster
    /// as the rest of the CLI. Commands that need no cluster (`pull`) skip it.
    fn with_kube_args(&self, args: &[String]) -> Vec<String> {
        let mut out = args.to_vec();
        if let Some(c) = &self.context {
            out.push("--kube-context".into());
            out.push(c.clone());
        }
        if let Some(k) = &self.kubeconfig {
            out.push("--kubeconfig".into());
            out.push(k.display().to_string());
        }
        out
    }

    pub async fn run(&self, args: &[String]) -> Result<std::process::Output> {
        Command::new(&self.path)
            .args(self.with_kube_args(args))
            .output()
            .await
            .with_context(|| format!("failed to run {}", self.path.display()))
    }

    /// Run a command that needs no cluster (e.g. `helm pull`).
    pub async fn run_offline(&self, args: &[String]) -> Result<std::process::Output> {
        Command::new(&self.path)
            .args(args)
            .output()
            .await
            .with_context(|| format!("failed to run {}", self.path.display()))
    }

    /// Run and return stdout, or an error carrying helm's own message.
    pub async fn run_ok(&self, args: &[String], action: &str) -> Result<String> {
        let out = self.run(args).await?;
        ensure_ok(&out, action)?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

pub fn ensure_ok(output: &std::process::Output, action: &str) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    bail!("{action} failed:\n{stdout}{stderr}");
}

/// `v3.16.4`, `3.16.4+gabc`, `v3.17.0-rc.1` -> (3, 16, 4) / (3, 17, 0).
pub fn parse_version(s: &str) -> Option<Version> {
    let s = s.trim().trim_start_matches('v');
    let core = s.split(['+', '-']).next()?;
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// `linux-amd64`, `darwin-arm64`, ... the way helm names its release archives.
pub fn platform() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("linux-amd64"),
        ("linux", "aarch64") => Ok("linux-arm64"),
        ("macos", "x86_64") => Ok("darwin-amd64"),
        ("macos", "aarch64") => Ok("darwin-arm64"),
        (os, arch) => bail!("no helm download for {os}/{arch}; install helm and re-run"),
    }
}

pub fn helm_url(platform: &str) -> String {
    format!("https://get.helm.sh/helm-v{PINNED_HELM}-{platform}.tar.gz")
}

fn expected_sha256(platform: &str) -> Option<&'static str> {
    HELM_SHA256
        .iter()
        .find(|(p, _)| *p == platform)
        .map(|(_, h)| *h)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Pull one file out of a `.tar.gz` (helm ships `<platform>/helm`).
pub fn extract_from_targz(targz: &[u8], member: &str) -> Result<Vec<u8>> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(targz));
    for entry in archive.entries().context("not a valid tar archive")? {
        let mut entry = entry?;
        if entry.path()?.to_string_lossy() == member {
            let mut out = Vec::new();
            entry.read_to_end(&mut out)?;
            return Ok(out);
        }
    }
    bail!("{member} not found in the archive")
}

fn install_dir() -> Result<PathBuf> {
    let home =
        std::env::var_os("HOME").context("HOME is not set; cannot choose where to put helm")?;
    Ok(PathBuf::from(home)
        .join(".paqtra")
        .join("bin")
        .join(format!("helm-{PINNED_HELM}")))
}

async fn version_of(path: &Path) -> Result<Version> {
    let out = Command::new(path)
        .args(["version", "--template", "{{.Version}}"])
        .output()
        .await
        .with_context(|| format!("cannot run {}", path.display()))?;
    if !out.status.success() {
        bail!(
            "{} version failed: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_version(&text).with_context(|| format!("cannot parse helm version {text:?}"))
}

/// Download, verify and unpack the pinned helm. Returns the binary's path.
async fn download_helm() -> Result<PathBuf> {
    let platform = platform()?;
    let want = expected_sha256(platform).context("no pinned checksum for this platform")?;
    let url = helm_url(platform);
    eprintln!(
        "→ helm not found; downloading helm v{PINNED_HELM} ({platform}) from get.helm.sh ..."
    );

    let bytes = tokio::task::spawn_blocking({
        let url = url.clone();
        move || -> Result<Vec<u8>> {
            let mut resp = ureq::get(&url)
                .call()
                .with_context(|| format!("download failed: {url}"))?;
            resp.body_mut()
                .with_config()
                .limit(128 * 1024 * 1024)
                .read_to_vec()
                .context("download interrupted")
        }
    })
    .await??;

    let got = sha256_hex(&bytes);
    if got != want {
        bail!("checksum mismatch for {url}: expected {want}, got {got}; not installing it");
    }
    let bin = extract_from_targz(&bytes, &format!("{platform}/helm"))?;

    let dir = install_dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let dest = dir.join("helm");
    // Write beside, then rename: a concurrent run never sees a half-written binary.
    let tmp = dir.join(format!("helm.{}.tmp", std::process::id()));
    std::fs::write(&tmp, &bin)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, &dest)?;
    eprintln!("✔ installed helm v{PINNED_HELM} to {}", dest.display());
    Ok(dest)
}

const NO_HELM_HINT: &str =
    "helm 3.8 or newer is required. Install it (https://helm.sh/docs/intro/install/) \
    or drop --no-download-helm / PAQTRA_NO_DOWNLOAD to let paqtra download a pinned copy";

/// A helm that is already available (explicit, PATH, or a previous download);
/// never downloads. `Ok(None)` when there is none.
async fn discover(g: &Global) -> Result<Option<Helm>> {
    let make = |path: PathBuf, version: Version| Helm {
        path,
        version,
        context: g.context.clone(),
        kubeconfig: g.kubeconfig.clone(),
    };

    if let Some(explicit) = &g.helm_path {
        let v = version_of(explicit).await?;
        if v < MIN_HELM {
            bail!(
                "{} is helm {}.{}.{}; need >= 3.8",
                explicit.display(),
                v.0,
                v.1,
                v.2
            );
        }
        return Ok(Some(make(explicit.clone(), v)));
    }

    if let Some(path) = find_in_path("helm") {
        match version_of(&path).await {
            Ok(v) if v >= MIN_HELM => return Ok(Some(make(path, v))),
            Ok(v) => eprintln!(
                "⚠ helm {}.{}.{} at {} is older than 3.8 (no OCI charts); looking for another",
                v.0,
                v.1,
                v.2,
                path.display()
            ),
            Err(e) => eprintln!("⚠ ignoring {}: {e}", path.display()),
        }
    }

    let cached = install_dir()?.join("helm");
    if cached.is_file() {
        if let Ok(v) = version_of(&cached).await {
            return Ok(Some(make(cached, v)));
        }
    }
    Ok(None)
}

/// Like [`ensure`] but for read-only commands: no download, `None` if absent.
pub async fn find(g: &Global) -> Option<Helm> {
    discover(g).await.ok().flatten()
}

/// Find or fetch a usable helm.
pub async fn ensure(g: &Global) -> Result<Helm> {
    if let Some(h) = discover(g).await? {
        return Ok(h);
    }
    if g.no_download_helm {
        bail!("{NO_HELM_HINT}");
    }
    let path = download_helm().await?;
    let version = version_of(&path).await?;
    Ok(Helm {
        path,
        version,
        context: g.context.clone(),
        kubeconfig: g.kubeconfig.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_helm_versions() {
        assert_eq!(parse_version("v3.16.4"), Some((3, 16, 4)));
        assert_eq!(parse_version("v3.16.4+g7877b45"), Some((3, 16, 4)));
        assert_eq!(parse_version("3.17.0-rc.1"), Some((3, 17, 0)));
        assert_eq!(parse_version(" v3.8\n"), Some((3, 8, 0)));
        assert_eq!(parse_version("nonsense"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn version_ordering_gates_features() {
        assert!(parse_version("v3.7.2").unwrap() < MIN_HELM);
        assert!(parse_version("v3.8.0").unwrap() >= MIN_HELM);
        let h = |v| Helm {
            path: "helm".into(),
            version: v,
            context: None,
            kubeconfig: None,
        };
        assert!(!h((3, 12, 9)).supports_dry_run_server());
        assert!(h((3, 13, 0)).supports_dry_run_server());
        assert!(h((3, 16, 4)).supports_dry_run_server());
    }

    #[test]
    fn every_supported_platform_has_a_pinned_checksum() {
        for p in ["linux-amd64", "linux-arm64", "darwin-amd64", "darwin-arm64"] {
            let h = expected_sha256(p).unwrap_or_else(|| panic!("no checksum for {p}"));
            assert_eq!(h.len(), 64, "{p}");
            assert!(h.bytes().all(|b| b.is_ascii_hexdigit()), "{p}");
        }
        assert!(expected_sha256("windows-amd64").is_none());
        assert_eq!(
            helm_url("linux-arm64"),
            "https://get.helm.sh/helm-v3.16.4-linux-arm64.tar.gz"
        );
    }

    #[test]
    fn sha256_matches_the_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    fn targz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut b = tar::Builder::new(gz);
        for (name, data) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, name, *data).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn extracts_the_helm_binary_from_the_archive() {
        let archive = targz(&[
            ("linux-amd64/LICENSE", b"license"),
            ("linux-amd64/helm", b"#!binary"),
        ]);
        assert_eq!(
            extract_from_targz(&archive, "linux-amd64/helm").unwrap(),
            b"#!binary"
        );
        assert!(extract_from_targz(&archive, "linux-amd64/nope").is_err());
        assert!(extract_from_targz(b"not gzip", "x").is_err());
    }

    #[test]
    fn kube_args_follow_the_global_flags() {
        let h = Helm {
            path: "helm".into(),
            version: (3, 16, 4),
            context: Some("prod".into()),
            kubeconfig: Some("/tmp/kc".into()),
        };
        let out = h.with_kube_args(&["list".into(), "-A".into()]);
        assert_eq!(
            out,
            [
                "list",
                "-A",
                "--kube-context",
                "prod",
                "--kubeconfig",
                "/tmp/kc"
            ]
        );
        let plain = Helm {
            context: None,
            kubeconfig: None,
            ..h
        };
        assert_eq!(plain.with_kube_args(&["list".into()]), ["list"]);
    }
}
