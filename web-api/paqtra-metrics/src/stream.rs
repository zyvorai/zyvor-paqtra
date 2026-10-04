//! Both ends of the agent-to-API stream without a transport: `Hub` is the
//! API's set of per-node stores and `Sender` walks an agent store forward,
//! replaying from the API's newest second after a gap. Hosts plug in their
//! HTTP client and server.

use crate::tsdb::{Db, Options as DbOptions, Sample, Source, Stats};
use crate::wire::{encode, from_series_points, Batch, Response};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

/// Points per POST when `Sender::new` gets 0.
pub const DEFAULT_MAX_POINTS: usize = 500_000;

/// `^[A-Za-z0-9][A-Za-z0-9._-]{0,252}$` and no `..`.
pub fn valid_node(node: &str) -> bool {
    let b = node.as_bytes();
    !b.is_empty()
        && b.len() <= 253
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(c))
        && !node.contains("..")
}

#[derive(Debug, PartialEq)]
pub enum IngestError {
    /// HTTP 429.
    NodeLimit,
    /// HTTP 400.
    Invalid(String),
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IngestError::NodeLimit => f.write_str("metrics node limit reached"),
            IngestError::Invalid(s) => f.write_str(s),
        }
    }
}

#[derive(Default)]
pub struct HubOptions {
    /// One store directory per node. None keeps everything in memory.
    pub dir: Option<PathBuf>,
    /// `dir` inside is ignored and set per node.
    pub db: DbOptions,
    pub max_nodes: usize, // default 2000
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    pub node: String,
    pub last_ingest: i64,
    pub stats: Stats,
}

pub type Hook = Box<dyn Fn(&str, &mut [Sample]) + Send + Sync>;
pub type OnIngest = Box<dyn Fn(&str, &[Sample]) + Send + Sync>;

/// The API's per-node stores.
pub struct Hub {
    opts: HubOptions,
    dbs: RwLock<HashMap<String, Arc<Db>>>,
    seen: Mutex<HashMap<String, i64>>,
    /// May mark samples anomalous before they are stored.
    pub annotate: Option<Hook>,
    /// Runs after each stored batch. Must not block.
    pub on_ingest: Option<OnIngest>,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Hub {
    /// Opens existing node stores under `opts.dir`.
    pub fn open(mut opts: HubOptions) -> Result<Hub, String> {
        if opts.max_nodes == 0 {
            opts.max_nodes = 2000;
        }
        let h = Hub {
            opts,
            dbs: RwLock::new(HashMap::new()),
            seen: Mutex::new(HashMap::new()),
            annotate: None,
            on_ingest: None,
        };
        if let Some(dir) = h.opts.dir.clone() {
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let rd = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if e.path().is_dir() && valid_node(&name) {
                    let db = h
                        .open_db(&name)
                        .map_err(|e| format!("open metrics for node {name}: {e}"))?;
                    h.dbs.write().unwrap().insert(name, db);
                }
            }
        }
        Ok(h)
    }

    fn open_db(&self, node: &str) -> Result<Arc<Db>, String> {
        let mut o = self.opts.db.clone();
        o.dir = self.opts.dir.as_ref().map(|d| d.join(node));
        Db::open(o).map(Arc::new).map_err(|e| e.to_string())
    }

    fn db_for(&self, node: &str) -> Result<Arc<Db>, IngestError> {
        if let Some(db) = self.dbs.read().unwrap().get(node) {
            return Ok(db.clone());
        }
        let mut dbs = self.dbs.write().unwrap();
        if let Some(db) = dbs.get(node) {
            return Ok(db.clone());
        }
        if dbs.len() >= self.opts.max_nodes {
            return Err(IngestError::NodeLimit);
        }
        let db = self.open_db(node).map_err(IngestError::Invalid)?;
        dbs.insert(node.into(), db.clone());
        Ok(db)
    }

