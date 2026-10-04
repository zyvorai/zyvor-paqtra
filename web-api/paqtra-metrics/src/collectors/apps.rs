//! Application collectors. Credentials are never written in the config:
//! `username_env`, `password_env` and `bearer_env` name environment variables
//! the operator populates (for example from a Secret mounted as env). Paqtra
//! never reads Kubernetes Secrets through the API for this.

use super::apps_native::{Apache, Haproxy, Memcached, Nginx, Redis};
use super::apps_prom::{PromApp, COREDNS_INCLUDE, ENVOY_INCLUDE, ETCD_INCLUDE};
use super::{Chart, Collector, Emitter, Info};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const DEFAULT_APP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppKind {
    Nginx,
    Apache,
    Haproxy,
    Redis,
    Memcached,
    Envoy,
    Coredns,
    Etcd,
    Prometheus,
}

impl AppKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AppKind::Nginx => "nginx",
            AppKind::Apache => "apache",
            AppKind::Haproxy => "haproxy",
            AppKind::Redis => "redis",
            AppKind::Memcached => "memcached",
            AppKind::Envoy => "envoy",
            AppKind::Coredns => "coredns",
            AppKind::Etcd => "etcd",
            AppKind::Prometheus => "prometheus",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.trim().to_lowercase())).ok()
    }

    fn tcp(self) -> bool {
        matches!(self, AppKind::Redis | AppKind::Memcached)
    }
}

/// One application endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub kind: AppKind,
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    /// host:port for redis and memcached.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub address: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub username_env: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password_env: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bearer_env: String,
    /// Prometheus metric name globs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub max_series: usize,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub insecure_skip_verify: bool,
    /// Seconds.
    #[serde(default)]
    pub timeout: f64,
    #[serde(skip)]
    pub discovered: bool,
}

#[derive(Deserialize)]
struct AppsFile {
    #[serde(default)]
    apps: Vec<AppConfig>,
}

/// Parses an apps YAML document (`apps: [...]`).
pub fn parse_app_configs(yaml: &str) -> Result<Vec<AppConfig>, String> {
    let f: AppsFile = serde_yaml::from_str(yaml).map_err(|e| e.to_string())?;
    let mut out = f.apps;
    for (i, c) in out.iter_mut().enumerate() {
        c.normalize().map_err(|e| format!("app {i}: {e}"))?;
    }
    Ok(out)
}

impl AppConfig {
    pub fn new(kind: AppKind, name: &str, target: &str) -> Self {
        let mut c = AppConfig {
            kind,
            name: name.into(),
            url: String::new(),
            address: String::new(),
            username_env: String::new(),
            password_env: String::new(),
            bearer_env: String::new(),
            include: Vec::new(),
            exclude: Vec::new(),
            max_series: 0,
            labels: BTreeMap::new(),
            insecure_skip_verify: false,
            timeout: 0.0,
            discovered: false,
        };
        if kind.tcp() {
            c.address = target.into();
        } else {
            c.url = target.into();
        }
        c
    }

    pub fn normalize(&mut self) -> Result<(), String> {
        if self.name.is_empty() {
            self.name = self.kind.as_str().into();
        }
        if self.kind.tcp() {
            if self.address.is_empty() {
                return Err(format!(
                    "{} {}: address is required",
                    self.kind.as_str(),
                    self.name
                ));
            }
        } else if self.url.is_empty() {
            return Err(format!(
                "{} {}: url is required",
                self.kind.as_str(),
                self.name
            ));
        }
        if self.timeout <= 0.0 {
            self.timeout = DEFAULT_APP_TIMEOUT.as_secs_f64();
        }
        if self.max_series == 0 {
            self.max_series = 2000;
        }
        if self.include.is_empty() {
            let preset: &[&str] = match self.kind {
                AppKind::Envoy => ENVOY_INCLUDE,
                AppKind::Coredns => COREDNS_INCLUDE,
                AppKind::Etcd => ETCD_INCLUDE,
                _ => &[],
            };
            self.include = preset.iter().map(|s| s.to_string()).collect();
        }
        Ok(())
    }

    pub fn id(&self) -> String {
        format!("{}/{}", self.kind.as_str(), self.name)
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_secs_f64(self.timeout.max(0.1))
    }

    /// Context `<kind>.<metric>`, chart id `<kind>_<name>.<metric>`, labels
    /// `app_name` plus operator labels.
    pub(crate) fn chart(
        &self,
        metric: &str,
        family: &str,
        units: &str,
        title: &str,
        ty: &str,
    ) -> Chart {
        let mut lbl = self.labels.clone();
        lbl.insert("app_name".into(), self.name.clone());
        let k = self.kind.as_str();
        Chart::new(&format!("{k}.{metric}"), family, units, title)
            .id(format!("{k}_{}.{metric}", sanitize_id(&self.name)))
            .ty(ty)
            .labels(&lbl)
    }

