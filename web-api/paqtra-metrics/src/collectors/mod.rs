//! Per-second host, network, cgroup, process-group and application
//! collectors. Every collector is read-only: it parses /proc, /sys,
//! cgroupfs or an application status endpoint and never writes to what it
//! observes. The process-group collector reads `/proc/<pid>/stat`, `statm`
//! and `io` only; it never opens `cmdline` or `environ`.

mod apps;
mod apps_native;
mod apps_prom;
mod cgroup;
mod fs;
mod host;
mod net;
mod procgroups;
mod scheduler;

pub use apps::{
    parse_app_configs, AppCollector, AppConfig, AppKind, AppStatus, Discover, HttpGet, HttpGetFn,
    DEFAULT_APP_TIMEOUT,
};
pub use apps_prom::{parse_prometheus_text, PromSample};
pub use cgroup::{discover_pod_cgroups, pod_level, Cgroups, PodCgroup, Workload};
pub use fs::Filesystems;
pub use host::{Cpu, Disks, Memory, Pressure, System};
pub use net::{Conntrack, NetDev, Netstat, Snmp, Sockstat, Softnet};
pub use procgroups::ProcessGroups;
pub use scheduler::{Scheduler, Sink, Status};

use crate::tsdb::{Sample, Series};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Describes a collector.
#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub name: String,
    pub family: String,
    /// Seconds between runs; 0 means 1.
    pub every: u64,
}

impl Info {
    pub fn new(name: &str, family: &str, every: u64) -> Self {
        Self {
            name: name.into(),
            family: family.into(),
            every,
        }
    }
}

/// Produces samples on each tick.
pub trait Collector: Send {
    fn info(&self) -> Info;
    fn collect(&mut self, now: i64, e: &mut Emitter) -> Result<(), String>;
}

/// Groups the dimensions emitted together.
#[derive(Debug, Clone, Default)]
pub struct Chart {
    pub context: String,
    pub id: String,
    pub family: String,
    pub units: String,
    pub title: String,
    pub ty: String,
    pub labels: BTreeMap<String, String>,
}

impl Chart {
    pub fn new(context: &str, family: &str, units: &str, title: &str) -> Self {
        Self {
            context: context.into(),
            family: family.into(),
            units: units.into(),
            title: title.into(),
            ..Default::default()
        }
    }

    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    pub fn ty(mut self, ty: &str) -> Self {
        self.ty = ty.into();
        self
    }

    pub fn labels(mut self, l: &BTreeMap<String, String>) -> Self {
        self.labels = l.clone();
        self
    }

    fn series(&self, dim: &str) -> Series {
        Series {
            context: self.context.clone(),
            chart: if self.id.is_empty() {
                self.context.clone()
            } else {
                self.id.clone()
            },
            dimension: dim.into(),
            family: self.family.clone(),
            units: self.units.clone(),
            title: self.title.clone(),
            chart_type: self.ty.clone(),
            labels: self.labels.clone(),
        }
    }
}