    /// Stores a batch and returns the node's newest stored second.
    pub fn ingest(&self, b: &Batch) -> Result<Response, IngestError> {
        if !valid_node(&b.node) {
            return Err(IngestError::Invalid(format!(
                "invalid node name {:?}",
                b.node
            )));
        }
        let db = self.db_for(&b.node)?;
        let prev = db.last_t();
        if b.from > prev && !b.series.is_empty() {
            return Ok(Response {
                prev_last_t: prev,
                last_t: prev,
                stored: 0,
                gap: true,
            });
        }
        let (n, err, samples) = if self.annotate.is_none() && self.on_ingest.is_none() {
            // Per series, not per point: a catch-up batch is ~150k points
            // over ~20k series, and a Sample clones its series metadata.
            let (mut n, mut first) = (0, None);
            for ws in &b.series {
                let (k, e) = db.append_points(
                    &ws.series,
                    ws.points.iter().map(|p| (p[0] as i64, p[1], p[2] != 0.0)),
                );
                n += k;
                if first.is_none() {
                    first = e;
                }
            }
            (n, first, Vec::new())
        } else {
            let mut samples = b.samples();
            if let Some(f) = &self.annotate {
                f(&b.node, &mut samples);
            }
            let (n, err) = db.append_batch(&samples);
            (n, err, samples)
        };
        self.seen.lock().unwrap().insert(b.node.clone(), unix_now());
        if let Some(e) = err {
            if !matches!(
                e,
                crate::tsdb::Error::SeriesLimit | crate::tsdb::Error::OutOfOrder
            ) {
                return Err(IngestError::Invalid(e.to_string()));
            }
        }
        if n > 0 {
            if let Some(f) = &self.on_ingest {
                f(&b.node, &samples);
            }
        }
        Ok(Response {
            prev_last_t: prev,
            last_t: db.last_t(),
            stored: n,
            gap: false,
        })
    }

    /// Every node store, sorted by node.
    pub fn sources(&self) -> Vec<Source> {
        let mut out: Vec<Source> = self
            .dbs
            .read()
            .unwrap()
            .iter()
            .map(|(n, db)| Source {
                node: n.clone(),
                db: db.clone(),
            })
            .collect();
        out.sort_by(|a, b| a.node.cmp(&b.node));
        out
    }

    pub fn db(&self, node: &str) -> Option<Arc<Db>> {
        self.dbs.read().unwrap().get(node).cloned()
    }

    pub fn nodes(&self) -> Vec<NodeInfo> {
        let seen = self.seen.lock().unwrap().clone();
        self.sources()
            .into_iter()
            .map(|s| NodeInfo {
                last_ingest: seen.get(&s.node).copied().unwrap_or(0),
                stats: s.db.stats(),
                node: s.node,
            })
            .collect()
    }

    /// Rolls up and expires every store.
    pub fn maintain(&self, now: i64) -> Vec<String> {
        self.sources()
            .into_iter()
            .filter_map(|s| s.db.maintain(now).err().map(|e| format!("{}: {e}", s.node)))
            .collect()
    }

    pub fn close(&self) {
        for (_, db) in self.dbs.write().unwrap().drain() {
            let _ = db.close();
        }
    }
}

/// POSTs one gzip JSON body to the API's ingest path and returns the
/// decoded response.
pub trait Post: Send + Sync {
    fn post(&self, body: Vec<u8>) -> Result<Response, String>;
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SenderStatus {
    pub cursor: i64,
    pub sent_batches: u64,
    pub sent_points: u64,
    pub failures: u64,
    pub replays: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_error: String,
    pub last_sent_at: i64,
}

/// Streams an agent store to the API.
pub struct Sender {
    db: Arc<Db>,
    node: String,
    post: Box<dyn Post>,
    /// Bounds one POST. A batch spans at least one second; while catching up
    /// it spans max_points/series seconds. Every batch repeats each series'
    /// metadata, so with ~20k series a small cap spends most of each POST on
    /// metadata and catch-up never outpaces collection. 500k points stay well
    /// under `MAX_DECODED`.
    max_points: usize,
    state: Mutex<(i64, bool, SenderStatus)>,
}

impl Sender {
    pub fn new(db: Arc<Db>, node: &str, post: Box<dyn Post>, max_points: usize) -> Self {
        Sender {
            db,
            node: node.into(),
            post,
            max_points: if max_points == 0 {
                DEFAULT_MAX_POINTS
            } else {
                max_points
            },
            state: Mutex::new((0, false, SenderStatus::default())),
        }
    }

