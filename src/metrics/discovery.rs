//! Pods on this node, from the Kubernetes API: maps pod cgroups to
//! namespace/pod/workload and finds application endpoints from pod
//! annotations. Reads pod metadata only; never Secrets.

use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, ListParams};
use paqtra_metrics::collectors::{discover_pod_cgroups, AppConfig, AppKind, Workload};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PodMeta {
    pub namespace: String,
    pub name: String,
    pub ip: String,
    pub workload_kind: String,
    pub workload_name: String,
    pub annotations: BTreeMap<String, String>,
}

/// Pod UID to metadata, refreshed by `spawn_pod_watch`.
pub type PodIndex = Arc<RwLock<HashMap<String, PodMeta>>>;

/// The owning workload: a ReplicaSet's pod hash is stripped to name the
/// Deployment.
fn owner(pod: &Pod) -> (String, String) {
    let Some(o) = pod.metadata.owner_references.as_ref().and_then(|os| {
        os.iter()
            .find(|o| o.controller == Some(true))
            .or(os.first())
    }) else {
        return ("Pod".into(), pod.metadata.name.clone().unwrap_or_default());
    };
    if o.kind == "ReplicaSet" {
        if let Some((base, hash)) = o.name.rsplit_once('-') {
            if !hash.is_empty() && hash.chars().all(|c| c.is_ascii_alphanumeric()) {
                return ("Deployment".into(), base.into());
            }
        }
    }
    if o.kind == "Job" {
        if let Some((base, ts)) = o.name.rsplit_once('-') {
            if ts.len() >= 8 && ts.chars().all(|c| c.is_ascii_digit()) {
                return ("CronJob".into(), base.into());
            }
        }
    }
    (o.kind.clone(), o.name.clone())
}

pub(crate) fn pod_meta(pod: &Pod) -> Option<(String, PodMeta)> {
    let uid = pod.metadata.uid.clone()?;
    let (workload_kind, workload_name) = owner(pod);
    let host_network = pod
        .spec
        .as_ref()
        .and_then(|s| s.host_network)
        .unwrap_or(false);
    let ip = pod
        .status
        .as_ref()
        .and_then(|s| s.pod_ip.clone())
        .filter(|_| !host_network)
        .unwrap_or_default();
    Some((
        uid,
        PodMeta {
            namespace: pod.metadata.namespace.clone().unwrap_or_default(),
            name: pod.metadata.name.clone().unwrap_or_default(),
            ip,
            workload_kind,
            workload_name,
            annotations: pod.metadata.annotations.clone().unwrap_or_default(),
        },
    ))
}

