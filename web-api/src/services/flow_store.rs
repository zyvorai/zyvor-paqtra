//! Durable flow index (SQLite when PAQTRA_DATA_DIR is set, else in-memory).
//!
//! Stores Hubble flow metadata for investigation and policy preview.
//! Never stores payloads, argv, or secrets.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration as ChronoDuration, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::models::flow::Flow;

pub const DEFAULT_RETENTION_DAYS: i64 = 7;
const MAX_MEMORY_FLOWS: usize = 50_000;
/// Expired rows deleted per purge, under the writer lock. Each row touches four
/// b-trees; on a cold spinning disk that is ~40 random reads per row, so a
/// chunk must stay small or ingest stalls behind it.
const PURGE_CHUNK: i64 = 500;
/// Minimum pause between chunks while a backlog drains, so ingest gets the lock.
const PURGE_PAUSE: Duration = Duration::from_secs(2);
/// Pause after a chunk as a multiple of the time it took: the purge gets at
/// most a fifth of the disk, whose random-read budget it otherwise saturates.
const PURGE_DUTY: u32 = 4;
/// Pause once nothing is left to delete.
const PURGE_IDLE: Duration = Duration::from_secs(600);
/// A read that runs longer than this is cut off. The store is one SQLite file
/// holding millions of rows; a filter with no usable index (a namespace on the
/// destination side, a pod-name LIKE) is a full scan, and without a limit it
/// ran for minutes and starved the API. Override: PAQTRA_FLOW_QUERY_TIMEOUT_SECS.
const READ_DEADLINE_SECS: u64 = 5;
/// Tables this small are counted exactly; larger ones by rowid span, which is
/// O(1) (COUNT(*) over millions of rows takes seconds).
const EXACT_COUNT_BELOW: i64 = 100_000;

/// Provenance of a stored flow row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowSource {
    HubbleCli,
    HubbleGrpc,
    Unavailable,
}

impl FlowSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HubbleCli => "hubble_cli",
            Self::HubbleGrpc => "hubble_grpc",
            Self::Unavailable => "unavailable",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "hubble_grpc" => Self::HubbleGrpc,
            "unavailable" => Self::Unavailable,
            _ => Self::HubbleCli,
        }
    }
}

/// Indexed flow row with investigation fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredFlow {
    pub id: String,
    pub ts: String,
    pub cluster: String,
    pub verdict: String,
    pub drop_reason: String,
    pub protocol: String,
    pub port: u16,
    pub src_namespace: String,
    pub src_pod: String,
    pub src_ip: String,
    pub src_identity: i64,
    pub dst_namespace: String,
    pub dst_pod: String,
    pub dst_ip: String,
    pub dst_identity: i64,
    pub source: FlowSource,
}

/// Fixed-width UTC timestamp (`2026-09-24T04:34:47.290678Z`). Stored and queried
/// in this one form so that comparing timestamps as text is the same as
/// comparing them in time; mixed offsets and fraction lengths would not be.
pub fn format_ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Micros, true)
}

pub fn now_ts() -> String {
    format_ts(Utc::now())
}

/// Parse any RFC 3339 timestamp into the stored form.
pub fn normalize_ts(s: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|t| format_ts(t.with_timezone(&Utc)))
}

impl StoredFlow {
    pub fn from_flow(flow: &Flow, source: FlowSource, drop_reason: &str) -> Self {
        Self {
            id: flow.id.clone(),
            ts: normalize_ts(&flow.timestamp).unwrap_or_else(now_ts),
            cluster: flow.cluster.clone().unwrap_or_else(|| "local".into()),
            verdict: flow.verdict.clone(),
            drop_reason: drop_reason.to_string(),
            protocol: flow.protocol.clone(),
            port: flow.port,
            src_namespace: flow.source.namespace.clone(),
            src_pod: flow.source.pod.clone(),
            src_ip: flow.source.ip.clone(),
            src_identity: flow
                .hubble
                .as_ref()
                .and_then(|m| m.source_identity)
                .map_or(0, i64::from),
            dst_namespace: flow.destination.namespace.clone(),
            dst_pod: flow.destination.pod.clone(),
            dst_ip: flow.destination.ip.clone(),
            dst_identity: flow
                .hubble
                .as_ref()
                .and_then(|m| m.destination_identity)
                .map_or(0, i64::from),
            source,
        }
    }

