//! Agent side of the per-second metrics platform: runs the read-only
//! collectors, keeps an hour of samples in a local store that doubles as the
//! replay buffer, flags anomalies on the node and streams everything to the
//! API (`/api/v1/agents/metrics`).
//!
//! Pod identity comes from the Kubernetes API (pods on this node only) and
//! applications are discovered from pod annotations. The agent never reads
//! `/proc/<pid>/cmdline`, `environ` or Kubernetes Secrets.

mod discovery;

use anyhow::Result;
use paqtra_metrics::anomaly::{Detector, Options as DetectorOptions};
use paqtra_metrics::collectors::{
    host, parse_app_configs, AppCollector, AppConfig, AppStatus, Cgroups, Collector, Config, Fsys,
    HttpGet, ProcessGroups, Scheduler,
};
use paqtra_metrics::stream::{Post, Sender};
use paqtra_metrics::tsdb::{Db, Options as DbOptions, Sample};
use paqtra_metrics::wire::{Response, PATH};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use discovery::{spawn_pod_watch, PodIndex};

/// Agent metrics settings, all from `PAQTRA_METRICS_*` environment variables.
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    pub enabled: bool,
    /// API base URL; empty keeps metrics local to the agent.
    pub api: String,
    /// Sent as `X-Paqtra-Agent-Key`.
    pub agent_key: String,
    pub insecure_tls: bool,
    pub node: String,
    pub proc_dir: PathBuf,
    pub sys_dir: PathBuf,
    /// Prefix for mount points before statfs (`/host` in the DaemonSet).
    pub fs_root: Option<PathBuf>,
    pub cgroup_root: PathBuf,
    pub process_groups: usize,
    pub containers: bool,
    pub apps_file: Option<PathBuf>,
    pub app_discovery: bool,
    pub anomaly: bool,
    pub buffer: Duration,
}

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

impl MetricsConfig {
    pub fn from_env() -> Self {
        let num = |k: &str, d: u64| env(k).and_then(|v| v.parse().ok()).unwrap_or(d);
        MetricsConfig {
            enabled: env_bool("PAQTRA_METRICS", true),
            api: env("PAQTRA_METRICS_API")
                .unwrap_or_default()
                .trim_end_matches('/')
                .to_string(),
            agent_key: env("PAQTRA_AGENT_KEY").unwrap_or_default(),
            insecure_tls: env_bool("PAQTRA_METRICS_INSECURE_TLS", false),
            node: env("NODE_NAME").unwrap_or_else(|| "unknown".into()),
            proc_dir: env("PAQTRA_METRICS_PROC")
                .unwrap_or_else(|| "/proc".into())
                .into(),
            sys_dir: env("PAQTRA_METRICS_SYS")
                .unwrap_or_else(|| "/sys".into())
                .into(),
            fs_root: env("PAQTRA_METRICS_FS_ROOT").map(PathBuf::from),
            cgroup_root: env("PAQTRA_METRICS_CGROUP_ROOT")
                .unwrap_or_else(|| "/sys/fs/cgroup".into())
                .into(),
            process_groups: num("PAQTRA_METRICS_PROCESS_GROUPS", 64) as usize,
            containers: env_bool("PAQTRA_METRICS_CONTAINERS", false),
            apps_file: env("PAQTRA_METRICS_APPS_FILE").map(PathBuf::from),
            app_discovery: env_bool("PAQTRA_METRICS_APP_DISCOVERY", true),
            anomaly: env_bool("PAQTRA_METRICS_ANOMALY", true),
            buffer: Duration::from_secs(num("PAQTRA_METRICS_BUFFER_SECONDS", 3600).max(300)),
        }
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn http_agent(timeout: Duration, insecure: bool) -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .disable_verification(insecure)
        .build();
    ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .tls_config(tls)
            .build(),
    )
}

struct UreqGet {
    secure: ureq::Agent,
    insecure: ureq::Agent,
}

impl HttpGet for UreqGet {
    fn get(
        &self,
        url: &str,
        headers: &[(String, String)],
        insecure: bool,
        timeout: Duration,
    ) -> Result<Vec<u8>, String> {
        let agent = if insecure {
            &self.insecure
        } else {
            &self.secure
        };
        let mut req = agent
            .get(url)
            .config()
            .timeout_global(Some(timeout))
            .build();
        for (k, v) in headers {
            req = req.header(k.as_str(), v.as_str());
        }
        let mut resp = req.call().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(format!("{url}: HTTP {status}"));
        }
        resp.body_mut()
            .with_config()
            .limit(16 << 20)
            .read_to_vec()
            .map_err(|e| e.to_string())
    }
}

struct UreqPost {
    agent: ureq::Agent,
    url: String,
    key: String,
}

impl Post for UreqPost {
    fn post(&self, body: Vec<u8>) -> Result<Response, String> {
        let mut req = self
            .agent
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Content-Encoding", "gzip");
        if !self.key.is_empty() {
            req = req.header("X-Paqtra-Agent-Key", self.key.as_str());
        }
        let mut resp = req.send(&body[..]).map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let raw = resp
            .body_mut()
            .with_config()
            .limit(1 << 20)
            .read_to_vec()
            .unwrap_or_default();
        if status >= 300 {
            return Err(format!(
                "metrics ingest: HTTP {status} {}",
                String::from_utf8_lossy(&raw).trim()
            ));
        }
        serde_json::from_slice(&raw).map_err(|e| format!("metrics ingest response: {e}"))
    }
}

