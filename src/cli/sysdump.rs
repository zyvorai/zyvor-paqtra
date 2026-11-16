//! `paqtra sysdump`: one zip with what support needs to diagnose an install,
//! like `cilium sysdump`.
//!
//! Hard boundaries (AGENTS.md): no Secret contents, no application payloads.
//! Kubernetes Secrets are never listed at all; everything collected passes
//! through the redactor (credential-looking keys, URLs with passwords, JWTs).
//! A partial bundle beats none, so a failing collector is recorded in
//! `collection-errors.txt` and the rest carries on.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
use k8s_openapi::api::core::v1::{ConfigMap, Event, Node, PersistentVolumeClaim, Pod, Service};
use kube::api::{DynamicObject, ListParams, LogParams};
use kube::core::GroupVersionKind;
use kube::discovery::ApiResource;
use kube::{Api, Client};
use owo_colors::OwoColorize;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt::Debug;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::time::Duration;

use super::global::Global;
use super::redact::{redact_text, redact_value};
use super::{doctor, helm, status};

#[derive(Debug, Clone)]
pub struct SysdumpOpts {
    pub output: Option<PathBuf>,
    pub since: Duration,
    pub log_lines: i64,
    pub cilium_namespace: String,
    pub no_policies: bool,
}

/// Cilium agent logs are noisy; cap them however many lines were asked for.
const AGENT_LOG_LINES: i64 = 500;

/// Files in memory until the end: a bundle is a few MB, and nothing lands on
/// disk half-written (or unredacted).
pub struct Bundle {
    root: String,
    entries: Vec<(String, Vec<u8>)>,
    errors: Vec<String>,
}