pub fn labels<const N: usize>(kv: [(&str, &str); N]) -> BTreeMap<String, String> {
    kv.into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

struct Prev {
    at: i64,
    v: f64,
    run: u64,
}

/// Collects samples for one collector run and keeps the previous raw value
/// of every incremental dimension across runs.
#[derive(Default)]
pub struct Emitter {
    now: i64,
    out: Vec<Sample>,
    run: u64,
    prev: HashMap<String, Prev>,
}

impl Emitter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a run at `now` and clears the previous run's samples.
    pub fn begin(&mut self, now: i64) {
        self.now = now;
        self.out.clear();
        self.run += 1;
    }

    pub fn samples(&self) -> &[Sample] {
        &self.out
    }

    pub fn take(&mut self) -> Vec<Sample> {
        std::mem::take(&mut self.out)
    }

    /// Forgets incremental dimensions not seen for 120 runs.
    pub fn end(&mut self) {
        let run = self.run;
        self.prev.retain(|_, p| run - p.run <= 120);
    }

    /// An absolute value. NaN and infinities are dropped.
    pub fn gauge(&mut self, c: &Chart, dim: &str, v: f64) {
        if !v.is_finite() {
            return;
        }
        self.out.push(Sample {
            series: c.series(dim),
            t: self.now,
            v,
            a: false,
        });
    }

    /// The per-second rate of a monotonic counter times `mult`. The first
    /// observation and counter resets emit nothing.
    pub fn incremental(&mut self, c: &Chart, dim: &str, raw: f64, mult: f64) {
        let s = c.series(dim);
        let key = s.key();
        let now = self.now;
        let run = self.run;
        match self.prev.get_mut(&key) {
            Some(p) => {
                let dt = (now - p.at) as f64;
                if raw >= p.v && dt > 0.0 && raw.is_finite() {
                    self.out.push(Sample {
                        series: s,
                        t: now,
                        v: (raw - p.v) / dt * mult,
                        a: false,
                    });
                }
                *p = Prev {
                    at: now,
                    v: raw,
                    run,
                };
            }
            None => {
                self.prev.insert(
                    key,
                    Prev {
                        at: now,
                        v: raw,
                        run,
                    },
                );
            }
        }
    }
}

/// Host filesystem roots, so tests can point at fixtures and containers at a
/// host mount.
#[derive(Debug, Clone)]
pub struct Fsys {
    pub proc: PathBuf,
    pub sys: PathBuf,
}

impl Default for Fsys {
    fn default() -> Self {
        Self {
            proc: "/proc".into(),
            sys: "/sys".into(),
        }
    }
}

impl Fsys {
    pub fn proc(&self, rel: &str) -> PathBuf {
        self.proc.join(rel)
    }
    pub fn sys(&self, rel: &str) -> PathBuf {
        self.sys.join(rel)
    }
}

/// Locates the host filesystems the collectors read.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub fs: Fsys,
    /// Prefixes mount points before statfs, for example `/host` when the
    /// host root is mounted there.
    pub fs_root: Option<PathBuf>,
}

/// The host and network collectors.
pub fn host(cfg: &Config) -> Vec<Box<dyn Collector>> {
    let f = cfg.fs.clone();
    vec![
        Box::new(Cpu::new(f.clone())),
        Box::new(Memory::new(f.clone())),
        Box::new(Pressure::new(f.clone())),
        Box::new(Disks::new(f.clone())),
        Box::new(Filesystems::new(f.clone(), cfg.fs_root.clone())),
        Box::new(System::new(f.clone())),
        Box::new(NetDev::new(f.clone())),
        Box::new(Snmp::new(f.clone())),
        Box::new(Netstat::new(f.clone())),
        Box::new(Sockstat::new(f.clone())),
        Box::new(Conntrack::new(f.clone())),
        Box::new(Softnet::new(f)),
    ]
}

pub(crate) fn read_lines(p: &Path) -> Result<Vec<String>, String> {
    std::fs::read_to_string(p)
        .map(|s| s.lines().map(str::to_string).collect())
        .map_err(|e| format!("{}: {e}", p.display()))
}

pub(crate) fn read_trim(p: &Path) -> Option<String> {
    std::fs::read_to_string(p)
        .ok()
        .map(|s| s.trim().to_string())
}

pub(crate) fn read_float(p: &Path) -> Option<f64> {
    read_trim(p)?.parse().ok()
}

/// Parses a float or unsigned integer; anything else is 0.
pub(crate) fn pf(s: &str) -> f64 {
    s.parse::<f64>()
        .or_else(|_| s.parse::<u64>().map(|v| v as f64))
        .unwrap_or(0.0)
}

pub(crate) fn phex(s: &str) -> f64 {
    u64::from_str_radix(s, 16).unwrap_or(0) as f64
}

/// "key value [unit]" lines such as /proc/meminfo, keys without a trailing
/// colon.
pub(crate) fn key_value_file(p: &Path) -> Result<HashMap<String, f64>, String> {
    Ok(read_lines(p)?
        .iter()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let k = f.next()?;
            let v = f.next()?;
            Some((k.trim_end_matches(':').to_string(), pf(v)))
        })
        .collect())
}

