//! Per-second metrics platform, API side: the ingest hub holding one store per
//! node (fed by the agents), a cluster store of series derived from Hubble
//! flows, anomaly scoring, metric alert rules wired to the notifier and the
//! remote write / OTLP / Graphite exporters.
//!
//! Everything here is read-only towards the cluster: a metric alert notifies,
//! it never applies policy.

use crate::models::flow::Flow;
use crate::services::notifier::{self, AlertEvent, EventKind};
use crate::AppState;
use paqtra_metrics::anomaly::{Detector, Options as DetectorOptions};
use paqtra_metrics::export::{
    graphite_lines, otlp_bodies, otlp_url, parse_headers, remote_write_bodies, ExportSeries,
    Exporter, Options as ExportOptions, Status as ExportStatus, REMOTE_WRITE_HEADERS,
};
use paqtra_metrics::metricalert::{
    load_rules, Engine, Event as AlertTransition, Options as AlertOptions, Status as AlertStatus,
};
use paqtra_metrics::stream::{Hub, HubOptions};
use paqtra_metrics::tsdb::{Db, Options as DbOptions, Sample, Series, Source};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Node name of the store holding series derived from Hubble flows.
pub const HUBBLE_NODE: &str = "hubble";

fn env(k: &str) -> Option<String> {
    std::env::var(k)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env_bool(k: &str, default: bool) -> bool {
    env(k).map_or(default, |v| {
        matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on")
    })
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Exporter destination.
pub enum SinkKind {
    RemoteWrite {
        url: String,
        headers: Vec<(String, String)>,
    },
    Otlp {
        url: String,
        headers: Vec<(String, String)>,
        resource: Vec<(String, String)>,
    },
    Graphite {
        addr: String,
    },
}

pub struct ExportRuntime {
    pub exporter: Exporter,
    pub sink: SinkKind,
    pub prefix: String,
}

pub struct MetricsPlatform {
    pub hub: Arc<Hub>,
    pub cluster: Arc<Db>,
    pub detector: Arc<Detector>,
    pub alerts: Option<Arc<Engine>>,
    pub alerts_error: Option<String>,
    pub exporters: Vec<Arc<ExportRuntime>>,
    /// Required in `X-Paqtra-Agent-Key` on ingest. Unset only works with
    /// auth disabled.
    pub agent_key: Option<String>,
    hubble: Mutex<HubbleAggregator>,
    events: Mutex<Option<tokio::sync::mpsc::Receiver<AlertTransition>>>,
}

impl MetricsPlatform {
    /// Builds the platform from `PAQTRA_METRICS_*` / `PAQTRA_METRICALERT_*`
    /// environment variables. `data_dir` enables on-disk rollup tiers.
    pub fn from_env(data_dir: Option<&str>) -> anyhow::Result<Self> {
        let dir = env("PAQTRA_METRICS_DIR")
            .map(PathBuf::from)
            .or_else(|| data_dir.map(|d| PathBuf::from(d).join("metrics")));
        let secs =
            |k: &str, d: u64| Duration::from_secs(env(k).and_then(|v| v.parse().ok()).unwrap_or(d));
        let db = DbOptions {
            tier0_retention: secs("PAQTRA_METRICS_TIER0_SECONDS", 3600),
            tier1_retention: secs("PAQTRA_METRICS_TIER1_SECONDS", 14 * 86400),
            tier2_retention: secs("PAQTRA_METRICS_TIER2_SECONDS", 365 * 86400),
            disk_quota_bytes: env("PAQTRA_METRICS_DISK_QUOTA_MB")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(1024)
                << 20,
            ..Default::default()
        };
        let hub = Arc::new(
            Hub::open(HubOptions {
                dir: dir.clone(),
                db: db.clone(),
                max_nodes: 0,
            })
            .map_err(|e| anyhow::anyhow!(e))?,
        );
        let mut cluster_opts = db.clone();
        cluster_opts.dir = dir.as_ref().map(|d| d.join("_cluster"));
        let cluster = Arc::new(Db::open(cluster_opts).map_err(|e| anyhow::anyhow!(e.to_string()))?);
        let detector = Arc::new(Detector::new(DetectorOptions::default()));

        let (tx, rx) = tokio::sync::mpsc::channel::<AlertTransition>(1024);
        let (mut alerts, mut alerts_error) = (None, None);
        if env_bool("PAQTRA_METRICALERT", true) {
            let rule_dir = env("PAQTRA_METRICALERT_DIR").map(PathBuf::from);
            let dirs: Vec<&std::path::Path> = rule_dir.iter().map(|p| p.as_path()).collect();
            let built = load_rules(env_bool("PAQTRA_METRICALERT_DEFAULTS", true), &dirs).and_then(
                |rules| {
                    let (h, c) = (hub.clone(), cluster.clone());
                    Engine::new(AlertOptions {
                        rules,
                        sources: Arc::new(move || all_sources(&h, &c)),
                        publish: Some(Arc::new(move |ev| {
                            if tx.try_send(ev).is_err() {
                                tracing::warn!("metric alert notification dropped (queue full)");
                            }
                        })),
                        silence_file: dir.as_ref().map(|d| d.join("metricalert-silences.json")),
                        history_size: 0,
                        max_alerts: 0,
                    })
                },
            );
            match built {
                Ok(e) => alerts = Some(Arc::new(e)),
                Err(e) => {
                    tracing::warn!("metric alerts disabled: {e}");
                    alerts_error = Some(e);
                }
            }
        }

        Ok(MetricsPlatform {
            hub,
            cluster,
            detector,
            alerts,
            alerts_error,
            exporters: exporters_from_env(),
            agent_key: env("PAQTRA_AGENT_KEY"),
            hubble: Mutex::new(HubbleAggregator::default()),
            events: Mutex::new(Some(rx)),
        })
    }

    /// Every node store plus the Hubble-derived cluster store.
    pub fn sources(&self) -> Vec<Source> {
        all_sources(&self.hub, &self.cluster)
    }

    /// Counts flows into the Hubble-derived series.
    pub fn observe_flows(&self, flows: &[Flow]) {
        let mut h = self.hubble.lock().unwrap();
        for f in flows {
            h.observe(f);
        }
    }

    pub fn hubble_stats(&self) -> serde_json::Value {
        let h = self.hubble.lock().unwrap();
        serde_json::json!({ "flowsObserved": h.observed, "nodes": h.flows.len(), "namespaces": h.http.len().max(h.dns.len()) })
    }
}

fn all_sources(hub: &Hub, cluster: &Arc<Db>) -> Vec<Source> {
    let mut s = hub.sources();
    if !cluster.list().is_empty() {
        s.push(Source {
            node: HUBBLE_NODE.into(),
            db: cluster.clone(),
        });
    }
    s
}

fn exporters_from_env() -> Vec<Arc<ExportRuntime>> {
    let list = |k: &str| {
        env(k)
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    let opts = ExportOptions {
        resolution: env("PAQTRA_METRICS_EXPORT_RESOLUTION")
            .and_then(|v| v.parse().ok())
            .unwrap_or(10),
        contexts: list("PAQTRA_METRICS_EXPORT_CONTEXTS"),
        exclude: list("PAQTRA_METRICS_EXPORT_EXCLUDE"),
        ..Default::default()
    };
    let prefix = env("PAQTRA_METRICS_EXPORT_PREFIX").unwrap_or_else(|| "paqtra".into());
    let mut out = Vec::new();
    let mut add = |name: &str, sink: SinkKind| {
        out.push(Arc::new(ExportRuntime {
            exporter: Exporter::new(name, opts.clone()),
            sink,
            prefix: prefix.clone(),
        }));
    };
    if let Some(url) = env("PAQTRA_METRICS_REMOTE_WRITE_URL") {
        let headers =
            parse_headers(&env("PAQTRA_METRICS_REMOTE_WRITE_HEADERS").unwrap_or_default());
        add(
            "prometheus-remote-write",
            SinkKind::RemoteWrite { url, headers },
        );
    }
    if let Some(ep) = env("PAQTRA_METRICS_OTLP_ENDPOINT") {
        let headers = parse_headers(&env("PAQTRA_METRICS_OTLP_HEADERS").unwrap_or_default());
        let mut resource = parse_headers(&env("PAQTRA_METRICS_OTLP_RESOURCE").unwrap_or_default());
        if let Some(c) = env("PAQTRA_CLUSTER_NAME") {
            resource.push(("k8s.cluster.name".into(), c));
        }
        add(
            "otlp",
            SinkKind::Otlp {
                url: otlp_url(&ep),
                headers,
                resource,
            },
        );
    }
    if let Some(addr) = env("PAQTRA_METRICS_GRAPHITE_ADDR") {
        add("graphite", SinkKind::Graphite { addr });
    }
    out
}

/// Per-second counts derived from Hubble flows. Charts are per Cilium node
/// (`hubble.flows`, `hubble.policy_verdicts`, `hubble.drop_reasons`) and per
/// namespace (`hubble.http`, `hubble.dns`). Values are events per second.
#[derive(Default)]
pub struct HubbleAggregator {
    flows: BTreeMap<String, [f64; 4]>,
    policy: BTreeMap<String, [f64; 2]>,
    drops: BTreeMap<String, BTreeMap<String, f64>>,
    http: BTreeMap<String, [f64; 4]>,
    dns: BTreeMap<String, [f64; 3]>,
    last_flush: i64,
    observed: u64,
}

const MAX_KEYS: usize = 500;
const MAX_REASONS: usize = 64;
const FLOW_DIMS: [&str; 4] = ["forwarded", "dropped", "error", "audit"];
const POLICY_DIMS: [&str; 2] = ["allowed", "denied"];
const HTTP_DIMS: [&str; 4] = ["requests", "responses", "errors_4xx", "errors_5xx"];
const DNS_DIMS: [&str; 3] = ["queries", "responses", "errors"];

fn bump<const N: usize>(m: &mut BTreeMap<String, [f64; N]>, key: &str, i: usize) {
    if let Some(v) = m.get_mut(key) {
        v[i] += 1.0;
    } else if m.len() < MAX_KEYS {
        let mut v = [0.0; N];
        v[i] = 1.0;
        m.insert(key.to_string(), v);
    }
}

impl HubbleAggregator {
    pub fn observe(&mut self, f: &Flow) {
        self.observed += 1;
        let node = f
            .hubble
            .as_ref()
            .and_then(|h| h.node_name.as_deref())
            .filter(|n| !n.is_empty())
            .unwrap_or("unknown")
            .to_string();
        let verdict = f.verdict.to_ascii_uppercase();
        let vi = match verdict.as_str() {
            "FORWARDED" | "REDIRECTED" | "TRACED" | "TRANSLATED" => Some(0),
            "DROPPED" => Some(1),
            "ERROR" => Some(2),
            "AUDIT" => Some(3),
            _ => None,
        };
        if let Some(i) = vi {
            bump(&mut self.flows, &node, i);
        }
        let reason = f.drop_reason.as_deref().unwrap_or("");
        if verdict == "DROPPED" {
            let r = if reason.is_empty() { "unknown" } else { reason };
            if r.to_ascii_uppercase().contains("POLICY") {
                bump(&mut self.policy, &node, 1);
            }
            if self.drops.len() < MAX_KEYS || self.drops.contains_key(&node) {
                let m = self.drops.entry(node.clone()).or_default();
                if m.len() < MAX_REASONS || m.contains_key(r) {
                    *m.entry(r.to_string()).or_default() += 1.0;
                }
            }
        } else if f
            .hubble
            .as_ref()
            .is_some_and(|h| h.policy_match_type.is_some())
        {
            bump(&mut self.policy, &node, 0);
        }
        if let Some(code) = f.http_code {
            let ns = f.source.namespace.as_str();
            if !ns.is_empty() {
                bump(&mut self.http, ns, 1);
                if (400..500).contains(&code) {
                    bump(&mut self.http, ns, 2);
                } else if code >= 500 {
                    bump(&mut self.http, ns, 3);
                }
            }
        } else if f.http_method.is_some() && !f.destination.namespace.is_empty() {
            bump(&mut self.http, &f.destination.namespace, 0);
        }
        if let Some(rc) = f.dns_rcode {
            let ns = f.destination.namespace.as_str();
            if !ns.is_empty() {
                bump(&mut self.dns, ns, 1);
                if rc != 0 {
                    bump(&mut self.dns, ns, 2);
                }
            }
        } else if f.dns_query.is_some() && !f.source.namespace.is_empty() {
            bump(&mut self.dns, &f.source.namespace, 0);
        }
    }

    /// Rates since the previous flush, stamped `t`. Known charts report zero
    /// when idle so ratio rules see data.
    pub fn flush(&mut self, t: i64) -> Vec<Sample> {
        let dt = if self.last_flush > 0 {
            (t - self.last_flush).max(1) as f64
        } else {
            1.0
        };
        self.last_flush = t;
        let mut out = Vec::new();
        let mut push = |ctx: &str,
                        chart: String,
                        dim: &str,
                        units: &str,
                        title: &str,
                        labels: &[(&str, &str)],
                        v: f64| {
            out.push(Sample {
                series: Series {
                    context: ctx.into(),
                    chart,
                    dimension: dim.into(),
                    family: "hubble".into(),
                    units: units.into(),
                    title: title.into(),
                    chart_type: "line".into(),
                    labels: labels
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                },
                t,
                v: v / dt,
                a: false,
            });
        };
        for (node, v) in self.flows.iter_mut() {
            for (i, d) in FLOW_DIMS.iter().enumerate() {
                push(
                    "hubble.flows",
                    format!("hubble.flows.{node}"),
                    d,
                    "flows/s",
                    "Hubble flows by verdict",
                    &[("node", node)],
                    v[i],
                );
            }
            *v = [0.0; 4];
        }
        for (node, v) in self.policy.iter_mut() {
            for (i, d) in POLICY_DIMS.iter().enumerate() {
                push(
                    "hubble.policy_verdicts",
                    format!("hubble.policy_verdicts.{node}"),
                    d,
                    "flows/s",
                    "Cilium policy verdicts",
                    &[("node", node)],
                    v[i],
                );
            }
            *v = [0.0; 2];
        }
        for (node, m) in self.drops.iter_mut() {
            for (r, v) in m.iter_mut() {
                push(
                    "hubble.drop_reasons",
                    format!("hubble.drop_reasons.{node}"),
                    r,
                    "drops/s",
                    "Cilium drops by reason",
                    &[("node", node)],
                    *v,
                );
                *v = 0.0;
            }
        }
        for (ns, v) in self.http.iter_mut() {
            for (i, d) in HTTP_DIMS.iter().enumerate() {
                push(
                    "hubble.http",
                    format!("hubble.http.{ns}"),
                    d,
                    "events/s",
                    "HTTP seen by Hubble L7 visibility",
                    &[("k8s_namespace", ns)],
                    v[i],
                );
            }
            *v = [0.0; 4];
        }
        for (ns, v) in self.dns.iter_mut() {
            for (i, d) in DNS_DIMS.iter().enumerate() {
                push(
                    "hubble.dns",
                    format!("hubble.dns.{ns}"),
                    d,
                    "events/s",
                    "DNS seen by Hubble",
                    &[("k8s_namespace", ns)],
                    v[i],
                );
            }
            *v = [0.0; 3];
        }
        out
    }
}

/// Starts the maintenance, Hubble flush, anomaly training, alert
/// evaluation, notification and exporter loops.
pub fn spawn_metrics_platform(state: Arc<AppState>) {
    let p = state.metrics_platform.clone();

    // Hubble-derived series, maintenance and training on one blocking thread.
    let bg = p.clone();
    std::thread::Builder::new()
        .name("metrics-platform".into())
        .spawn(move || {
            let mut tick = 0u64;
            loop {
                std::thread::sleep(Duration::from_secs(1));
                tick += 1;
                let now = unix_now();
                let mut samples = bg.hubble.lock().unwrap().flush(now - 1);
                if !samples.is_empty() {
                    bg.detector.annotate(&mut samples);
                    let (_, err) = bg.cluster.append_batch(&samples);
                    if let Some(e) = err {
                        tracing::debug!("hubble series: {e}");
                    }
                }
                if let Some(a) = &bg.alerts {
                    a.evaluate(now);
                }
                if tick.is_multiple_of(10) {
                    for e in bg.hub.maintain(now) {
                        tracing::warn!("metrics maintenance: {e}");
                    }
                    if let Err(e) = bg.cluster.maintain(now) {
                        tracing::warn!("metrics maintenance (hubble): {e}");
                    }
                }
                // Agents score their own samples; the API scores the
                // Hubble-derived series only.
                if tick.is_multiple_of(60) {
                    bg.detector
                        .train_due(&bg.cluster, now, bg.detector.per_cycle());
                }
            }
        })
        .expect("spawn metrics platform thread");

    // Metric alert transitions to the notifier.
    if let Some(mut rx) = p.events.lock().unwrap().take() {
        let st = state.clone();
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                let rule_id = format!("metric:{}", ev.rule);
                if notifier::is_silenced(&st, &rule_id).await {
                    continue;
                }
                let kind = if ev.status == AlertStatus::Clear {
                    EventKind::Resolved
                } else {
                    EventKind::Firing
                };
                notifier::notify(
                    &st,
                    AlertEvent {
                        kind,
                        rule_id,
                        rule_name: ev.rule.clone(),
                        severity: ev.severity.to_string(),
                        message: ev.message.clone(),
                        at: chrono::DateTime::from_timestamp(ev.time, 0)
                            .unwrap_or_default()
                            .to_rfc3339(),
                    },
                )
                .await;
            }
        });
    }

    for ex in p.exporters.clone() {
        let p = p.clone();
        tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default();
            let mut t =
                tokio::time::interval(Duration::from_secs(ex.exporter.resolution().max(1) as u64));
            loop {
                t.tick().await;
                let now = unix_now();
                let (batch, commit) = ex.exporter.collect(&p.sources(), now);
                if batch.is_empty() {
                    continue;
                }
                match send(&client, &ex, &batch).await {
                    Ok(()) => ex.exporter.commit(commit, &batch, now),
                    Err(e) => {
                        tracing::warn!(sink = %ex.exporter.status().sink, "metrics export failed; will retry: {e}");
                        ex.exporter.fail(&e);
                    }
                }
            }
        });
    }
}