    /// Request headers for this app's auth, read from the environment.
    pub(crate) fn auth_headers(&self) -> Vec<(String, String)> {
        let env = |k: &str| {
            if k.is_empty() {
                String::new()
            } else {
                std::env::var(k).unwrap_or_default()
            }
        };
        if !self.bearer_env.is_empty() {
            return vec![(
                "Authorization".into(),
                format!("Bearer {}", env(&self.bearer_env)),
            )];
        }
        if !self.username_env.is_empty() || !self.password_env.is_empty() {
            let raw = format!("{}:{}", env(&self.username_env), env(&self.password_env));
            return vec![(
                "Authorization".into(),
                format!("Basic {}", base64(raw.as_bytes())),
            )];
        }
        Vec::new()
    }

    pub(crate) fn get(&self, http: &dyn HttpGet, url: &str) -> Result<Vec<u8>, String> {
        http.get(
            url,
            &self.auth_headers(),
            self.insecure_skip_verify,
            self.timeout(),
        )
    }
}

pub(crate) fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Fetches an HTTP(S) URL with extra headers, reading at most 8 MiB. The
/// agent supplies the implementation so this crate stays free of an HTTP
/// client.
pub trait HttpGet: Send + Sync {
    fn get(
        &self,
        url: &str,
        headers: &[(String, String)],
        insecure: bool,
        timeout: Duration,
    ) -> Result<Vec<u8>, String>;
}

/// Adapts a closure to `HttpGet`.
pub struct HttpGetFn<F>(pub F);

impl<F> HttpGet for HttpGetFn<F>
where
    F: Fn(&str, &[(String, String)], bool, Duration) -> Result<Vec<u8>, String> + Send + Sync,
{
    fn get(
        &self,
        url: &str,
        headers: &[(String, String)],
        insecure: bool,
        timeout: Duration,
    ) -> Result<Vec<u8>, String> {
        (self.0)(url, headers, insecure, timeout)
    }
}

pub(crate) trait App: Send {
    fn collect(&mut self, http: &dyn HttpGet, e: &mut Emitter) -> Result<(), String>;
}

fn make(c: &AppConfig) -> Box<dyn App> {
    match c.kind {
        AppKind::Nginx => Box::new(Nginx(c.clone())),
        AppKind::Apache => Box::new(Apache(c.clone())),
        AppKind::Haproxy => Box::new(Haproxy(c.clone())),
        AppKind::Redis => Box::new(Redis(c.clone())),
        AppKind::Memcached => Box::new(Memcached(c.clone())),
        _ => Box::new(PromApp::new(c.clone())),
    }
}

/// One configured or discovered application.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub target: String,
    pub discovered: bool,
    pub ok: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_error: String,
    pub last_ok: i64,
}

struct Entry {
    cfg: AppConfig,
    app: Box<dyn App>,
    status: AppStatus,
    fails: u32,
    retry: i64,
}

pub type Discover = Box<dyn Fn() -> Vec<AppConfig> + Send>;

/// Drives every configured application and, with a discovery function, the
/// applications it finds. Runs as one collector so the scheduler sees a
/// single slot; each app has its own timeout.
pub struct AppCollector {
    http: Arc<dyn HttpGet>,
    every: u64,
    discover: Option<Discover>,
    last_disc: i64,
    entries: HashMap<String, Entry>,
    statuses: Arc<Mutex<Vec<AppStatus>>>,
}

impl AppCollector {
    pub fn new(
        statics: Vec<AppConfig>,
        http: Arc<dyn HttpGet>,
        discover: Option<Discover>,
        every: u64,
    ) -> Self {
        let mut a = Self {
            http,
            every: every.max(1),
            discover,
            last_disc: i64::MIN / 2,
            entries: HashMap::new(),
            statuses: Arc::new(Mutex::new(Vec::new())),
        };
        for c in statics {
            a.add(c);
        }
        a
    }

    /// A handle that reads the latest statuses from another thread.
    pub fn statuses(&self) -> Arc<Mutex<Vec<AppStatus>>> {
        self.statuses.clone()
    }

    fn add(&mut self, c: AppConfig) {
        let id = c.id();
        if self.entries.contains_key(&id) {
            return;
        }
        let target = if c.url.is_empty() {
            c.address.clone()
        } else {
            c.url.clone()
        };
        let status = AppStatus {
            id: id.clone(),
            kind: c.kind.as_str().into(),
            name: c.name.clone(),
            target,
            discovered: c.discovered,
            ok: false,
            last_error: String::new(),
            last_ok: 0,
        };
        self.entries.insert(
            id,
            Entry {
                app: make(&c),
                cfg: c,
                status,
                fails: 0,
                retry: 0,
            },
        );
    }
}