impl Bundle {
    pub fn new(root: &str) -> Self {
        Self {
            root: root.into(),
            entries: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn add(&mut self, path: &str, bytes: impl Into<Vec<u8>>) {
        self.entries
            .push((format!("{}/{path}", self.root), bytes.into()));
    }

    /// A JSON document as redacted YAML.
    pub fn add_value(&mut self, path: &str, mut value: serde_json::Value) {
        redact_value(&mut value);
        match serde_yaml::to_string(&value) {
            Ok(y) => self.add(path, y),
            Err(e) => self.error(path, &e.to_string()),
        }
    }

    pub fn error(&mut self, what: &str, why: &str) {
        self.errors.push(format!("{what}: {why}"));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Write the zip (plus `collection-errors.txt` when something failed).
    pub fn finish<W: Write + std::io::Seek>(mut self, sink: W) -> Result<W> {
        if !self.errors.is_empty() {
            let text = self.errors.join("\n") + "\n";
            self.add("collection-errors.txt", text);
        }
        let mut zip = zip::ZipWriter::new(sink);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (path, bytes) in &self.entries {
            zip.start_file(path, options)?;
            zip.write_all(bytes)?;
        }
        Ok(zip.finish()?)
    }
}

pub fn default_filename(now: DateTime<Utc>) -> String {
    format!("paqtra-sysdump-{}.zip", now.format("%Y%m%d-%H%M%S"))
}

/// `list` a namespaced/cluster resource into redacted YAML.
async fn list_yaml<K>(api: Api<K>, lp: &ListParams) -> Result<serde_json::Value>
where
    K: Clone + DeserializeOwned + Serialize + Debug,
{
    let list = api.list(lp).await?;
    Ok(serde_json::to_value(&list.items)?)
}

/// Record one collector's outcome in the bundle.
fn record(b: &mut Bundle, path: &str, result: Result<serde_json::Value>) {
    match result {
        Ok(v) => b.add_value(path, v),
        Err(e) => b.error(path, &format!("{e:#}")),
    }
}

fn cilium_crd(plural: &str, kind: &str) -> ApiResource {
    ApiResource::from_gvk_with_plural(&GroupVersionKind::gvk("cilium.io", "v2", kind), plural)
}

async fn pod_logs(
    client: &Client,
    ns: &str,
    pod: &Pod,
    lines: i64,
    since: Duration,
    b: &mut Bundle,
    prefix: &str,
) {
    let Some(name) = pod.metadata.name.clone() else {
        return;
    };
    let api: Api<Pod> = Api::namespaced(client.clone(), ns);
    let containers = pod
        .spec
        .as_ref()
        .map(|s| {
            s.containers
                .iter()
                .map(|c| c.name.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for c in containers {
        for previous in [false, true] {
            // Only ask for the previous container's log when it restarted.
            let restarted = pod
                .status
                .as_ref()
                .and_then(|s| s.container_statuses.as_ref())
                .is_some_and(|cs| cs.iter().any(|x| x.name == c && x.restart_count > 0));
            if previous && !restarted {
                continue;
            }
            let lp = LogParams {
                container: Some(c.clone()),
                tail_lines: Some(lines),
                since_seconds: Some(since.as_secs() as i64),
                previous,
                ..Default::default()
            };
            let file = format!(
                "{prefix}/{name}/{c}{}.log",
                if previous { ".previous" } else { "" }
            );
            match api.logs(&name, &lp).await {
                Ok(text) => {
                    let cleaned: String =
                        text.lines().map(redact_text).collect::<Vec<_>>().join("\n");
                    b.add(&file, cleaned + "\n");
                }
                Err(e) => b.error(&file, &e.to_string()),
            }
        }
    }
}

/// Pods in `ns` whose name starts with one of `prefixes` (Cilium's pods carry
/// different labels per component; the names are stable).
fn pods_named<'a>(pods: &'a [Pod], prefixes: &[&str]) -> Vec<&'a Pod> {
    pods.iter()
        .filter(|p| {
            p.metadata
                .name
                .as_deref()
                .is_some_and(|n| prefixes.iter().any(|pre| n.starts_with(pre)))
        })
        .collect()
}

pub async fn cmd_sysdump(g: &Global, o: SysdumpOpts) -> Result<()> {
    let now = Utc::now();
    let out_path = o
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from(default_filename(now)));
    if out_path.exists() {
        bail!(
            "{} already exists; choose another --output-filename",
            out_path.display()
        );
    }
    let client = super::kube::client(g).await?;
    let root = out_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "paqtra-sysdump".into());
    let mut b = Bundle::new(&root);
    println!(
        "{} Collecting a Paqtra sysdump (namespace {}, Cilium in {})...",
        "→".cyan(),
        g.namespace.bold(),
        o.cilium_namespace.bold()
    );

    // ── overview
    let version = client.apiserver_version().await.ok();
    b.add_value(
        "sysdump-info.yaml",
        serde_json::json!({
            "collected_at": now.to_rfc3339(),
            "cli_version": super::chart::CLI_VERSION,
            "namespace": g.namespace,
            "release": g.release,
            "cilium_namespace": o.cilium_namespace,
            "kubernetes": version.map(|v| v.git_version),
            "log_lines": o.log_lines,
            "log_since_seconds": o.since.as_secs(),
            "note": "Secrets are never collected; credentials are redacted. Review before sharing.",
        }),
    );
    println!("  {} doctor report", "·".dimmed());
    let report = doctor::collect(g).await;
    match serde_json::to_value(&report) {
        Ok(v) => b.add_value("doctor.yaml", v),
        Err(e) => b.error("doctor.yaml", &e.to_string()),
    }

    // ── Paqtra namespace
    let ns = &g.namespace;
    let all = ListParams::default();
    println!("  {} Paqtra objects and logs", "·".dimmed());
    record(
        &mut b,
        "paqtra/deployments.yaml",
        list_yaml(Api::<Deployment>::namespaced(client.clone(), ns), &all).await,
    );
    record(
        &mut b,
        "paqtra/daemonsets.yaml",
        list_yaml(Api::<DaemonSet>::namespaced(client.clone(), ns), &all).await,
    );
    record(
        &mut b,
        "paqtra/pods.yaml",
        list_yaml(Api::<Pod>::namespaced(client.clone(), ns), &all).await,
    );
    record(
        &mut b,
        "paqtra/services.yaml",
        list_yaml(Api::<Service>::namespaced(client.clone(), ns), &all).await,
    );
    record(
        &mut b,
        "paqtra/configmaps.yaml",
        list_yaml(Api::<ConfigMap>::namespaced(client.clone(), ns), &all).await,
    );
    record(
        &mut b,
        "paqtra/persistentvolumeclaims.yaml",
        list_yaml(
            Api::<PersistentVolumeClaim>::namespaced(client.clone(), ns),
            &all,
        )
        .await,
    );
    record(
        &mut b,
        "paqtra/events.yaml",
        list_yaml(Api::<Event>::namespaced(client.clone(), ns), &all).await,
    );

    let instance =
        ListParams::default().labels(&format!("app.kubernetes.io/instance={}", g.release));
    match Api::<Pod>::namespaced(client.clone(), ns)
        .list(&instance)
        .await
    {
        Ok(pods) => {
            for p in &pods.items {
                pod_logs(&client, ns, p, o.log_lines, o.since, &mut b, "paqtra/logs").await;
            }
        }
        Err(e) => b.error("paqtra/logs", &e.to_string()),
    }
    if let Some(Ok(body)) = status::api_health_raw(&client, g).await {
        b.add("paqtra/api-health.json", redact_text(&body));
    }

    // ── Helm (values only: `helm get manifest` would include rendered Secrets)
    println!("  {} Helm release", "·".dimmed());
    match helm::find(g).await {
        Some(h) => {
            let get = |args: &[&str]| args.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            let values = get(&[
                "get",
                "values",
                &g.release,
                "--namespace",
                ns,
                "--all",
                "--output",
                "json",
            ]);
            match h.run_ok(&values, "helm get values").await {
                Ok(json) => match serde_json::from_str(&json) {
                    Ok(v) => b.add_value("helm/values.yaml", v),
                    Err(e) => b.error("helm/values.yaml", &e.to_string()),
                },
                Err(e) => b.error("helm/values.yaml", &e.to_string()),
            }
            let history = get(&["history", &g.release, "--namespace", ns, "--output", "json"]);
            match h.run_ok(&history, "helm history").await {
                Ok(json) => match serde_json::from_str(&json) {
                    Ok(v) => b.add_value("helm/history.yaml", v),
                    Err(e) => b.error("helm/history.yaml", &e.to_string()),
                },
                Err(e) => b.error("helm/history.yaml", &e.to_string()),
            }
        }
        None => b.error("helm", "helm not found; release values and history skipped"),
    }

    // ── Cluster and Cilium
    println!("  {} Cilium and Hubble", "·".dimmed());
    let nodes = Api::<Node>::all(client.clone()).list(&all).await.map(|l| {
        l.items
            .into_iter()
            .map(|n| {
                let s = n.status.unwrap_or_default();
                serde_json::json!({
                    "name": n.metadata.name,
                    "kubelet": s.node_info.as_ref().map(|i| &i.kubelet_version),
                    "os": s.node_info.as_ref().map(|i| &i.os_image),
                    "kernel": s.node_info.as_ref().map(|i| &i.kernel_version),
                    "conditions": s.conditions.unwrap_or_default().into_iter().map(|c| serde_json::json!({"type": c.type_, "status": c.status})).collect::<Vec<_>>(),
                    "allocatable": s.allocatable,
                })
            })
            .collect::<Vec<_>>()
    });
    record(
        &mut b,
        "cluster/nodes.yaml",
        nodes.map(serde_json::Value::from).map_err(Into::into),
    );

    let cns = &o.cilium_namespace;
    record(
        &mut b,
        "cilium/cilium-config.yaml",
        async {
            let cm = Api::<ConfigMap>::namespaced(client.clone(), cns)
                .get("cilium-config")
                .await?;
            Ok(serde_json::to_value(&cm.data)?)
        }
        .await,
    );
    record(
        &mut b,
        "cilium/daemonsets.yaml",
        list_yaml(Api::<DaemonSet>::namespaced(client.clone(), cns), &all).await,
    );
    record(
        &mut b,
        "cilium/deployments.yaml",
        list_yaml(Api::<Deployment>::namespaced(client.clone(), cns), &all).await,
    );
    if let Ok(pods) = Api::<Pod>::namespaced(client.clone(), cns).list(&all).await {
        let selected = pods_named(&pods.items, &["cilium", "hubble"]);
        b.add_value(
            "cilium/pods.yaml",
            serde_json::to_value(&selected).unwrap_or_default(),
        );
        for p in selected {
            let name = p.metadata.name.as_deref().unwrap_or("");
            // The agent logs at length; relay and operator are the informative ones.
            let lines = if name.starts_with("cilium-") && !name.starts_with("cilium-operator") {
                o.log_lines.min(AGENT_LOG_LINES)
            } else {
                o.log_lines
            };
            pod_logs(&client, cns, p, lines, o.since, &mut b, "cilium/logs").await;
        }
    }

    if !o.no_policies {
        println!("  {} Cilium policies and nodes", "·".dimmed());
        for (path, plural, kind, cluster) in [
            (
                "cilium/ciliumnetworkpolicies.yaml",
                "ciliumnetworkpolicies",
                "CiliumNetworkPolicy",
                false,
            ),
            (
                "cilium/ciliumclusterwidenetworkpolicies.yaml",
                "ciliumclusterwidenetworkpolicies",
                "CiliumClusterwideNetworkPolicy",
                true,
            ),
            ("cilium/ciliumnodes.yaml", "ciliumnodes", "CiliumNode", true),
        ] {
            let _ = cluster;
            let api: Api<DynamicObject> = Api::all_with(client.clone(), &cilium_crd(plural, kind));
            record(&mut b, path, list_yaml(api, &all).await);
        }
    }

    // ── write
    let file_count = b.len();
    let buf = b.finish(Cursor::new(Vec::new()))?.into_inner();
    write_private(&out_path, &buf)
        .with_context(|| format!("cannot write {}", out_path.display()))?;
    println!(
        "{} Wrote {} ({} files, {:.1} MB)",
        "✔".green(),
        out_path.display().to_string().bold(),
        file_count,
        buf.len() as f64 / 1_048_576.0
    );
    println!(
        "  Secrets were not collected and credentials are redacted, but the bundle still holds"
    );
    println!("  pod names, IPs and policy specs: review it before sharing.");
    Ok(())
}

/// The bundle can hold cluster topology: owner-only permissions.
fn write_private(path: &PathBuf, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(bytes)
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::io::Read;

    fn read_zip(bytes: Vec<u8>) -> Vec<(String, String)> {
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        (0..z.len())
            .map(|i| {
                let mut f = z.by_index(i).unwrap();
                let mut s = String::new();
                f.read_to_string(&mut s).unwrap();
                (f.name().expect("zip entry name").into_owned(), s)
            })
            .collect()
    }

    #[test]
    fn a_bundle_round_trips_through_a_zip_under_one_root_directory() {
        let mut b = Bundle::new("dump");
        b.add("a.txt", "hello");
        b.add_value("b.yaml", serde_json::json!({"x": 1}));
        let files = read_zip(b.finish(Cursor::new(Vec::new())).unwrap().into_inner());
        assert_eq!(files.len(), 2);
        assert_eq!(files[0], ("dump/a.txt".to_string(), "hello".to_string()));
        assert_eq!(files[1].0, "dump/b.yaml");
        assert!(files[1].1.contains("x: 1"));
    }

    #[test]
    fn collection_failures_are_recorded_not_fatal() {
        let mut b = Bundle::new("dump");
        b.add("ok.txt", "fine");
        record(
            &mut b,
            "paqtra/pods.yaml",
            Err(anyhow::anyhow!("forbidden")),
        );
        let files = read_zip(b.finish(Cursor::new(Vec::new())).unwrap().into_inner());
        let errors = files
            .iter()
            .find(|(n, _)| n.ends_with("collection-errors.txt"))
            .expect("errors file");
        assert!(errors.1.contains("paqtra/pods.yaml: forbidden"));
        assert!(
            files.iter().any(|(n, _)| n == "dump/ok.txt"),
            "the rest is still there"
        );
    }

    #[test]
    fn no_errors_means_no_errors_file() {
        let mut b = Bundle::new("d");
        b.add("a", "x");
        let files = read_zip(b.finish(Cursor::new(Vec::new())).unwrap().into_inner());
        assert!(!files.iter().any(|(n, _)| n.contains("collection-errors")));
    }

    #[test]
    fn credentials_never_reach_the_bundle() {
        let mut b = Bundle::new("d");
        let pw = ["hun", "ter2"].concat();
        b.add_value(
            "paqtra/pods.yaml",
            serde_json::json!([{
                "metadata": {"name": "api", "managedFields": [{"manager": "helm"}]},
                "spec": {"containers": [{"env": [
                    {"name": "JWT_SECRET", "value": "s3cr3t-value"},
                    {"name": "PROMETHEUS_URL", "value": format!("http://u:{pw}@prom:9090")},
                    {"name": "HUBBLE_ADDRESS", "value": "relay:80"},
                ]}]}
            }]),
        );
        let files = read_zip(b.finish(Cursor::new(Vec::new())).unwrap().into_inner());
        let body = &files[0].1;
        for leaked in ["s3cr3t-value", pw.as_str(), "managedFields"] {
            assert!(!body.contains(leaked), "{leaked} leaked:\n{body}");
        }
        assert!(
            body.contains("relay:80") && body.contains("HUBBLE_ADDRESS"),
            "harmless values survive:\n{body}"
        );
    }

    #[test]
    fn filenames_are_timestamped() {
        let t = DateTime::parse_from_rfc3339("2026-09-25T13:04:05Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(default_filename(t), "paqtra-sysdump-20260925-130405.zip");
    }

    fn named(n: &str) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(n.into()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn cilium_pods_are_picked_by_name() {
        let pods = vec![
            named("cilium-abc"),
            named("cilium-operator-x"),
            named("hubble-relay-1"),
            named("coredns-1"),
            named("metrics-server"),
        ];
        let got: Vec<_> = pods_named(&pods, &["cilium", "hubble"])
            .iter()
            .map(|p| p.metadata.name.clone().unwrap())
            .collect();
        assert_eq!(got, ["cilium-abc", "cilium-operator-x", "hubble-relay-1"]);
    }

    #[test]
    fn an_existing_file_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.zip");
        write_private(&path, b"one").unwrap();
        assert!(
            write_private(&path, b"two").is_err(),
            "create_new refuses to clobber"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"one");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    /// The boundary that matters most: this module must never read Secrets.
    #[test]
    fn kubernetes_secrets_are_never_listed() {
        const SRC: &str = include_str!("sysdump.rs");
        // Built from pieces so this test does not trip itself.
        let forbidden = [
            concat!("core::v1::", "Secret"),
            concat!("Api::<", "Secret>"),
            concat!("\"secrets", "\""),
        ];
        for f in forbidden {
            let hits = SRC.matches(f).count();
            // The only occurrence is the forbidden list above.
            assert!(hits <= 1, "sysdump.rs references {f} {hits} times");
        }
    }
}