async fn post(
    client: &reqwest::Client,
    url: &str,
    body: Vec<u8>,
    headers: &[(String, String)],
    fixed: &[(&str, &str)],
) -> Result<(), String> {
    let mut req = client
        .post(url)
        .body(body)
        .header("User-Agent", "paqtra-metrics");
    for (k, v) in fixed {
        req = req.header(*k, *v);
    }
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!(
            "{url}: HTTP {}: {}",
            status.as_u16(),
            text.chars().take(512).collect::<String>().trim()
        ));
    }
    Ok(())
}

async fn send(
    client: &reqwest::Client,
    ex: &ExportRuntime,
    batch: &[ExportSeries],
) -> Result<(), String> {
    match &ex.sink {
        SinkKind::RemoteWrite { url, headers } => {
            for body in remote_write_bodies(&ex.prefix, batch) {
                post(client, url, body, headers, &REMOTE_WRITE_HEADERS).await?;
            }
            Ok(())
        }
        SinkKind::Otlp {
            url,
            headers,
            resource,
        } => {
            for body in otlp_bodies(&ex.prefix, resource, batch) {
                post(
                    client,
                    url,
                    body,
                    headers,
                    &[("Content-Type", "application/json")],
                )
                .await?;
            }
            Ok(())
        }
        SinkKind::Graphite { addr } => {
            use tokio::io::AsyncWriteExt;
            let lines = graphite_lines(&ex.prefix, batch);
            let fut = async {
                let mut c = tokio::net::TcpStream::connect(addr).await?;
                c.write_all(lines.as_bytes()).await?;
                c.shutdown().await
            };
            tokio::time::timeout(Duration::from_secs(10), fut)
                .await
                .map_err(|_| format!("{addr}: timeout"))?
                .map_err(|e| format!("{addr}: {e}"))
        }
    }
}