/// Lists this node's pods every 30 s. Failures keep the previous index.
pub fn spawn_pod_watch(node: String) -> PodIndex {
    let idx: PodIndex = Arc::new(RwLock::new(HashMap::new()));
    let out = idx.clone();
    tokio::spawn(async move {
        let client = loop {
            match kube::Client::try_default().await {
                Ok(c) => break c,
                Err(e) => {
                    tracing::debug!("metrics pod index: no kube client: {e}");
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
            }
        };
        let pods: Api<Pod> = Api::all(client);
        let lp = ListParams::default().fields(&format!("spec.nodeName={node}"));
        loop {
            match pods.list(&lp).await {
                Ok(list) => {
                    let m: HashMap<String, PodMeta> =
                        list.items.iter().filter_map(pod_meta).collect();
                    *idx.write().unwrap() = m;
                }
                Err(e) => tracing::debug!("metrics pod index: {e}"),
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
    out
}

/// Pod and container cgroups under `root` for pods in the index.
pub(crate) fn workloads(root: &Path, idx: &PodIndex) -> Vec<Workload> {
    let idx = idx.read().unwrap();
    let mut out = Vec::new();
    for pc in discover_pod_cgroups(root) {
        let Some(m) = idx.get(&pc.uid) else { continue };
        let base = Workload {
            namespace: m.namespace.clone(),
            pod: m.name.clone(),
            container: String::new(),
            workload_kind: m.workload_kind.clone(),
            workload_name: m.workload_name.clone(),
            cgroup_path: pc.path.clone(),
        };
        for (id, path) in &pc.containers {
            out.push(Workload {
                container: id.clone(),
                cgroup_path: path.clone(),
                ..base.clone()
            });
        }
        out.push(base);
    }
    out
}

fn default_target(kind: AppKind) -> (u16, &'static str) {
    match kind {
        AppKind::Nginx => (80, "/stub_status"),
        AppKind::Apache => (80, "/server-status?auto"),
        AppKind::Haproxy => (8404, "/stats;csv"),
        AppKind::Redis => (6379, ""),
        AppKind::Memcached => (11211, ""),
        AppKind::Envoy => (9901, "/stats/prometheus"),
        AppKind::Coredns => (9153, "/metrics"),
        AppKind::Etcd => (2379, "/metrics"),
        AppKind::Prometheus => (9090, "/metrics"),
    }
}

fn host_port(ip: &str, port: u16) -> String {
    if ip.contains(':') {
        format!("[{ip}]:{port}")
    } else {
        format!("{ip}:{port}")
    }
}

/// One app per annotated pod with an IP. `paqtra.io/app-kind` (with optional
/// `paqtra.io/app-port`, `paqtra.io/app-path`, `paqtra.io/app-scheme`) wins
/// over `prometheus.io/scrape: "true"` (`prometheus.io/port`, `path`,
/// `scheme`).
pub(crate) fn app_for(m: &PodMeta) -> Option<AppConfig> {
    if m.ip.is_empty() {
        return None;
    }
    let a = |k: &str| {
        m.annotations
            .get(k)
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
    };
    let (kind, port, path, scheme) = if let Some(k) = a("paqtra.io/app-kind") {
        let kind = AppKind::parse(k)?;
        let (dp, dpath) = default_target(kind);
        let port = a("paqtra.io/app-port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(dp);
        (
            kind,
            port,
            a("paqtra.io/app-path").unwrap_or(dpath).to_string(),
            a("paqtra.io/app-scheme").unwrap_or("http"),
        )
    } else if a("prometheus.io/scrape") == Some("true") {
        let port = a("prometheus.io/port")?.parse().ok()?;
        (
            AppKind::Prometheus,
            port,
            a("prometheus.io/path").unwrap_or("/metrics").to_string(),
            a("prometheus.io/scheme").unwrap_or("http"),
        )
    } else {
        return None;
    };
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let hp = host_port(&m.ip, port);
    let target = match kind {
        AppKind::Redis | AppKind::Memcached => hp,
        _ => {
            let path = if path.starts_with('/') {
                path
            } else {
                format!("/{path}")
            };
            format!("{scheme}://{hp}{path}")
        }
    };
    let mut c = AppConfig::new(kind, &format!("{}_{}", m.namespace, m.name), &target);
    c.labels.insert("k8s_namespace".into(), m.namespace.clone());
    c.labels.insert("k8s_pod".into(), m.name.clone());
    if !m.workload_name.is_empty() {
        c.labels
            .insert("k8s_workload".into(), m.workload_name.clone());
    }
    c.discovered = true;
    c.normalize().ok()?;
    Some(c)
}

pub(crate) fn apps(idx: &PodIndex) -> Vec<AppConfig> {
    let mut out: Vec<AppConfig> = idx.read().unwrap().values().filter_map(app_for).collect();
    out.sort_by_key(|c| c.id());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

    fn pod(owner_kind: &str, owner_name: &str, ann: &[(&str, &str)]) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some("web-7d9f8-abcde".into()),
                namespace: Some("shop".into()),
                uid: Some("0b1e2c3d-0000-4000-8000-000000000001".into()),
                owner_references: Some(vec![OwnerReference {
                    kind: owner_kind.into(),
                    name: owner_name.into(),
                    controller: Some(true),
                    ..Default::default()
                }]),
                annotations: Some(
                    ann.iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                ),
                ..Default::default()
            },
            status: Some(k8s_openapi::api::core::v1::PodStatus {
                pod_ip: Some("10.0.1.5".into()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn owners_resolve_to_workloads() {
        let (_, m) = pod_meta(&pod("ReplicaSet", "web-7d9f8", &[])).unwrap();
        assert_eq!(
            (m.workload_kind.as_str(), m.workload_name.as_str()),
            ("Deployment", "web")
        );
        let (_, m) = pod_meta(&pod("Job", "backup-28800000", &[])).unwrap();
        assert_eq!(m.workload_kind, "CronJob");
        let (_, m) = pod_meta(&pod("StatefulSet", "db", &[])).unwrap();
        assert_eq!(m.workload_name, "db");
    }

    #[test]
    fn annotations_become_apps() {
        let (_, m) = pod_meta(&pod(
            "ReplicaSet",
            "web-7d9f8",
            &[("paqtra.io/app-kind", "redis")],
        ))
        .unwrap();
        let c = app_for(&m).unwrap();
        assert_eq!(c.address, "10.0.1.5:6379");
        assert_eq!(c.labels["k8s_workload"], "web");

        let (_, m) = pod_meta(&pod(
            "ReplicaSet",
            "web-7d9f8",
            &[
                ("prometheus.io/scrape", "true"),
                ("prometheus.io/port", "8080"),
                ("prometheus.io/path", "stats"),
            ],
        ))
        .unwrap();
        assert_eq!(app_for(&m).unwrap().url, "http://10.0.1.5:8080/stats");

        let (_, m) = pod_meta(&pod(
            "ReplicaSet",
            "web-7d9f8",
            &[
                ("paqtra.io/app-kind", "nginx"),
                ("paqtra.io/app-port", "8081"),
            ],
        ))
        .unwrap();
        assert_eq!(app_for(&m).unwrap().url, "http://10.0.1.5:8081/stub_status");

        let (_, m) = pod_meta(&pod(
            "ReplicaSet",
            "web-7d9f8",
            &[("prometheus.io/scrape", "true")],
        ))
        .unwrap();
        assert!(app_for(&m).is_none(), "port is required");
        let (_, m) = pod_meta(&pod(
            "ReplicaSet",
            "web-7d9f8",
            &[("paqtra.io/app-kind", "bogus")],
        ))
        .unwrap();
        assert!(app_for(&m).is_none());
    }

    #[test]
    fn workloads_join_cgroups_to_pods() {
        let d = tempfile::tempdir().unwrap();
        let uid = "0b1e2c3d-0000-4000-8000-000000000001";
        let ctr = "a".repeat(64);
        let pod_dir = d
            .path()
            .join("kubepods.slice/kubepods-burstable.slice")
            .join(format!(
                "kubepods-burstable-pod{}.slice",
                uid.replace('-', "_")
            ));
        std::fs::create_dir_all(pod_dir.join(format!("cri-containerd-{ctr}.scope"))).unwrap();
        let idx: PodIndex = Arc::new(RwLock::new(HashMap::new()));
        assert!(
            workloads(d.path(), &idx).is_empty(),
            "unknown pods are skipped"
        );
        let (u, m) = pod_meta(&pod("ReplicaSet", "web-7d9f8", &[])).unwrap();
        idx.write().unwrap().insert(u, m);
        let ws = workloads(d.path(), &idx);
        assert_eq!(ws.len(), 2);
        assert_eq!(ws[1].pod, "web-7d9f8-abcde");
        assert_eq!(ws[0].container, ctr);
    }
}