impl Collector for AppCollector {
    fn info(&self) -> Info {
        Info::new("apps", "apps", self.every)
    }

    fn collect(&mut self, now: i64, e: &mut Emitter) -> Result<(), String> {
        if self.discover.is_some() && now - self.last_disc >= 60 {
            self.last_disc = now;
            let found = (self.discover.as_ref().unwrap())();
            let live: std::collections::HashSet<String> = found.iter().map(AppConfig::id).collect();
            for mut c in found {
                c.discovered = true;
                if c.normalize().is_ok() {
                    self.add(c);
                }
            }
            // Discovered apps whose pod went away are dropped; static ones
            // stay and report failures.
            self.entries
                .retain(|id, en| !en.cfg.discovered || live.contains(id));
        }
        let mut ids: Vec<String> = self.entries.keys().cloned().collect();
        ids.sort();
        let up = Chart::new(
            "apps.up",
            "apps",
            "boolean",
            "Application collectors reachable (1 up)",
        );
        for id in &ids {
            let en = self.entries.get_mut(id).unwrap();
            if now < en.retry {
                continue;
            }
            match en.app.collect(self.http.as_ref(), e) {
                Err(err) => {
                    en.fails += 1;
                    en.status.ok = false;
                    en.status.last_error = err;
                    // Back off failing apps: discovered guesses give up quickly.
                    let mut wait = en.fails.min(10) as i64 * self.every as i64;
                    if en.cfg.discovered && en.fails >= 3 {
                        wait = 600;
                    }
                    en.retry = now + wait;
                    e.gauge(&up, id, 0.0);
                }
                Ok(()) => {
                    en.fails = 0;
                    en.status.ok = true;
                    en.status.last_error.clear();
                    en.status.last_ok = now;
                    e.gauge(&up, id, 1.0);
                }
            }
        }
        let mut st: Vec<AppStatus> = self.entries.values().map(|e| e.status.clone()).collect();
        st.sort_by(|a, b| a.id.cmp(&b.id));
        *self.statuses.lock().unwrap() = st;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_normalize() {
        let y = r#"
apps:
  - kind: redis
    name: cache
    address: 10.0.0.5:6379
    password_env: REDIS_PASSWORD
  - kind: envoy
    url: http://127.0.0.1:9901/stats/prometheus
"#;
        let apps = parse_app_configs(y).unwrap();
        assert_eq!(apps[0].id(), "redis/cache");
        assert_eq!(apps[1].name, "envoy");
        assert!(!apps[1].include.is_empty());
        assert!(parse_app_configs("apps:\n  - kind: nginx\n").is_err());
        assert!(parse_app_configs("apps:\n  - kind: mystery\n    url: x\n").is_err());
    }

    #[test]
    fn basic_auth_from_env() {
        std::env::set_var("PAQTRA_TEST_USER", "user");
        std::env::set_var("PAQTRA_TEST_PASS", "pass");
        let mut c = AppConfig::new(AppKind::Nginx, "n", "http://x/");
        c.username_env = "PAQTRA_TEST_USER".into();
        c.password_env = "PAQTRA_TEST_PASS".into();
        assert_eq!(c.auth_headers()[0].1, "Basic dXNlcjpwYXNz");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"a"), "YQ==");
    }

    #[test]
    fn collector_reports_up_and_backs_off() {
        let http: Arc<dyn HttpGet> = Arc::new(HttpGetFn(
            |url: &str, _: &[(String, String)], _, _| {
                if url.contains("good") {
                    Ok(b"Active connections: 2\nserver accepts handled requests\n 10 10 20\nReading: 0 Writing: 1 Waiting: 1\n".to_vec())
                } else {
                    Err("refused".into())
                }
            },
        ));
        let mut a = AppCollector::new(
            vec![
                AppConfig::new(AppKind::Nginx, "good", "http://good/stub_status"),
                AppConfig::new(AppKind::Nginx, "bad", "http://bad/stub_status"),
            ],
            http,
            None,
            5,
        );
        let mut e = Emitter::new();
        e.begin(1000);
        a.collect(1000, &mut e).unwrap();
        let up: HashMap<_, _> = e
            .samples()
            .iter()
            .filter(|s| s.series.context == "apps.up")
            .map(|s| (s.series.dimension.clone(), s.v))
            .collect();
        assert_eq!(up["nginx/good"], 1.0);
        assert_eq!(up["nginx/bad"], 0.0);
        assert!(e
            .samples()
            .iter()
            .any(|s| s.series.context == "nginx.connections"));
        e.begin(1001);
        a.collect(1001, &mut e).unwrap();
        assert!(!e
            .samples()
            .iter()
            .any(|s| s.series.dimension == "nginx/bad"));
        let st = a.statuses();
        let st = st.lock().unwrap();
        assert_eq!(st[0].id, "nginx/bad");
        assert_eq!(st[0].last_error, "refused");
    }
}