/// The paired header/value lines of /proc/net/snmp and netstat as
/// section -> field -> value.
pub(crate) fn header_pairs(p: &Path) -> Result<HashMap<String, HashMap<String, f64>>, String> {
    let lines = read_lines(p)?;
    let mut out: HashMap<String, HashMap<String, f64>> = HashMap::new();
    for pair in lines.chunks(2) {
        let [h, v] = pair else { continue };
        let h: Vec<&str> = h.split_whitespace().collect();
        let v: Vec<&str> = v.split_whitespace().collect();
        if h.is_empty() || h.len() != v.len() || h[0] != v[0] {
            continue;
        }
        let m = out
            .entry(h[0].trim_end_matches(':').to_string())
            .or_default();
        for j in 1..h.len() {
            m.insert(h[j].to_string(), pf(v[j]));
        }
    }
    Ok(out)
}

pub(crate) trait Get {
    fn g(&self, k: &str) -> f64;
}

impl Get for HashMap<String, f64> {
    fn g(&self, k: &str) -> f64 {
        self.get(k).copied().unwrap_or(0.0)
    }
}

impl Get for Option<&HashMap<String, f64>> {
    fn g(&self, k: &str) -> f64 {
        self.and_then(|m| m.get(k)).copied().unwrap_or(0.0)
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    /// A temp copy of testdata/<name> so tests can change counters between
    /// runs.
    pub struct Fixture {
        dir: tempfile::TempDir,
    }

    fn copy_dir(src: &Path, dst: &Path) {
        std::fs::create_dir_all(dst).unwrap();
        for e in std::fs::read_dir(src).unwrap() {
            let e = e.unwrap();
            let to = dst.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                copy_dir(&e.path(), &to);
            } else {
                std::fs::copy(e.path(), to).unwrap();
            }
        }
    }

    impl Fixture {
        pub fn new(name: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let src = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("testdata")
                .join(name);
            copy_dir(&src, dir.path());
            Self { dir }
        }

        pub fn path(&self, rel: &str) -> PathBuf {
            self.dir.path().join(rel)
        }

        pub fn fs(&self) -> Fsys {
            Fsys {
                proc: self.path("proc"),
                sys: self.path("sys"),
            }
        }

        pub fn rewrite(&self, rel: &str, old: &str, new: &str) {
            let p = self.path(rel);
            let s = std::fs::read_to_string(&p).unwrap();
            assert!(s.contains(old), "{rel} does not contain {old:?}");
            std::fs::write(p, s.replacen(old, new, 1)).unwrap();
        }

        pub fn write(&self, rel: &str, content: &str) {
            let p = self.path(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
    }

    /// Samples of one run keyed by "chart/dimension".
    #[derive(Debug)]
    pub struct Found(pub HashMap<String, f64>);

    impl Found {
        pub fn from(samples: &[Sample]) -> Self {
            Found(
                samples
                    .iter()
                    .map(|s| (format!("{}/{}", s.series.chart, s.series.dimension), s.v))
                    .collect(),
            )
        }

        pub fn has(&self, k: &str) -> bool {
            self.0.contains_key(k)
        }

        pub fn keys(&self) -> impl Iterator<Item = &String> {
            self.0.keys()
        }

        pub fn want(&self, k: &str, v: f64) {
            let got = *self
                .0
                .get(k)
                .unwrap_or_else(|| panic!("missing {k} in {:?}", self.0.keys()));
            assert!(
                (got - v).abs() <= 1e-6 * v.abs().max(1.0),
                "{k} = {got}, want {v}"
            );
        }
    }

    /// Drives one collector with a persistent emitter.
    pub struct Runs {
        e: Emitter,
    }

    impl Runs {
        pub fn new() -> Self {
            Self { e: Emitter::new() }
        }

        pub fn run(&mut self, c: &mut dyn Collector, now: i64) -> Found {
            self.e.begin(now);
            c.collect(now, &mut self.e).unwrap();
            self.e.end();
            Found::from(self.e.samples())
        }
    }
}