pub fn exporter_statuses(p: &MetricsPlatform) -> Vec<ExportStatus> {
    p.exporters.iter().map(|e| e.exporter.status()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::flow::{FlowEndpoint, FlowMeta};

    fn flow(verdict: &str, reason: Option<&str>) -> Flow {
        Flow {
            id: "1".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            source: FlowEndpoint {
                namespace: "shop".into(),
                pod: "web".into(),
                ip: "10.0.0.1".into(),
            },
            destination: FlowEndpoint {
                namespace: "db".into(),
                pod: "pg".into(),
                ip: "10.0.0.2".into(),
            },
            verdict: verdict.into(),
            protocol: "TCP".into(),
            port: 80,
            http_method: None,
            http_url: None,
            http_code: None,
            cluster: None,
            dns_query: None,
            dns_qtypes: None,
            dns_rcode: None,
            dns_rcode_name: None,
            dns_ips: None,
            dns_latency_ns: None,
            drop_reason: reason.map(String::from),
            hubble: Some(FlowMeta {
                node_name: Some("n1".into()),
                ..Default::default()
            }),
        }
    }

    fn value(s: &[Sample], ctx: &str, dim: &str) -> f64 {
        s.iter()
            .find(|x| x.series.context == ctx && x.series.dimension == dim)
            .map(|x| x.v)
            .unwrap_or(f64::NAN)
    }

    #[test]
    fn hubble_aggregation() {
        let mut h = HubbleAggregator::default();
        h.observe(&flow("FORWARDED", None));
        h.observe(&flow("DROPPED", Some("POLICY_DENIED")));
        let mut http = flow("FORWARDED", None);
        http.http_code = Some(503);
        h.observe(&http);
        let mut dns = flow("FORWARDED", None);
        dns.dns_rcode = Some(3);
        h.observe(&dns);
        let s = h.flush(1000);
        assert_eq!(value(&s, "hubble.flows", "forwarded"), 3.0);
        assert_eq!(value(&s, "hubble.flows", "dropped"), 1.0);
        assert_eq!(value(&s, "hubble.policy_verdicts", "denied"), 1.0);
        assert_eq!(value(&s, "hubble.drop_reasons", "POLICY_DENIED"), 1.0);
        assert_eq!(value(&s, "hubble.http", "errors_5xx"), 1.0);
        assert_eq!(value(&s, "hubble.dns", "errors"), 1.0);
        let http_s = s
            .iter()
            .find(|x| x.series.context == "hubble.http")
            .unwrap();
        assert_eq!(http_s.series.labels["k8s_namespace"], "shop");
        // Idle second: known charts report zero, rates divide by elapsed time.
        let s = h.flush(1002);
        assert_eq!(value(&s, "hubble.flows", "forwarded"), 0.0);
    }
}