    pub fn status(&self) -> SenderStatus {
        let st = self.state.lock().unwrap();
        SenderStatus {
            cursor: st.0,
            ..st.2.clone()
        }
    }

    /// Sends one batch; `Ok(true)` means a backlog remains.
    pub fn once(&self) -> Result<bool, String> {
        let (cursor, synced) = {
            let st = self.state.lock().unwrap();
            (st.0, st.1)
        };
        let mut b = Batch {
            node: self.node.clone(),
            from: cursor,
            to: cursor,
            series: Vec::new(),
        };
        let mut npts = 0usize;
        if synced {
            let (sp, to) = self.db.since(cursor, self.max_points);
            b.series = from_series_points(sp);
            b.to = to;
            npts = b.series.iter().map(|s| s.points.len()).sum();
            if to == cursor {
                return Ok(false);
            }
        }
        let res = encode(&b)
            .map_err(|e| e.to_string())
            .and_then(|body| self.post.post(body));
        let mut st = self.state.lock().unwrap();
        let resp = match res {
            Ok(r) => r,
            Err(e) => {
                st.2.failures += 1;
                st.2.last_error = e.clone();
                return Err(e);
            }
        };
        st.2.last_error.clear();
        if !synced {
            // First contact: resume after whatever the API already holds.
            st.0 = resp.last_t;
            st.1 = true;
            return Ok(true);
        }
        if resp.gap {
            // The API is missing seconds before this batch (it restarted
            // with an empty store): replay from what it has.
            st.0 = resp.prev_last_t;
            st.2.replays += 1;
            return Ok(true);
        }
        st.2.sent_batches += 1;
        st.2.sent_points += npts as u64;
        st.2.last_sent_at = unix_now();
        st.0 = b.to;
        Ok(self.db.last_t() > b.to)
    }