/// Running agent metrics.
pub struct AgentMetrics {
    pub cfg: MetricsConfig,
    scheduler: Arc<Scheduler>,
    db: Arc<Db>,
    sender: Option<Arc<Sender>>,
    detector: Option<Arc<Detector>>,
    apps: Option<Arc<Mutex<Vec<AppStatus>>>>,
    stop: Arc<AtomicBool>,
}

impl AgentMetrics {
    /// Builds the collectors and starts the scheduler, sender and trainer
    /// threads.
    pub fn start(cfg: MetricsConfig, pods: PodIndex) -> Result<Self> {
        let db = Arc::new(Db::open(DbOptions {
            tier0_retention: cfg.buffer,
            tier1_retention: Duration::from_secs(6 * 3600),
            tier2_retention: Duration::from_secs(86400),
            mem_tier1_rows: 360,
            mem_tier2_rows: 24,
            ..Default::default()
        })?);
        let detector = cfg
            .anomaly
            .then(|| Arc::new(Detector::new(DetectorOptions::default())));

        let fs = Fsys {
            proc: cfg.proc_dir.clone(),
            sys: cfg.sys_dir.clone(),
        };
        let mut cs: Vec<Box<dyn Collector>> = host(&Config {
            fs: fs.clone(),
            fs_root: cfg.fs_root.clone(),
        });
        if cfg.process_groups > 0 {
            cs.push(Box::new(ProcessGroups::new(fs.clone(), cfg.process_groups)));
        }
        let (root, idx) = (cfg.cgroup_root.clone(), pods.clone());
        cs.push(Box::new(Cgroups::new(
            Box::new(move || discovery::workloads(&root, &idx)),
            cfg.containers,
        )));

        let mut statics: Vec<AppConfig> = Vec::new();
        if let Some(p) = &cfg.apps_file {
            match std::fs::read_to_string(p)
                .map_err(|e| e.to_string())
                .and_then(|y| parse_app_configs(&y))
            {
                Ok(a) => statics = a,
                Err(e) => tracing::warn!("ignoring {}: {e}", p.display()),
            }
        }
        let mut apps = None;
        if !statics.is_empty() || cfg.app_discovery {
            let http = Arc::new(UreqGet {
                secure: http_agent(Duration::from_secs(5), false),
                insecure: http_agent(Duration::from_secs(5), true),
            });
            let discover = cfg.app_discovery.then(|| {
                let idx = pods.clone();
                Box::new(move || discovery::apps(&idx)) as paqtra_metrics::collectors::Discover
            });
            let ac = AppCollector::new(statics, http, discover, 5);
            apps = Some(ac.statuses());
            cs.push(Box::new(ac));
        }

        let (sink_db, sink_det) = (db.clone(), detector.clone());
        let sink = Arc::new(move |mut samples: Vec<Sample>| {
            if let Some(d) = &sink_det {
                d.annotate(&mut samples);
            }
            let (_, err) = sink_db.append_batch(&samples);
            if let Some(e) = err {
                tracing::debug!("metrics append: {e}");
            }
        });
        let scheduler = Arc::new(Scheduler::new(sink, cs));
        let stop = Arc::new(AtomicBool::new(false));
        scheduler.clone().spawn(stop.clone());

        let sender = (!cfg.api.is_empty()).then(|| {
            Arc::new(Sender::new(
                db.clone(),
                &cfg.node,
                Box::new(UreqPost {
                    agent: http_agent(Duration::from_secs(30), cfg.insecure_tls),
                    url: format!("{}{PATH}", cfg.api),
                    key: cfg.agent_key.clone(),
                }),
                0,
            ))
        });
        if let Some(s) = sender.clone() {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("metrics-sender".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        if let Err(e) = s.tick(5) {
                            tracing::debug!("metrics stream: {e}");
                        }
                        std::thread::sleep(Duration::from_secs(1));
                    }
                })?;
        }

        let (mdb, mdet, mstop) = (db.clone(), detector.clone(), stop.clone());
        std::thread::Builder::new()
            .name("metrics-maintain".into())
            .spawn(move || {
                let mut tick = 0u64;
                while !mstop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_secs(1));
                    tick += 1;
                    let now = unix_now();
                    if tick.is_multiple_of(10) {
                        if let Err(e) = mdb.maintain(now) {
                            tracing::debug!("metrics maintain: {e}");
                        }
                    }
                    if let Some(d) = &mdet {
                        if tick.is_multiple_of(60) {
                            d.train_due(&mdb, now, d.per_cycle());
                        }
                    }
                }
            })?;

        Ok(AgentMetrics {
            cfg,
            scheduler,
            db,
            sender,
            detector,
            apps,
            stop,
        })
    }

    /// `GET /metrics/status` on the agent.
    pub fn status_json(&self) -> serde_json::Value {
        serde_json::json!({
            "node": self.cfg.node,
            "api": self.cfg.api,
            "store": self.db.stats(),
            "collectors": self.scheduler.statuses(),
            "sender": self.sender.as_ref().map(|s| s.status()),
            "anomaly": self.detector.as_ref().map(|d| d.stats()),
            "apps": self.apps.as_ref().map(|a| a.lock().unwrap().clone()),
        })
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for AgentMetrics {
    fn drop(&mut self) {
        self.stop();
    }
}