    /// Convert a stored row back into the API `Flow` shape for list/stats UIs.
    pub fn into_flow(self) -> Flow {
        use crate::models::flow::FlowEndpoint;
        Flow {
            id: self.id,
            timestamp: self.ts,
            source: FlowEndpoint {
                namespace: self.src_namespace,
                pod: self.src_pod,
                ip: self.src_ip,
            },
            destination: FlowEndpoint {
                namespace: self.dst_namespace,
                pod: self.dst_pod,
                ip: self.dst_ip,
            },
            verdict: self.verdict,
            protocol: self.protocol,
            port: self.port,
            cluster: Some(self.cluster),
            drop_reason: if self.drop_reason.is_empty() {
                None
            } else {
                Some(self.drop_reason)
            },
            // Only identities are persisted; the rest of the Hubble context is not.
            hubble: crate::models::flow::FlowMeta {
                source_identity: u32::try_from(self.src_identity).ok().filter(|&i| i != 0),
                destination_identity: u32::try_from(self.dst_identity).ok().filter(|&i| i != 0),
                ..Default::default()
            }
            .or_none(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FlowQuery {
    pub src_namespace: Option<String>,
    pub src_pod: Option<String>,
    pub dst_namespace: Option<String>,
    pub dst_pod: Option<String>,
    /// Matches when either side is in this namespace.
    pub namespace: Option<String>,
    /// Matches when either side's pod name contains this text.
    pub pod: Option<String>,
    pub port: Option<u16>,
    pub verdict: Option<String>,
    /// Inclusive lower bound, in the stored timestamp form.
    pub since_rfc3339: Option<String>,
    /// Exclusive upper bound, in the stored timestamp form.
    pub until_rfc3339: Option<String>,
    /// When set, only flows with a side in one of these namespaces. This is a
    /// caller's namespace limit; applying it here keeps counts and paging right.
    pub scope: Option<Vec<String>>,
    pub limit: usize,
    pub offset: usize,
}

/// A window at most this wide is read through the time index (see
/// `FlowQuery::table`); wider ones are left to the planner.
const TIME_INDEX_WINDOW_HOURS: i64 = 24;

impl FlowQuery {
    /// The `FROM` target. With a lower time bound of at most a day, read through
    /// `idx_flows_ts`. Left alone, SQLite has no statistics here and prefers
    /// `idx_flows_path` whenever a namespace is given: for a busy namespace that
    /// reads a large slice of the table and sorts it, ignoring the time bound, so
    /// a two-minute question costs seconds on a store of millions of rows. The
    /// time index makes the cost track the window, whatever the filters.
    fn table(&self) -> &'static str {
        let narrow = self.since_rfc3339.as_deref().is_some_and(|since| {
            let parse = |v: &str| chrono::DateTime::parse_from_rfc3339(v).ok();
            let Some(from) = parse(since) else {
                return false;
            };
            let to = self
                .until_rfc3339
                .as_deref()
                .and_then(parse)
                .map(|t| t.with_timezone(&Utc))
                .unwrap_or_else(Utc::now);
            to - from.with_timezone(&Utc) <= ChronoDuration::hours(TIME_INDEX_WINDOW_HOURS)
        });
        if narrow {
            "flows INDEXED BY idx_flows_ts"
        } else {
            "flows"
        }
    }
}

/// Escape `\`, `%` and `_` so user text matches literally in a LIKE.
fn like_contains(v: &str) -> String {
    let mut out = String::with_capacity(v.len() + 2);
    out.push('%');
    for c in v.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

impl FlowQuery {
    /// SQL conditions (each starting with " AND") and their bound values.
    fn where_sql(&self) -> (String, Vec<Box<dyn rusqlite::types::ToSql>>) {
        let mut sql = String::new();
        let mut vals: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut eq = |col: &str, v: &Option<String>| {
            if let Some(v) = v {
                sql.push_str(&format!(" AND {col} = ?"));
                vals.push(Box::new(v.clone()));
            }
        };
        eq("src_namespace", &self.src_namespace);
        eq("dst_namespace", &self.dst_namespace);
        eq("verdict", &self.verdict);
        if let Some(v) = &self.src_pod {
            sql.push_str(" AND src_pod LIKE ? ESCAPE '\\'");
            vals.push(Box::new(like_contains(v)));
        }
        if let Some(v) = &self.dst_pod {
            sql.push_str(" AND dst_pod LIKE ? ESCAPE '\\'");
            vals.push(Box::new(like_contains(v)));
        }
        if let Some(v) = &self.namespace {
            sql.push_str(" AND (src_namespace = ? OR dst_namespace = ?)");
            vals.push(Box::new(v.clone()));
            vals.push(Box::new(v.clone()));
        }
        if let Some(v) = &self.pod {
            sql.push_str(" AND (src_pod LIKE ? ESCAPE '\\' OR dst_pod LIKE ? ESCAPE '\\')");
            vals.push(Box::new(like_contains(v)));
            vals.push(Box::new(like_contains(v)));
        }
        if let Some(p) = self.port {
            sql.push_str(" AND port = ?");
            vals.push(Box::new(p as i64));
        }
        if let Some(v) = &self.since_rfc3339 {
            sql.push_str(" AND ts >= ?");
            vals.push(Box::new(v.clone()));
        }
        if let Some(v) = &self.until_rfc3339 {
            sql.push_str(" AND ts < ?");
            vals.push(Box::new(v.clone()));
        }
        if let Some(scope) = &self.scope {
            if scope.is_empty() {
                // An empty scope list would mean "everything" elsewhere; here it
                // is a caller bug, so match nothing rather than everything.
                sql.push_str(" AND 0");
            } else {
                let marks = vec!["?"; scope.len()].join(",");
                sql.push_str(&format!(
                    " AND (src_namespace IN ({marks}) OR dst_namespace IN ({marks}))"
                ));
                for _ in 0..2 {
                    for ns in scope {
                        vals.push(Box::new(ns.clone()));
                    }
                }
            }
        }
        (sql, vals)
    }

    /// The same conditions for the in-memory store.
    fn matches(&self, f: &StoredFlow) -> bool {
        let eq = |want: &Option<String>, got: &str| want.as_deref().is_none_or(|w| w == got);
        let has =
            |want: &Option<String>, got: &str| want.as_deref().is_none_or(|w| got.contains(w));
        eq(&self.src_namespace, &f.src_namespace)
            && eq(&self.dst_namespace, &f.dst_namespace)
            && eq(&self.verdict, &f.verdict)
            && has(&self.src_pod, &f.src_pod)
            && has(&self.dst_pod, &f.dst_pod)
            && self
                .namespace
                .as_deref()
                .is_none_or(|n| f.src_namespace == n || f.dst_namespace == n)
            && self
                .pod
                .as_deref()
                .is_none_or(|p| f.src_pod.contains(p) || f.dst_pod.contains(p))
            && self.port.is_none_or(|p| f.port == p)
            && self
                .since_rfc3339
                .as_deref()
                .is_none_or(|s| f.ts.as_str() >= s)
            && self
                .until_rfc3339
                .as_deref()
                .is_none_or(|u| f.ts.as_str() < u)
            && self.scope.as_ref().is_none_or(|scope| {
                scope
                    .iter()
                    .any(|ns| *ns == f.src_namespace || *ns == f.dst_namespace)
            })
    }
}

/// Flows per time bucket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TimelineBucket {
    /// Start of the bucket, unix seconds.
    pub start: i64,
    pub forwarded: u64,
    pub dropped: u64,
    pub other: u64,
}

/// What the store holds, for telling a reader how far back the history goes.
#[derive(Debug, Clone, Serialize)]
pub struct Coverage {
    pub oldest: Option<String>,
    pub newest: Option<String>,
    pub total: u64,
    pub retention_days: i64,
    /// False when running in memory: history is lost on restart and capped.
    pub durable: bool,
}

#[derive(Debug, Default)]
pub struct FlowStoreStats {
    pub total: u64,
    pub last_ingest_ok: bool,
    pub last_ingest_at: Option<String>,
    pub last_ingest_count: u64,
    pub ingest_source: String,
    /// Flows Hubble returned without a usable timestamp, which are not stored.
    pub skipped_no_time: u64,
    pub stream_connected: bool,
    pub disconnect_count: u64,
    pub gap_count: u64,
    pub last_gap_at: Option<String>,
    pub events_per_sec: f64,
    pub lag_secs: Option<i64>,
}

pub struct FlowStore {
    /// Writer: ingest, purge. Never used for long reads.
    db: Option<Arc<Mutex<Connection>>>,
    /// A second connection to the same WAL database for reads, so a slow query
    /// cannot hold up ingest (or the reverse). `query_only`.
    read_db: Option<Arc<Mutex<Connection>>>,
    read_deadline: Duration,
    /// Last total `stats()` computed; returned instead of waiting when busy.
    total_hint: AtomicU64,
    /// Newest flow timestamp `lag_secs` last saw; used when the store is busy.
    newest_hint: Mutex<Option<String>>,
    memory: Mutex<Vec<StoredFlow>>,
    retention_days: i64,
    pub ingested_total: AtomicU64,
    skipped_no_time: AtomicU64,
    pub last_ingest_ok: AtomicU64, // 1 = ok, 0 = fail
    pub last_ingest_count: AtomicU64,
    last_ingest_at: Mutex<Option<String>>,
    ingest_source: Mutex<String>,
    stream_connected: AtomicU64,
    disconnect_count: AtomicU64,
    gap_count: AtomicU64,
    last_gap_at: Mutex<Option<String>>,
    gap_events: Mutex<Vec<String>>,
    rate_window: Mutex<Vec<i64>>,
}

impl FlowStore {
    /// In-memory only (no PAQTRA_DATA_DIR).
    pub fn memory_only() -> Self {
        Self {
            db: None,
            read_db: None,
            read_deadline: Duration::from_secs(READ_DEADLINE_SECS),
            total_hint: AtomicU64::new(0),
            newest_hint: Mutex::new(None),
            memory: Mutex::new(Vec::new()),
            retention_days: DEFAULT_RETENTION_DAYS,
            ingested_total: AtomicU64::new(0),
            skipped_no_time: AtomicU64::new(0),
            last_ingest_ok: AtomicU64::new(0),
            last_ingest_count: AtomicU64::new(0),
            last_ingest_at: Mutex::new(None),
            ingest_source: Mutex::new("unavailable".into()),
            stream_connected: AtomicU64::new(0),
            disconnect_count: AtomicU64::new(0),
            gap_count: AtomicU64::new(0),
            last_gap_at: Mutex::new(None),
            gap_events: Mutex::new(Vec::new()),
            rate_window: Mutex::new(Vec::new()),
        }
    }

    /// Open/create flows table in the same directory as the cache DB.
    pub fn open(data_dir: &Path, retention_days: i64) -> Result<Self> {
        std::fs::create_dir_all(data_dir)
            .with_context(|| format!("create data dir {}", data_dir.display()))?;
        let path: PathBuf = data_dir.join("flows.db");
        let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS flows (
                id TEXT NOT NULL,
                ts TEXT NOT NULL,
                cluster TEXT NOT NULL DEFAULT 'local',
                verdict TEXT NOT NULL,
                drop_reason TEXT NOT NULL DEFAULT '',
                protocol TEXT NOT NULL,
                port INTEGER NOT NULL,
                src_namespace TEXT NOT NULL DEFAULT '',
                src_pod TEXT NOT NULL DEFAULT '',
                src_ip TEXT NOT NULL DEFAULT '',
                src_identity INTEGER NOT NULL DEFAULT 0,
                dst_namespace TEXT NOT NULL DEFAULT '',
                dst_pod TEXT NOT NULL DEFAULT '',
                dst_ip TEXT NOT NULL DEFAULT '',
                dst_identity INTEGER NOT NULL DEFAULT 0,
                source TEXT NOT NULL DEFAULT 'hubble_cli',
                PRIMARY KEY (id, ts)
            );
            CREATE INDEX IF NOT EXISTS idx_flows_ts ON flows(ts DESC);
            CREATE INDEX IF NOT EXISTS idx_flows_path ON flows(src_namespace, src_pod, dst_namespace, dst_pod, port);
            CREATE INDEX IF NOT EXISTS idx_flows_verdict ON flows(verdict);
            ",
        )?;
        tracing::info!("FlowStore ready at {}", path.display());
        let read_conn = Connection::open(&path)
            .with_context(|| format!("open {} for reading", path.display()))?;
        read_conn.pragma_update(None, "query_only", true)?;
        let deadline_secs = std::env::var("PAQTRA_FLOW_QUERY_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|s| *s > 0)
            .unwrap_or(READ_DEADLINE_SECS);
        let store = Self {
            db: Some(Arc::new(Mutex::new(conn))),
            read_db: Some(Arc::new(Mutex::new(read_conn))),
            read_deadline: Duration::from_secs(deadline_secs),
            total_hint: AtomicU64::new(0),
            newest_hint: Mutex::new(None),
            memory: Mutex::new(Vec::new()),
            retention_days,
            ingested_total: AtomicU64::new(0),
            skipped_no_time: AtomicU64::new(0),
            last_ingest_ok: AtomicU64::new(0),
            last_ingest_count: AtomicU64::new(0),
            last_ingest_at: Mutex::new(None),
            ingest_source: Mutex::new("unavailable".into()),
            stream_connected: AtomicU64::new(0),
            disconnect_count: AtomicU64::new(0),
            gap_count: AtomicU64::new(0),
            last_gap_at: Mutex::new(None),
            gap_events: Mutex::new(Vec::new()),
            rate_window: Mutex::new(Vec::new()),
        };
        Ok(store)
    }

    /// Drains expired rows in the background, a chunk at a time. Retention is
    /// never enforced on the startup or ingest path: a multi-GB backlog would
    /// keep the API from binding its port. Stops when the store is dropped.
    pub fn spawn_purger(self: &Arc<Self>) {
        if self.db.is_none() {
            return;
        }
        let weak = Arc::downgrade(self);
        let spawned = std::thread::Builder::new()
            .name("flow-purge".into())
            .spawn(move || loop {
                let Some(store) = weak.upgrade() else { return };
                let started = Instant::now();
                let pause = match store.purge_expired() {
                    Ok(n) if n as i64 >= PURGE_CHUNK => {
                        PURGE_PAUSE.max(started.elapsed() * PURGE_DUTY)
                    }
                    Ok(_) => PURGE_IDLE,
                    Err(e) => {
                        tracing::warn!("flow retention purge: {e}");
                        PURGE_IDLE
                    }
                };
                drop(store);
                std::thread::sleep(pause);
            });
        if let Err(e) = spawned {
            tracing::warn!("flow retention purge not started: {e}");
        }
    }

    /// The connection reads use: the read-only one, else the writer.
    fn read_conn(&self) -> Option<&Arc<Mutex<Connection>>> {
        self.read_db.as_ref().or(self.db.as_ref())
    }

    /// Run a read under [`READ_DEADLINE_SECS`]: SQLite is told to abandon the
    /// statement once it has run too long, so the lock is never held for minutes.
    fn read<T>(
        &self,
        db: &Arc<Mutex<Connection>>,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T> {
        let conn = db
            .lock()
            .map_err(|_| anyhow::anyhow!("flow db lock poisoned"))?;
        let started = Instant::now();
        let limit = self.read_deadline;
        conn.progress_handler(10_000, Some(move || started.elapsed() > limit))?;
        let result = f(&conn);
        // Clearing the handler is cleanup: a failure here must not mask the result.
        let _ = conn.progress_handler(0, None::<fn() -> bool>);
        result.map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::OperationInterrupted =>
            {
                anyhow::anyhow!(
                    "flow query exceeded {}s; narrow it with a time range (from/to) or a pod",
                    limit.as_secs().max(1)
                )
            }
            other => other.into(),
        })
    }

    pub fn stats(&self) -> FlowStoreStats {
        // Called on every request, so it must be cheap and must never wait behind
        // a long read: when the store is busy the last known total is returned.
        let total = if let Some(db) = self.read_conn() {
            match db.try_lock() {
                Ok(conn) => match fast_total(&conn) {
                    Ok(n) => {
                        self.total_hint.store(n as u64, Ordering::Relaxed);
                        n as u64
                    }
                    Err(_) => self.total_hint.load(Ordering::Relaxed),
                },
                Err(_) => self.total_hint.load(Ordering::Relaxed),
            }
        } else {
            self.memory.lock().map(|m| m.len() as u64).unwrap_or(0)
        };
        FlowStoreStats {
            total,
            last_ingest_ok: self.last_ingest_ok.load(Ordering::Relaxed) == 1,
            last_ingest_at: self.last_ingest_at.lock().ok().and_then(|g| g.clone()),
            last_ingest_count: self.last_ingest_count.load(Ordering::Relaxed),
            skipped_no_time: self.skipped_no_time.load(Ordering::Relaxed),
            ingest_source: self
                .ingest_source
                .lock()
                .map(|g| g.clone())
                .unwrap_or_else(|_| "unavailable".into()),
            stream_connected: self.stream_connected.load(Ordering::Relaxed) == 1,
            disconnect_count: self.disconnect_count.load(Ordering::Relaxed),
            gap_count: self.gap_count.load(Ordering::Relaxed),
            last_gap_at: self.last_gap_at.lock().ok().and_then(|g| g.clone()),
            events_per_sec: self.events_per_sec(),
            lag_secs: self.lag_secs(),
        }
    }

    fn events_per_sec(&self) -> f64 {
        let now = Utc::now().timestamp();
        let Ok(mut w) = self.rate_window.lock() else {
            return 0.0;
        };
        w.retain(|t| now - *t <= 60);
        if w.is_empty() {
            0.0
        } else {
            w.len() as f64 / 60.0
        }
    }

    /// Newest stored timestamp, without ever waiting for the store: an index
    /// lookup when it is free, the last known value when it is busy.
    fn newest_ts(&self) -> Option<String> {
        let Some(db) = self.read_conn() else {
            return self.coverage(None).ok()?.newest;
        };
        if let Ok(conn) = db.try_lock() {
            if let Ok(v) = conn.query_row("SELECT MAX(ts) FROM flows", [], |r| {
                r.get::<_, Option<String>>(0)
            }) {
                if let Ok(mut hint) = self.newest_hint.lock() {
                    *hint = v.clone();
                }
                return v;
            }
        }
        self.newest_hint.lock().ok().and_then(|h| h.clone())
    }

    fn lag_secs(&self) -> Option<i64> {
        let newest = self.newest_ts()?;
        let ts = DateTime::parse_from_rfc3339(&newest)
            .ok()?
            .with_timezone(&Utc);
        Some((Utc::now() - ts).num_seconds().max(0))
    }

    pub fn set_stream_connected(&self, connected: bool) {
        self.stream_connected
            .store(if connected { 1 } else { 0 }, Ordering::Relaxed);
    }

    pub fn record_stream_gap(&self) {
        self.disconnect_count.fetch_add(1, Ordering::Relaxed);
        self.gap_count.fetch_add(1, Ordering::Relaxed);
        self.stream_connected.store(0, Ordering::Relaxed);
        let at = Utc::now().to_rfc3339();
        if let Ok(mut g) = self.last_gap_at.lock() {
            *g = Some(at.clone());
        }
        if let Ok(mut ring) = self.gap_events.lock() {
            ring.push(at);
            if ring.len() > 64 {
                let drain = ring.len() - 64;
                ring.drain(0..drain);
            }
        }
    }

    pub fn recent_gaps(&self) -> Vec<String> {
        self.gap_events
            .lock()
            .ok()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn retention_days(&self) -> i64 {
        self.retention_days
    }

    /// Delete flows older than `days` (defaults to store retention), optional namespace scope.
    pub fn purge(&self, namespace: Option<&str>, older_than_days: Option<i64>) -> Result<usize> {
        let days = older_than_days.unwrap_or(self.retention_days).max(0);
        let cutoff = format_ts(Utc::now() - ChronoDuration::days(days));
        if let Some(db) = &self.db {
            let conn = db
                .lock()
                .map_err(|_| anyhow::anyhow!("flow db lock poisoned"))?;
            let n = if let Some(ns) = namespace.filter(|s| !s.is_empty()) {
                conn.execute(
                    "DELETE FROM flows WHERE ts < ?1 AND (src_namespace = ?2 OR dst_namespace = ?2)",
                    params![cutoff, ns],
                )?
            } else {
                conn.execute("DELETE FROM flows WHERE ts < ?1", params![cutoff])?
            };
            return Ok(n);
        }
        let mut mem = self
            .memory
            .lock()
            .map_err(|_| anyhow::anyhow!("flow memory lock poisoned"))?;
        let before = mem.len();
        mem.retain(|f| {
            if f.ts >= cutoff {
                return true;
            }
            match namespace.filter(|s| !s.is_empty()) {
                Some(ns) => !(f.src_namespace == ns || f.dst_namespace == ns),
                None => false,
            }
        });
        Ok(before.saturating_sub(mem.len()))
    }

    pub fn note_stream_event(&self) {
        let now = Utc::now().timestamp();
        if let Ok(mut w) = self.rate_window.lock() {
            w.push(now);
            w.retain(|t| now - *t <= 60);
        }
    }

    /// Count flows that were left out for lack of a timestamp. Returns the total
    /// before this call, so a caller can log only the first time.
    pub fn note_skipped_no_time(&self, n: u64) -> u64 {
        self.skipped_no_time.fetch_add(n, Ordering::Relaxed)
    }

    pub fn record_ingest(&self, ok: bool, count: u64, source: FlowSource) {
        self.last_ingest_ok
            .store(if ok { 1 } else { 0 }, Ordering::Relaxed);
        self.last_ingest_count.store(count, Ordering::Relaxed);
        self.ingested_total.fetch_add(count, Ordering::Relaxed);
        if let Ok(mut g) = self.last_ingest_at.lock() {
            *g = Some(Utc::now().to_rfc3339());
        }
        if let Ok(mut g) = self.ingest_source.lock() {
            *g = source.as_str().to_string();
        }
    }

    pub fn insert_batch(&self, rows: &[StoredFlow]) -> Result<usize> {
        if rows.is_empty() {
            return Ok(0);
        }
        if let Some(db) = &self.db {
            let conn = db
                .lock()
                .map_err(|_| anyhow::anyhow!("flow db lock poisoned"))?;
            let tx = conn.unchecked_transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT OR REPLACE INTO flows (
                        id, ts, cluster, verdict, drop_reason, protocol, port,
                        src_namespace, src_pod, src_ip, src_identity,
                        dst_namespace, dst_pod, dst_ip, dst_identity, source
                    ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                )?;
                for r in rows {
                    stmt.execute(params![
                        r.id,
                        r.ts,
                        r.cluster,
                        r.verdict,
                        r.drop_reason,
                        r.protocol,
                        r.port as i64,
                        r.src_namespace,
                        r.src_pod,
                        r.src_ip,
                        r.src_identity,
                        r.dst_namespace,
                        r.dst_pod,
                        r.dst_ip,
                        r.dst_identity,
                        r.source.as_str(),
                    ])?;
                }
            }
            tx.commit()?;
            return Ok(rows.len());
        }

        let mut mem = self
            .memory
            .lock()
            .map_err(|_| anyhow::anyhow!("flow memory lock poisoned"))?;
        for r in rows {
            if let Some(pos) = mem.iter().position(|x| x.id == r.id && x.ts == r.ts) {
                mem[pos] = r.clone();
            } else {
                mem.push(r.clone());
            }
        }
        if mem.len() > MAX_MEMORY_FLOWS {
            let drop_n = mem.len() - MAX_MEMORY_FLOWS;
            mem.drain(0..drop_n);
        }
        Ok(rows.len())
    }

    pub fn purge_expired(&self) -> Result<usize> {
        let cutoff = format_ts(Utc::now() - ChronoDuration::days(self.retention_days));
        if let Some(db) = &self.db {
            let conn = db
                .lock()
                .map_err(|_| anyhow::anyhow!("flow db lock poisoned"))?;
            let n = conn.execute(
                "DELETE FROM flows WHERE rowid IN
                    (SELECT rowid FROM flows INDEXED BY idx_flows_ts WHERE ts < ?1 LIMIT ?2)",
                params![cutoff, PURGE_CHUNK],
            )?;
            return Ok(n);
        }
        let mut mem = self
            .memory
            .lock()
            .map_err(|_| anyhow::anyhow!("flow memory lock poisoned"))?;
        let before = mem.len();
        mem.retain(|f| f.ts >= cutoff);
        Ok(before.saturating_sub(mem.len()))
    }

    pub fn query(&self, q: &FlowQuery) -> Result<Vec<StoredFlow>> {
        let limit = q.limit.clamp(1, 5_000);
        let offset = q.offset;

        if let Some(db) = self.read_conn() {
            let (conds, mut vals) = q.where_sql();
            let sql = format!(
                "SELECT id, ts, cluster, verdict, drop_reason, protocol, port,
                        src_namespace, src_pod, src_ip, src_identity,
                        dst_namespace, dst_pod, dst_ip, dst_identity, source
                 FROM {} WHERE 1=1{conds} ORDER BY ts DESC LIMIT ? OFFSET ?",
                q.table()
            );
            vals.push(Box::new(limit as i64));
            vals.push(Box::new(offset as i64));
            return self.read(db, |conn| {
                let mut stmt = conn.prepare(&sql)?;
                let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                    vals.iter().map(|b| b.as_ref()).collect();
                let rows = stmt.query_map(params_ref.as_slice(), row_to_stored)?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            });
        }

        let mem = self
            .memory
            .lock()
            .map_err(|_| anyhow::anyhow!("flow memory lock poisoned"))?;
        let mut filtered: Vec<_> = mem.iter().filter(|f| q.matches(f)).cloned().collect();
        filtered.sort_by(|a, b| b.ts.cmp(&a.ts));
        Ok(filtered.into_iter().skip(offset).take(limit).collect())
    }

    /// How many stored flows match, ignoring limit and offset.
    pub fn count(&self, q: &FlowQuery) -> Result<u64> {
        if let Some(db) = self.read_conn() {
            let (conds, vals) = q.where_sql();
            let sql = format!("SELECT COUNT(*) FROM {} WHERE 1=1{conds}", q.table());
            let n = self.read(db, |conn| {
                let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                    vals.iter().map(|b| b.as_ref()).collect();
                conn.query_row(&sql, params_ref.as_slice(), |r| r.get::<_, i64>(0))
            })?;
            return Ok(n as u64);
        }
        let mem = self
            .memory
            .lock()
            .map_err(|_| anyhow::anyhow!("flow memory lock poisoned"))?;
        Ok(mem.iter().filter(|f| q.matches(f)).count() as u64)
    }

    /// Flow counts per `bucket_secs` window, oldest first. Only windows that
    /// contain flows are returned; the caller fills the gaps. Ignores limit and
    /// offset.
    pub fn timeline(&self, q: &FlowQuery, bucket_secs: i64) -> Result<Vec<TimelineBucket>> {
        let bucket_secs = bucket_secs.max(1);
        if let Some(db) = self.read_conn() {
            let (conds, mut vals) = q.where_sql();
            // strftime('%s') is unix seconds; integer division floors to the bucket.
            let sql = format!(
                "SELECT CAST(strftime('%s', ts) AS INTEGER) / ? * ? AS b,
                        SUM(verdict = 'FORWARDED'), SUM(verdict = 'DROPPED'), COUNT(*)
                 FROM {} WHERE strftime('%s', ts) IS NOT NULL{conds}
                 GROUP BY b ORDER BY b",
                q.table()
            );
            let mut all: Vec<Box<dyn rusqlite::types::ToSql>> =
                vec![Box::new(bucket_secs), Box::new(bucket_secs)];
            all.append(&mut vals);
            return self.read(db, |conn| {
                let mut stmt = conn.prepare(&sql)?;
                let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                    all.iter().map(|b| b.as_ref()).collect();
                let rows = stmt.query_map(params_ref.as_slice(), |r| {
                    let fwd: i64 = r.get(1)?;
                    let drp: i64 = r.get(2)?;
                    let all: i64 = r.get(3)?;
                    Ok(TimelineBucket {
                        start: r.get(0)?,
                        forwarded: fwd as u64,
                        dropped: drp as u64,
                        other: (all - fwd - drp).max(0) as u64,
                    })
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            });
        }

        let mem = self
            .memory
            .lock()
            .map_err(|_| anyhow::anyhow!("flow memory lock poisoned"))?;
        let mut buckets: std::collections::BTreeMap<i64, TimelineBucket> = Default::default();
        for f in mem.iter().filter(|f| q.matches(f)) {
            let Ok(t) = DateTime::parse_from_rfc3339(&f.ts) else {
                continue;
            };
            let start = t.timestamp().div_euclid(bucket_secs) * bucket_secs;
            let b = buckets.entry(start).or_insert(TimelineBucket {
                start,
                forwarded: 0,
                dropped: 0,
                other: 0,
            });
            match f.verdict.as_str() {
                "FORWARDED" => b.forwarded += 1,
                "DROPPED" => b.dropped += 1,
                _ => b.other += 1,
            }
        }
        Ok(buckets.into_values().collect())
    }

    /// What the store holds. With `scope`, only flows with a side in those
    /// namespaces are counted, so a limited caller learns nothing about the rest.
    pub fn coverage(&self, scope: Option<&[String]>) -> Result<Coverage> {
        let q = FlowQuery {
            scope: scope.map(|s| s.to_vec()),
            ..Default::default()
        };
        if let Some(db) = self.read_conn() {
            let (conds, vals) = q.where_sql();
            let unscoped = scope.is_none();
            let (oldest, newest, total) = self.read(db, |conn| {
                if unscoped {
                    // The whole table: MIN/MAX(ts) are index lookups and the
                    // total is the cheap estimate, not a scan of every row.
                    let (oldest, newest): (Option<String>, Option<String>) =
                        conn.query_row("SELECT MIN(ts), MAX(ts) FROM flows", [], |r| {
                            Ok((r.get(0)?, r.get(1)?))
                        })?;
                    return Ok((oldest, newest, fast_total(conn)?));
                }
                let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                    vals.iter().map(|b| b.as_ref()).collect();
                conn.query_row(
                    &format!("SELECT MIN(ts), MAX(ts), COUNT(*) FROM flows WHERE 1=1{conds}"),
                    params_ref.as_slice(),
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
            })?;
            return Ok(Coverage {
                oldest,
                newest,
                total: total as u64,
                retention_days: self.retention_days,
                durable: true,
            });
        }
        let mem = self
            .memory
            .lock()
            .map_err(|_| anyhow::anyhow!("flow memory lock poisoned"))?;
        let visible: Vec<&StoredFlow> = mem.iter().filter(|f| q.matches(f)).collect();
        Ok(Coverage {
            oldest: visible.iter().map(|f| f.ts.clone()).min(),
            newest: visible.iter().map(|f| f.ts.clone()).max(),
            total: visible.len() as u64,
            retention_days: self.retention_days,
            durable: false,
        })
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<StoredFlow>> {
        if let Some(db) = self.read_conn() {
            return self.read(db, |conn| {
                conn.query_row(
                    "SELECT id, ts, cluster, verdict, drop_reason, protocol, port,
                            src_namespace, src_pod, src_ip, src_identity,
                            dst_namespace, dst_pod, dst_ip, dst_identity, source
                     FROM flows WHERE id = ?1 ORDER BY ts DESC LIMIT 1",
                    params![id],
                    row_to_stored,
                )
                .optional()
            });
        }
        let mem = self
            .memory
            .lock()
            .map_err(|_| anyhow::anyhow!("flow memory lock poisoned"))?;
        Ok(mem.iter().rev().find(|f| f.id == id).cloned())
    }
}

/// Number of stored flows. Exact while the table is small; for a big one the
/// rowid span (`MAX - MIN + 1`, both O(log n)), which is accurate because rows
/// are appended and expired oldest-first.
fn fast_total(conn: &Connection) -> rusqlite::Result<i64> {
    let span: i64 = conn.query_row(
        "SELECT COALESCE(MAX(rowid) - MIN(rowid) + 1, 0) FROM flows",
        [],
        |r| r.get(0),
    )?;
    if span <= EXACT_COUNT_BELOW {
        conn.query_row("SELECT COUNT(*) FROM flows", [], |r| r.get(0))
    } else {
        Ok(span)
    }
}

fn row_to_stored(r: &rusqlite::Row<'_>) -> rusqlite::Result<StoredFlow> {
    Ok(StoredFlow {
        id: r.get(0)?,
        ts: r.get(1)?,
        cluster: r.get(2)?,
        verdict: r.get(3)?,
        drop_reason: r.get(4)?,
        protocol: r.get(5)?,
        port: r.get::<_, i64>(6)? as u16,
        src_namespace: r.get(7)?,
        src_pod: r.get(8)?,
        src_ip: r.get(9)?,
        src_identity: r.get(10)?,
        dst_namespace: r.get(11)?,
        dst_pod: r.get(12)?,
        dst_ip: r.get(13)?,
        dst_identity: r.get(14)?,
        source: FlowSource::parse(&r.get::<_, String>(15)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::flow::FlowEndpoint;

    fn sample(id: &str) -> StoredFlow {
        StoredFlow::from_flow(
            &Flow {
                id: id.into(),
                timestamp: Utc::now().to_rfc3339(),
                source: FlowEndpoint {
                    namespace: "shop".into(),
                    pod: "checkout-1".into(),
                    ip: "10.0.0.1".into(),
                },
                destination: FlowEndpoint {
                    namespace: "shop".into(),
                    pod: "payments-1".into(),
                    ip: "10.0.0.2".into(),
                },
                verdict: "DROPPED".into(),
                protocol: "TCP".into(),
                port: 443,
                http_method: None,
                http_url: None,
                http_code: None,
                cluster: Some("local".into()),
                ..Default::default()
            },
            FlowSource::HubbleCli,
            "Policy denied",
        )
    }

    #[test]
    fn identities_survive_the_stored_row() {
        let mut flow = sample("f-ident").into_flow();
        flow.hubble = Some(crate::models::flow::FlowMeta {
            source_identity: Some(4242),
            destination_identity: Some(7),
            node_name: Some("not-persisted".into()),
            ..Default::default()
        });
        let row = StoredFlow::from_flow(&flow, FlowSource::HubbleCli, "");
        assert_eq!((row.src_identity, row.dst_identity), (4242, 7));

        let meta = row.into_flow().hubble.expect("identities restored");
        assert_eq!(meta.source_identity, Some(4242));
        assert_eq!(meta.destination_identity, Some(7));
        assert_eq!(meta.node_name, None);
        // No identities: no object at all.
        assert!(sample("plain").into_flow().hubble.is_none());
    }

    #[test]
    fn memory_insert_and_query() {
        let store = FlowStore::memory_only();
        store.insert_batch(&[sample("f1")]).unwrap();
        let rows = store
            .query(&FlowQuery {
                src_namespace: Some("shop".into()),
                port: Some(443),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].verdict, "DROPPED");
    }

    #[test]
    fn sqlite_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "paqtra-flows-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = FlowStore::open(&dir, 7).unwrap();
        store.insert_batch(&[sample("f2")]).unwrap();
        let rows = store
            .query(&FlowQuery {
                dst_pod: Some("payments".into()),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(store.stats().total, 1);
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn purge_drains_expired_rows_a_chunk_at_a_time() {
        let dir = std::env::temp_dir().join(format!("paqtra-purge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = FlowStore::open(&dir, 7).unwrap();
        let old = format_ts(Utc::now() - ChronoDuration::days(30));
        {
            let mut conn = store.db.as_ref().unwrap().lock().unwrap();
            let tx = conn.transaction().unwrap();
            for i in 0..PURGE_CHUNK + 5 {
                tx.execute(
                    "INSERT INTO flows (id, ts, verdict, protocol, port) VALUES (?1, ?2, 'FORWARDED', 'TCP', 80)",
                    params![format!("old-{i}"), old],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }
        store.insert_batch(&[sample("fresh")]).unwrap();
        assert_eq!(store.purge_expired().unwrap(), PURGE_CHUNK as usize);
        assert_eq!(store.purge_expired().unwrap(), 5);
        assert_eq!(store.purge_expired().unwrap(), 0);
        let rows = store
            .query(&FlowQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "fresh");
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── history queries ─────────────────────────────────────────

    fn row(
        id: &str,
        ts: &str,
        src: (&str, &str),
        dst: (&str, &str),
        port: u16,
        verdict: &str,
    ) -> StoredFlow {
        StoredFlow {
            id: id.into(),
            ts: normalize_ts(ts).expect("test timestamp"),
            cluster: "local".into(),
            verdict: verdict.into(),
            drop_reason: String::new(),
            protocol: "TCP".into(),
            port,
            src_namespace: src.0.into(),
            src_pod: src.1.into(),
            src_ip: String::new(),
            src_identity: 0,
            dst_namespace: dst.0.into(),
            dst_pod: dst.1.into(),
            dst_ip: String::new(),
            dst_identity: 0,
            source: FlowSource::HubbleCli,
        }
    }

    /// The same data in a SQLite store and an in-memory store, so each check
    /// proves both implementations agree.
    fn stores(rows: &[StoredFlow]) -> Vec<(&'static str, FlowStore, Option<std::path::PathBuf>)> {
        let dir = std::env::temp_dir().join(format!(
            "paqtra-flowq-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let sqlite = FlowStore::open(&dir, 36500).unwrap();
        let memory = FlowStore::memory_only();
        sqlite.insert_batch(rows).unwrap();
        memory.insert_batch(rows).unwrap();
        vec![("sqlite", sqlite, Some(dir)), ("memory", memory, None)]
    }

    fn cleanup(all: Vec<(&'static str, FlowStore, Option<std::path::PathBuf>)>) {
        for (_, store, dir) in all {
            drop(store);
            if let Some(d) = dir {
                let _ = std::fs::remove_dir_all(d);
            }
        }
    }

    fn ids(store: &FlowStore, q: &FlowQuery) -> Vec<String> {
        let mut q = q.clone();
        q.limit = 1000;
        store.query(&q).unwrap().into_iter().map(|f| f.id).collect()
    }

    fn dataset() -> Vec<StoredFlow> {
        let t = |m: i64| (Utc::now() - ChronoDuration::minutes(m)).to_rfc3339();
        vec![
            row(
                "a",
                &t(50),
                ("shop", "web-1"),
                ("shop", "db-1"),
                5432,
                "FORWARDED",
            ),
            row(
                "b",
                &t(40),
                ("shop", "web-1"),
                ("pay", "gw-1"),
                443,
                "DROPPED",
            ),
            row(
                "c",
                &t(30),
                ("pay", "gw-1"),
                ("shop", "web-2"),
                8080,
                "FORWARDED",
            ),
            row(
                "d",
                &t(20),
                ("ops", "cron-1"),
                ("ops", "cron-2"),
                80,
                "FORWARDED",
            ),
            row("e", &t(10), ("", ""), ("pay", "gw-1"), 443, "DROPPED"),
        ]
    }

    #[test]
    fn timestamps_are_normalized_to_one_comparable_form() {
        for input in [
            "2026-09-24T04:34:47Z",
            "2026-09-24T04:34:47.290678123Z",
            "2026-09-24T10:04:47.29+05:30",
            "2026-09-24T04:34:47.290678+00:00",
        ] {
            let n = normalize_ts(input).unwrap();
            assert_eq!(
                n.len(),
                "2026-09-24T04:34:47.290678Z".len(),
                "{input} -> {n}"
            );
            assert!(n.ends_with('Z'));
        }
        assert_eq!(
            normalize_ts("2026-09-24T10:04:47.29+05:30").unwrap(),
            "2026-09-24T04:34:47.290000Z"
        );
        assert!(
            normalize_ts("").is_none()
                && normalize_ts("yesterday").is_none()
                && normalize_ts("2026-09-24").is_none()
        );
        // Text order is time order, even for inputs written with different offsets.
        let earlier = normalize_ts("2026-09-24T09:59:59+05:30").unwrap(); // 04:29:59Z
        let later = normalize_ts("2026-09-24T04:30:00Z").unwrap();
        assert!(earlier < later);
        // A flow with no usable time is stamped with the current time in the same form.
        let f = StoredFlow::from_flow(
            &sample("x").into_flow_for_test(""),
            FlowSource::HubbleCli,
            "",
        );
        assert!(normalize_ts(&f.ts).is_some());
    }

    trait IntoFlowForTest {
        fn into_flow_for_test(self, ts: &str) -> Flow;
    }
    impl IntoFlowForTest for StoredFlow {
        fn into_flow_for_test(self, ts: &str) -> Flow {
            Flow {
                id: self.id,
                timestamp: ts.into(),
                source: crate::models::flow::FlowEndpoint {
                    namespace: self.src_namespace,
                    pod: self.src_pod,
                    ip: self.src_ip,
                },
                destination: crate::models::flow::FlowEndpoint {
                    namespace: self.dst_namespace,
                    pod: self.dst_pod,
                    ip: self.dst_ip,
                },
                verdict: self.verdict,
                protocol: self.protocol,
                port: self.port,
                http_method: None,
                http_url: None,
                http_code: None,
                cluster: None,
                ..Default::default()
            }
        }
    }

    #[test]
    fn time_range_is_inclusive_below_and_exclusive_above() {
        let all = stores(&dataset());
        for (name, store, _) in &all {
            let rows = store
                .query(&FlowQuery {
                    limit: 100,
                    ..Default::default()
                })
                .unwrap();
            let by = |id: &str| rows.iter().find(|r| r.id == id).unwrap().ts.clone();
            let q = |since: Option<String>, until: Option<String>| FlowQuery {
                since_rfc3339: since,
                until_rfc3339: until,
                ..Default::default()
            };
            assert_eq!(
                ids(store, &q(Some(by("b")), Some(by("d")))),
                vec!["c", "b"],
                "{name}: [b, d)"
            );
            assert_eq!(
                ids(store, &q(Some(by("e")), None)),
                vec!["e"],
                "{name}: since is inclusive"
            );
            assert!(
                ids(store, &q(None, Some(by("a")))).is_empty(),
                "{name}: until is exclusive"
            );
            assert_eq!(
                ids(store, &q(None, None)),
                vec!["e", "d", "c", "b", "a"],
                "{name}: newest first"
            );
        }
        cleanup(all);
    }

    #[test]
    fn namespace_and_pod_match_either_side() {
        let all = stores(&dataset());
        for (name, store, _) in &all {
            let ns = |n: &str| FlowQuery {
                namespace: Some(n.into()),
                ..Default::default()
            };
            assert_eq!(ids(store, &ns("pay")), vec!["e", "c", "b"], "{name}");
            assert_eq!(ids(store, &ns("shop")), vec!["c", "b", "a"], "{name}");
            let pod = |p: &str| FlowQuery {
                pod: Some(p.into()),
                ..Default::default()
            };
            assert_eq!(ids(store, &pod("gw")), vec!["e", "c", "b"], "{name}");
            assert_eq!(ids(store, &pod("web-1")), vec!["b", "a"], "{name}");
            // Filters combine with AND.
            let both = FlowQuery {
                namespace: Some("pay".into()),
                verdict: Some("DROPPED".into()),
                port: Some(443),
                ..Default::default()
            };
            assert_eq!(ids(store, &both), vec!["e", "b"], "{name}");
        }
        cleanup(all);
    }

    #[test]
    fn scope_matches_either_side_and_keeps_count_and_paging_consistent() {
        let all = stores(&dataset());
        for (name, store, _) in &all {
            let scoped = |limit: usize, offset: usize| FlowQuery {
                scope: Some(vec!["shop".into()]),
                limit,
                offset,
                ..Default::default()
            };
            assert_eq!(
                ids(store, &scoped(1000, 0)),
                vec!["c", "b", "a"],
                "{name}: to or from shop"
            );
            assert_eq!(
                store.count(&scoped(1, 0)).unwrap(),
                3,
                "{name}: count ignores paging"
            );
            assert_eq!(store.query(&scoped(2, 0)).unwrap().len(), 2, "{name}");
            assert_eq!(
                store
                    .query(&scoped(2, 2))
                    .unwrap()
                    .iter()
                    .map(|f| f.id.as_str())
                    .collect::<Vec<_>>(),
                vec!["a"],
                "{name}: second page"
            );
            let two = FlowQuery {
                scope: Some(vec!["ops".into(), "pay".into()]),
                ..Default::default()
            };
            assert_eq!(ids(store, &two), vec!["e", "d", "c", "b"], "{name}");
            // A scope combined with a filter never widens it.
            let narrowed = FlowQuery {
                scope: Some(vec!["ops".into()]),
                namespace: Some("shop".into()),
                ..Default::default()
            };
            assert!(ids(store, &narrowed).is_empty(), "{name}");
            // An empty scope list matches nothing, not everything.
            let empty = FlowQuery {
                scope: Some(vec![]),
                ..Default::default()
            };
            assert!(
                ids(store, &empty).is_empty() && store.count(&empty).unwrap() == 0,
                "{name}"
            );
        }
        cleanup(all);
    }

    #[test]
    fn a_flow_with_no_namespace_on_either_side_is_never_in_scope() {
        let rows = vec![row(
            "w",
            &Utc::now().to_rfc3339(),
            ("", ""),
            ("", ""),
            53,
            "FORWARDED",
        )];
        let all = stores(&rows);
        for (name, store, _) in &all {
            assert!(
                ids(
                    store,
                    &FlowQuery {
                        scope: Some(vec!["shop".into()]),
                        ..Default::default()
                    }
                )
                .is_empty(),
                "{name}"
            );
            assert_eq!(
                ids(store, &FlowQuery::default()),
                vec!["w"],
                "{name}: visible without a scope"
            );
        }
        cleanup(all);
    }

    #[test]
    fn like_input_matches_literally() {
        let now = Utc::now().to_rfc3339();
        let rows = vec![
            row("p1", &now, ("n", "100%-done"), ("n", "x"), 1, "FORWARDED"),
            row("p2", &now, ("n", "a_b"), ("n", "x"), 1, "FORWARDED"),
            row("p3", &now, ("n", "axb"), ("n", "x"), 1, "FORWARDED"),
            row("p4", &now, ("n", "back\\slash"), ("n", "x"), 1, "FORWARDED"),
        ];
        let all = stores(&rows);
        for (name, store, _) in &all {
            let pod = |p: &str| {
                ids(
                    store,
                    &FlowQuery {
                        pod: Some(p.into()),
                        ..Default::default()
                    },
                )
            };
            assert_eq!(pod("%"), vec!["p1"], "{name}: % is not a wildcard");
            assert_eq!(pod("a_b"), vec!["p2"], "{name}: _ is not a wildcard");
            assert_eq!(pod("\\"), vec!["p4"], "{name}: backslash is literal");
            assert_eq!(
                ids(
                    store,
                    &FlowQuery {
                        src_pod: Some("_".into()),
                        ..Default::default()
                    }
                ),
                vec!["p2"],
                "{name}: src_pod escapes too"
            );
        }
        cleanup(all);
    }

    #[test]
    fn timeline_buckets_and_counts_verdicts() {
        let base = "2026-06-01T10:00:00Z";
        let at = |sec: i64| {
            (DateTime::parse_from_rfc3339(base).unwrap() + ChronoDuration::seconds(sec))
                .to_rfc3339()
        };
        let rows = vec![
            row("t1", &at(5), ("a", "p"), ("a", "q"), 1, "FORWARDED"),
            row("t2", &at(50), ("a", "p"), ("a", "q"), 1, "DROPPED"),
            row("t3", &at(59), ("a", "p"), ("a", "q"), 1, "DROPPED"),
            row("t4", &at(60), ("a", "p"), ("a", "q"), 1, "AUDIT"),
            row("t5", &at(200), ("b", "p"), ("b", "q"), 1, "FORWARDED"),
        ];
        let start = DateTime::parse_from_rfc3339(base).unwrap().timestamp();
        let all = stores(&rows);
        for (name, store, _) in &all {
            let got = store.timeline(&FlowQuery::default(), 60).unwrap();
            assert_eq!(
                got,
                vec![
                    TimelineBucket {
                        start,
                        forwarded: 1,
                        dropped: 2,
                        other: 0
                    },
                    TimelineBucket {
                        start: start + 60,
                        forwarded: 0,
                        dropped: 0,
                        other: 1
                    },
                    TimelineBucket {
                        start: start + 180,
                        forwarded: 1,
                        dropped: 0,
                        other: 0
                    },
                ],
                "{name}"
            );
            let big = store.timeline(&FlowQuery::default(), 3600).unwrap();
            assert_eq!(big.len(), 1, "{name}");
            assert_eq!(
                (big[0].forwarded, big[0].dropped, big[0].other),
                (2, 2, 1),
                "{name}"
            );
            let scoped = store
                .timeline(
                    &FlowQuery {
                        scope: Some(vec!["b".into()]),
                        ..Default::default()
                    },
                    60,
                )
                .unwrap();
            assert_eq!(scoped.len(), 1, "{name}: filters apply to the timeline");
            assert_eq!(
                store
                    .timeline(
                        &FlowQuery {
                            namespace: Some("nowhere".into()),
                            ..Default::default()
                        },
                        60
                    )
                    .unwrap(),
                vec![],
                "{name}"
            );
        }
        cleanup(all);
    }

    #[test]
    fn timeline_reads_hubble_nanosecond_timestamps() {
        // Real Hubble timestamps have nine fractional digits. They are normalized
        // on the way in, but rows stored by older versions may still carry them.
        let dir = std::env::temp_dir().join(format!(
            "paqtra-flowq-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store = FlowStore::open(&dir, 36500).unwrap();
        {
            let db = store.db.as_ref().unwrap().lock().unwrap();
            db.execute(
                "INSERT INTO flows (id, ts, verdict, protocol, port) VALUES ('old', '2026-06-01T10:00:30.123456789Z', 'DROPPED', 'TCP', 1)",
                [],
            )
            .unwrap();
        }
        let got = store.timeline(&FlowQuery::default(), 60).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(
            (got[0].dropped, got[0].start),
            (
                1,
                DateTime::parse_from_rfc3339("2026-06-01T10:00:00Z")
                    .unwrap()
                    .timestamp()
            )
        );
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn coverage_reports_the_span_and_durability() {
        let all = stores(&dataset());
        for (name, store, _) in &all {
            let c = store.coverage(None).unwrap();
            assert_eq!(c.total, 5, "{name}");
            assert!(
                c.oldest.as_ref().unwrap() < c.newest.as_ref().unwrap(),
                "{name}"
            );
            assert_eq!(c.durable, *name == "sqlite", "{name}");
        }
        // A scope shows only what the caller may see: no count, oldest or newest
        // from namespaces outside it.
        for (name, store, _) in &all {
            let mine = store.coverage(Some(&["ops".to_string()])).unwrap();
            assert_eq!(mine.total, 1, "{name}: only the ops flow");
            assert_eq!(mine.oldest, mine.newest, "{name}");
            let none = store.coverage(Some(&["nowhere".to_string()])).unwrap();
            assert_eq!((none.total, none.oldest), (0, None), "{name}");
            let empty_scope = store.coverage(Some(&[])).unwrap();
            assert_eq!(empty_scope.total, 0, "{name}: an empty scope shows nothing");
        }
        let empty = FlowStore::memory_only().coverage(None).unwrap();
        assert_eq!((empty.total, empty.oldest, empty.newest), (0, None, None));
        cleanup(all);
    }

    #[test]
    fn retention_drops_old_flows_and_repeat_ingest_does_not_duplicate() {
        let dir = std::env::temp_dir().join(format!(
            "paqtra-flowq-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store = FlowStore::open(&dir, 7).unwrap();
        let old = (Utc::now() - ChronoDuration::days(10)).to_rfc3339();
        let recent = (Utc::now() - ChronoDuration::days(1)).to_rfc3339();
        let rows = vec![
            row("old", &old, ("a", "p"), ("a", "q"), 1, "FORWARDED"),
            row("new", &recent, ("a", "p"), ("a", "q"), 1, "FORWARDED"),
        ];
        store.insert_batch(&rows).unwrap();
        store.purge_expired().unwrap();
        assert_eq!(
            ids(&store, &FlowQuery::default()),
            vec!["new"],
            "the 10-day-old flow is past retention"
        );
        // Hubble is polled every 30 s and returns overlapping windows: the same
        // flow arriving again must not become a second row.
        store.insert_batch(&rows).unwrap();
        store.insert_batch(&rows[1..]).unwrap();
        store.purge_expired().unwrap();
        assert_eq!(store.count(&FlowQuery::default()).unwrap(), 1);
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    // ── reads must never take the API down (found on a 10M-row store) ────────

    fn many(n: usize) -> Vec<StoredFlow> {
        (0..n)
            .map(|i| {
                let mut f = sample(&format!("f{i}"));
                f.ts = format!(
                    "2026-09-25T10:{:02}:{:02}.{:06}Z",
                    (i / 3600) % 60,
                    (i / 60) % 60,
                    i % 60
                );
                f.src_pod = format!("pod-{}", i % 50);
                f
            })
            .collect()
    }

    fn temp_store() -> (FlowStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "paqtra-flowread-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        (FlowStore::open(&dir, 36500).unwrap(), dir)
    }

    #[test]
    fn a_runaway_read_is_cut_off_with_a_clear_error() {
        let (mut store, dir) = temp_store();
        store.insert_batch(&many(60_000)).unwrap();
        store.read_deadline = Duration::from_millis(1);
        // A LIKE on a pod name has no index: a full scan.
        let err = store
            .query(&FlowQuery {
                pod: Some("zzz-no-such-pod".into()),
                limit: 10,
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("exceeded") && err.contains("time range"),
            "{err}"
        );

        // The connection is usable again straight after (handler cleared).
        store.read_deadline = Duration::from_secs(30);
        let rows = store
            .query(&FlowQuery {
                limit: 5,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(rows.len(), 5);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stats_never_waits_behind_a_busy_store() {
        let (store, dir) = temp_store();
        store.insert_batch(&many(3)).unwrap();
        assert_eq!(store.stats().total, 3, "exact while small, and remembered");

        let store = Arc::new(store);
        let s2 = store.clone();
        // Hold the read connection, as a long scan would.
        let guard = store.read_db.as_ref().unwrap().lock().unwrap();
        let started = Instant::now();
        let handle = std::thread::spawn(move || s2.stats().total);
        let total = handle.join().unwrap();
        drop(guard);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "stats waited for the lock"
        );
        assert_eq!(total, 3, "falls back to the last known total");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn writes_are_not_blocked_by_a_long_read() {
        let (store, dir) = temp_store();
        store.insert_batch(&many(10)).unwrap();
        // A reader holding its connection for a long time...
        let guard = store.read_db.as_ref().unwrap().lock().unwrap();
        // ...does not stop ingest, which has its own connection.
        let started = Instant::now();
        store.insert_batch(&many(20)).unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(guard);
        assert_eq!(
            store.count(&FlowQuery::default()).unwrap(),
            20,
            "ids repeat: replaced, not duplicated"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_read_connection_cannot_write() {
        let (store, dir) = temp_store();
        let r = store
            .read_db
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .execute("DELETE FROM flows", []);
        assert!(r.is_err(), "readers are query_only");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn totals_are_exact_when_small_and_the_rowid_span_when_large() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE flows (id TEXT)", []).unwrap();
        assert_eq!(fast_total(&conn).unwrap(), 0);
        conn.execute(
            "INSERT INTO flows (rowid, id) VALUES (1, 'a'), (2, 'b'), (3, 'c')",
            [],
        )
        .unwrap();
        assert_eq!(fast_total(&conn).unwrap(), 3);
        // Two rows far apart: a big table by span, so the O(1) estimate is used.
        conn.execute("INSERT INTO flows (rowid, id) VALUES (500000, 'z')", [])
            .unwrap();
        assert_eq!(fast_total(&conn).unwrap(), 500_000);
    }

    #[test]
    fn coverage_of_the_whole_store_does_not_scan() {
        let (store, dir) = temp_store();
        store.insert_batch(&many(50)).unwrap();
        let c = store.coverage(None).unwrap();
        assert_eq!(c.total, 50);
        assert!(c.oldest.is_some() && c.newest.is_some() && c.durable);
        // Scoped coverage still counts exactly.
        let scoped = store.coverage(Some(&["nope".to_string()])).unwrap();
        assert_eq!(scoped.total, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn plan_of(store: &FlowStore, q: &FlowQuery) -> String {
        let (conds, mut vals) = q.where_sql();
        vals.push(Box::new(10_i64));
        vals.push(Box::new(0_i64));
        let sql = format!(
            "EXPLAIN QUERY PLAN SELECT id FROM {} WHERE 1=1{conds} ORDER BY ts DESC LIMIT ? OFFSET ?",
            q.table()
        );
        let db = store.read_conn().unwrap();
        store
            .read(db, |conn| {
                let mut stmt = conn.prepare(&sql)?;
                let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                    vals.iter().map(|b| b.as_ref()).collect();
                let rows = stmt.query_map(params_ref.as_slice(), |r| r.get::<_, String>(3))?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?.join(" | "))
            })
            .unwrap()
    }

    #[test]
    fn a_recent_window_reads_the_time_index_not_the_namespace_index() {
        let (store, dir) = temp_store();
        store.insert_batch(&many(200)).unwrap();
        let now = Utc::now();
        let stamp = |ago: ChronoDuration| (now - ago).to_rfc3339_opts(SecondsFormat::Micros, true);
        let pair = |since: Option<String>, until: Option<String>| FlowQuery {
            src_namespace: Some("kube-system".into()),
            dst_namespace: Some("kube-system".into()),
            port: Some(53),
            since_rfc3339: since,
            until_rfc3339: until,
            ..Default::default()
        };

        // Without the hint SQLite picks the namespace index and sorts a big slice.
        let free = pair(None, None);
        assert_eq!(free.table(), "flows");
        assert!(plan_of(&store, &free).contains("idx_flows_path"));

        // The connectivity monitor's two windows: the last minutes, and the hour before.
        let recent = pair(Some(stamp(ChronoDuration::minutes(2))), None);
        let baseline = pair(
            Some(stamp(ChronoDuration::hours(1))),
            Some(stamp(ChronoDuration::minutes(2))),
        );
        for q in [&recent, &baseline] {
            let plan = plan_of(&store, q);
            assert!(plan.contains("idx_flows_ts"), "{plan}");
            assert!(!plan.contains("idx_flows_path"), "{plan}");
            assert!(!plan.contains("TEMP B-TREE"), "no sort needed: {plan}");
        }

        // A window wider than a day, or a bound that does not parse, is left to the planner.
        assert_eq!(
            pair(Some(stamp(ChronoDuration::days(3))), None).table(),
            "flows"
        );
        assert_eq!(pair(Some("not-a-time".into()), None).table(), "flows");
        // An old but narrow window is still narrow.
        let old = pair(
            Some(stamp(ChronoDuration::days(5))),
            Some(stamp(ChronoDuration::days(5) - ChronoDuration::hours(1))),
        );
        assert_eq!(old.table(), "flows INDEXED BY idx_flows_ts");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_time_index_hint_does_not_change_results() {
        let (store, dir) = temp_store();
        let rows = many(300);
        store.insert_batch(&rows).unwrap();
        let q = FlowQuery {
            src_namespace: Some(rows[0].src_namespace.clone()),
            dst_namespace: Some(rows[0].dst_namespace.clone()),
            since_rfc3339: Some("2026-09-25T10:00:00.000000Z".into()),
            until_rfc3339: Some("2026-09-25T10:00:02.000000Z".into()),
            limit: 1000,
            ..Default::default()
        };
        assert_eq!(q.table(), "flows INDEXED BY idx_flows_ts");
        let hinted = store.query(&q).unwrap();
        let expected = rows
            .iter()
            .filter(|f| {
                f.ts.as_str() >= "2026-09-25T10:00:00.000000Z"
                    && f.ts.as_str() < "2026-09-25T10:00:02.000000Z"
            })
            .count();
        assert!(expected > 0 && expected < rows.len());
        assert_eq!(hinted.len(), expected);
        assert_eq!(store.count(&q).unwrap() as usize, expected);
        assert!(
            hinted.windows(2).all(|w| w[0].ts >= w[1].ts),
            "newest first"
        );
        assert_eq!(
            store
                .timeline(&q, 60)
                .unwrap()
                .iter()
                .map(|b| b.forwarded + b.dropped + b.other)
                .sum::<u64>() as usize,
            expected
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