    /// One tick: a batch plus up to `catch_up` more while a backlog remains.
    pub fn tick(&self, catch_up: usize) -> Result<(), String> {
        for _ in 0..=catch_up {
            if !self.once()? {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsdb::Series;
    use crate::wire::{decode, WireSeries, MAX_DECODED};

    #[test]
    fn a_full_batch_fits_the_decode_limit() {
        let labels: std::collections::BTreeMap<String, String> = [
            ("namespace", "kube-system-long-namespace"),
            ("pod", "cilium-operator-5f985f674d-nlpr8"),
            ("container_id", "0123456789ab"),
            ("workload_kind", "Deployment"),
            ("workload", "cilium-operator"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let per = 20;
        let series = (0..DEFAULT_MAX_POINTS / per)
            .map(|i| WireSeries {
                series: Series {
                    context: "cgroup.mem".into(),
                    chart: format!("cgroup_kube-system_cilium-operator-{i}.mem"),
                    dimension: "anon".into(),
                    family: "mem".into(),
                    units: "MiB".into(),
                    title: "Workload memory breakdown".into(),
                    chart_type: "stacked".into(),
                    labels: labels.clone(),
                },
                points: (0..per)
                    .map(|j| [1_791_134_041.0 + j as f64, 123.456_789_012 + i as f64, 0.0])
                    .collect(),
            })
            .collect();
        let b = Batch {
            node: "nldw4-4-04-32".into(),
            from: 1,
            to: 2,
            series,
        };
        let json = serde_json::to_vec(&b).unwrap().len() as u64;
        assert!(json < MAX_DECODED / 2, "{json} bytes");
    }

    struct Direct(Arc<Hub>);

    impl Post for Direct {
        fn post(&self, body: Vec<u8>) -> Result<Response, String> {
            let b = decode(&body, true)?;
            self.0.ingest(&b).map_err(|e| e.to_string())
        }
    }

    fn agent_db(from: i64, to: i64) -> Arc<Db> {
        let db = Db::open(DbOptions::default()).unwrap();
        let s = Series {
            context: "system.cpu".into(),
            chart: "system.cpu".into(),
            dimension: "user".into(),
            ..Default::default()
        };
        for t in from..to {
            db.append(&Sample {
                series: s.clone(),
                t,
                v: t as f64,
                a: false,
            })
            .unwrap();
        }
        Arc::new(db)
    }

    #[test]
    fn node_names() {
        assert!(valid_node("ip-10-0-0-1.ec2.internal"));
        for bad in ["", "-x", "a/b", "a..b", "a b"] {
            assert!(!valid_node(bad), "{bad}");
        }
    }

    #[test]
    fn sync_catch_up_and_replay_after_hub_restart() {
        let db = agent_db(1000, 1100);
        let hub = Arc::new(Hub::open(HubOptions::default()).unwrap());
        let s = Sender::new(db.clone(), "n1", Box::new(Direct(hub.clone())), 30);
        s.tick(10).unwrap();
        assert_eq!(hub.db("n1").unwrap().last_t(), 1099);
        assert!(
            s.status().sent_batches >= 3,
            "max_points splits the backlog"
        );

        // Hub restarts empty; agent has newer data. The first send after the
        // restart reports a gap and the agent replays its whole buffer.
        let hub2 = Arc::new(Hub::open(HubOptions::default()).unwrap());
        let s2 = Sender::new(db.clone(), "n1", Box::new(Direct(hub2.clone())), 0);
        s2.once().unwrap(); // sync: hub2 holds nothing
        let d = hub2.db("n1").unwrap();
        assert_eq!(d.last_t(), 0);
        s2.tick(5).unwrap();
        assert_eq!(hub2.db("n1").unwrap().last_t(), 1099);
        assert_eq!(hub2.nodes()[0].node, "n1");
    }

    #[test]
    fn gap_is_reported_and_node_limit_enforced() {
        let hub = Hub::open(HubOptions {
            max_nodes: 1,
            ..Default::default()
        })
        .unwrap();
        let db = agent_db(1000, 1010);
        let mk = |node: &str, from: i64| Batch {
            node: node.into(),
            from,
            to: 1009,
            series: from_series_points(db.since(from, 1000).0),
        };
        let r = hub.ingest(&mk("n1", 0)).unwrap();
        assert_eq!(r.last_t, 1009);
        let db2 = agent_db(1020, 1030);
        let gap = Batch {
            node: "n1".into(),
            from: 1015,
            to: 1029,
            series: from_series_points(db2.since(1015, 1000).0),
        };
        let r = hub.ingest(&gap).unwrap();
        assert!(r.gap);
        assert_eq!(r.prev_last_t, 1009);
        assert_eq!(
            hub.ingest(&mk("n2", 0)).unwrap_err(),
            IngestError::NodeLimit
        );
        assert!(matches!(
            hub.ingest(&mk("../x", 0)),
            Err(IngestError::Invalid(_))
        ));
    }

    #[test]
    fn hub_reopens_node_dirs() {
        let d = tempfile::tempdir().unwrap();
        let opts = || HubOptions {
            dir: Some(d.path().to_path_buf()),
            ..Default::default()
        };
        let hub = Hub::open(opts()).unwrap();
        let db = agent_db(1000, 1010);
        hub.ingest(&Batch {
            node: "n1".into(),
            from: 0,
            to: 1009,
            series: from_series_points(db.since(0, 100).0),
        })
        .unwrap();
        hub.close();
        let hub = Hub::open(opts()).unwrap();
        assert!(hub.db("n1").is_some());
    }
}
