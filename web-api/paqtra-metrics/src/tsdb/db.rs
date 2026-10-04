use super::chunk::Chunk;
use super::series::{Point, Rollup, Sample, Series};
use super::tier::{DiskTier, MemTier, TierStore};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

pub const TIER0_RESOLUTION: i64 = 1;
pub const TIER1_RESOLUTION: i64 = 60;
pub const TIER2_RESOLUTION: i64 = 3600;

#[derive(Debug, PartialEq)]
pub enum Error {
    /// The sample is not newer than the last sample of its series.
    OutOfOrder,
    /// A new series would exceed `max_series`.
    SeriesLimit,
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::OutOfOrder => write!(f, "tsdb: sample out of order"),
            Error::SeriesLimit => write!(f, "tsdb: series limit reached"),
            Error::Io(e) => write!(f, "tsdb: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

/// Zero values pick the defaults.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Holds rollup segments and the series index. `None` keeps the rollup
    /// tiers in a capped in-memory ring.
    pub dir: Option<PathBuf>,
    pub tier0_retention: Duration, // 1h
    pub tier1_retention: Duration, // 14d
    pub tier2_retention: Duration, // 365d
    /// Caps tier 1 plus tier 2 on disk. Tier 1 gets 80%.
    pub disk_quota_bytes: i64, // 1 GiB
    pub max_series: usize,         // 50000
    pub mem_tier1_rows: usize,     // 1440
    pub mem_tier2_rows: usize,     // 168
}

impl Options {
    fn defaults(mut self) -> Self {
        if self.tier0_retention.is_zero() {
            self.tier0_retention = Duration::from_secs(3600);
        }
        if self.tier1_retention.is_zero() {
            self.tier1_retention = Duration::from_secs(14 * 86400);
        }
        if self.tier2_retention.is_zero() {
            self.tier2_retention = Duration::from_secs(365 * 86400);
        }
        if self.disk_quota_bytes <= 0 {
            self.disk_quota_bytes = 1 << 30;
        }
        if self.max_series == 0 {
            self.max_series = 50_000;
        }
        if self.mem_tier1_rows == 0 {
            self.mem_tier1_rows = 1440;
        }
        if self.mem_tier2_rows == 0 {
            self.mem_tier2_rows = 168;
        }
        self
    }
}

struct SeriesState {
    id: u32,
    meta: Series,
    chunks: Vec<Chunk>,
    first_t: i64,
    last_t: i64,
    last_v: f64,
    last_a: bool,
    agg: [Rollup; 2],
}

type SeriesRef = Arc<Mutex<SeriesState>>;

fn update_meta(s: &mut SeriesState, m: &Series) {
    if !m.units.is_empty() || !m.title.is_empty() || !m.family.is_empty() {
        s.meta.units.clone_from(&m.units);
        s.meta.title.clone_from(&m.title);
        s.meta.family.clone_from(&m.family);
        s.meta.chart_type.clone_from(&m.chart_type);
    }
}

#[derive(Default)]
struct Index {
    by_key: HashMap<String, SeriesRef>,
    by_id: HashMap<u32, SeriesRef>,
    next_id: u32,
    dirty: bool,
}

/// A per-node metrics store: compressed 1s tier in memory plus per-minute
/// and per-hour rollup tiers.
pub struct Db {
    opts: Options,
    idx: RwLock<Index>,
    tiers: [Box<dyn TierStore>; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesInfo {
    pub id: u32,
    pub key: String,
    pub series: Series,
    pub first_t: i64,
    pub last_t: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexEntry {
    id: u32,
    series: Series,
    first_t: i64,
    last_t: i64,
}

/// Tier-0 points of one series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesPoints {
    pub series: Series,
    pub points: Vec<Point>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub series: usize,
    pub memory_bytes: i64,
    pub disk_bytes: i64,
    pub oldest_t: i64,
    pub newest_t: i64,
}

impl Db {
    /// With a directory, loads the series index so rollups written before a
    /// restart stay queryable.
    pub fn open(opts: Options) -> Result<Db, Error> {
        let opts = opts.defaults();
        let tiers: [Box<dyn TierStore>; 2] = match &opts.dir {
            None => [
                Box::new(MemTier::new(opts.mem_tier1_rows)),
                Box::new(MemTier::new(opts.mem_tier2_rows)),
            ],
            Some(dir) => [
                Box::new(DiskTier::new(dir.join("tier1"), 86400)?),
                Box::new(DiskTier::new(dir.join("tier2"), 30 * 86400)?),
            ],
        };
        let db = Db {
            opts,
            idx: RwLock::new(Index {
                next_id: 1,
                ..Default::default()
            }),
            tiers,
        };
        db.load_index()?;
        Ok(db)
    }

    fn index_path(&self) -> Option<PathBuf> {
        self.opts.dir.as_ref().map(|d| d.join("series.json"))
    }

    fn load_index(&self) -> Result<(), Error> {
        let Some(path) = self.index_path() else {
            return Ok(());
        };
        let b = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let entries: Vec<IndexEntry> =
            serde_json::from_slice(&b).map_err(|e| Error::Io(format!("index: {e}")))?;
        let mut idx = self.idx.write().unwrap();
        for e in entries {
            let s = Arc::new(Mutex::new(SeriesState {
                id: e.id,
                meta: e.series.clone(),
                chunks: Vec::new(),
                first_t: e.first_t,
                last_t: e.last_t,
                last_v: 0.0,
                last_a: false,
                agg: [Rollup::default(); 2],
            }));
            idx.by_key.insert(e.series.key(), s.clone());
            idx.by_id.insert(e.id, s);
            idx.next_id = idx.next_id.max(e.id + 1);
        }
        Ok(())
    }

    fn save_index(&self) -> Result<(), Error> {
        let Some(path) = self.index_path() else {
            return Ok(());
        };
        let mut entries = {
            let idx = self.idx.read().unwrap();
            if !idx.dirty {
                return Ok(());
            }
            idx.by_id
                .values()
                .map(|s| {
                    let s = s.lock().unwrap();
                    IndexEntry {
                        id: s.id,
                        series: s.meta.clone(),
                        first_t: s.first_t,
                        last_t: s.last_t,
                    }
                })
                .collect::<Vec<_>>()
        };
        entries.sort_by_key(|e| e.id);
        let b = serde_json::to_vec(&entries).map_err(|e| Error::Io(e.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, b)?;
        std::fs::rename(&tmp, &path)?;
        self.idx.write().unwrap().dirty = false;
        Ok(())
    }

    fn get_or_create(&self, meta: &Series) -> Result<SeriesRef, Error> {
        let key = meta.key();
        if let Some(s) = self.idx.read().unwrap().by_key.get(&key) {
            return Ok(s.clone());
        }
        let mut idx = self.idx.write().unwrap();
        if let Some(s) = idx.by_key.get(&key) {
            return Ok(s.clone());
        }
        if idx.by_key.len() >= self.opts.max_series {
            return Err(Error::SeriesLimit);
        }
        let id = idx.next_id;
        idx.next_id += 1;
        let s = Arc::new(Mutex::new(SeriesState {
            id,
            meta: meta.clone(),
            chunks: Vec::new(),
            first_t: 0,
            last_t: 0,
            last_v: 0.0,
            last_a: false,
            agg: [Rollup::default(); 2],
        }));
        idx.by_key.insert(key, s.clone());
        idx.by_id.insert(id, s.clone());
        idx.dirty = true;
        Ok(s)
    }

    pub fn append(&self, smp: &Sample) -> Result<(), Error> {
        let s = self.get_or_create(&smp.series)?;
        let mut s = s.lock().unwrap();
        if s.last_t != 0 && smp.t <= s.last_t {
            return Err(Error::OutOfOrder);
        }
        update_meta(&mut s, &smp.series);
        self.append_point(&mut s, smp.t, smp.v, smp.a)
    }

    /// Stores one series' points with a single index lookup and lock,
    /// skipping out-of-order ones. Returns the number stored and the first
    /// error that was not `OutOfOrder`, after which the rest are dropped.
    pub fn append_points(
        &self,
        meta: &Series,
        points: impl IntoIterator<Item = (i64, f64, bool)>,
    ) -> (usize, Option<Error>) {
        let s = match self.get_or_create(meta) {
            Ok(s) => s,
            Err(e) => return (0, Some(e)),
        };
        let mut s = s.lock().unwrap();
        update_meta(&mut s, meta);
        let mut n = 0;
        for (t, v, a) in points {
            if s.last_t != 0 && t <= s.last_t {
                continue;
            }
            if let Err(e) = self.append_point(&mut s, t, v, a) {
                return (n, Some(e));
            }
            n += 1;
        }
        (n, None)
    }

    fn append_point(&self, s: &mut SeriesState, t: i64, v: f64, a: bool) -> Result<(), Error> {
        if s.chunks.last().is_none_or(|c| c.full()) {
            s.chunks.push(Chunk::default());
        }
        s.chunks.last_mut().unwrap().append(t, v, a);
        if s.first_t == 0 {
            s.first_t = t;
        }
        s.last_t = t;
        s.last_v = v;
        s.last_a = a;
        let id = s.id;
        for (i, res) in [TIER1_RESOLUTION, TIER2_RESOLUTION].into_iter().enumerate() {
            let start = t - t.rem_euclid(res);
            if s.agg[i].count > 0 && s.agg[i].start != start {
                self.tiers[i].write(id, s.agg[i])?;
                s.agg[i] = Rollup::default();
            }
            if s.agg[i].count == 0 {
                s.agg[i].start = start;
            }
            s.agg[i].add(v, a);
        }
        Ok(())
    }

    /// Stores samples, skipping out-of-order ones. Returns the number stored
    /// and the first error that was not `OutOfOrder`.
    pub fn append_batch(&self, samples: &[Sample]) -> (usize, Option<Error>) {
        let mut n = 0;
        let mut first = None;
        for s in samples {
            match self.append(s) {
                Ok(()) => n += 1,
                Err(Error::OutOfOrder) => {}
                Err(e) => {
                    if first.is_none() {
                        first = Some(e);
                    }
                }
            }
        }
        (n, first)
    }

    fn all(&self) -> Vec<SeriesRef> {
        self.idx.read().unwrap().by_id.values().cloned().collect()
    }

    fn lookup(&self, key: &str) -> Option<SeriesRef> {
        self.idx.read().unwrap().by_key.get(key).cloned()
    }

    /// Every known series, sorted by key.
    pub fn list(&self) -> Vec<SeriesInfo> {
        let mut out: Vec<SeriesInfo> = self
            .all()
            .iter()
            .map(|s| {
                let s = s.lock().unwrap();
                SeriesInfo {
                    id: s.id,
                    key: s.meta.key(),
                    series: s.meta.clone(),
                    first_t: s.first_t,
                    last_t: s.last_t,
                }
            })
            .collect();
        out.sort_by(|a, b| a.key.cmp(&b.key));
        out
    }

    pub fn last(&self, key: &str) -> Option<Point> {
        let s = self.lookup(key)?;
        let s = s.lock().unwrap();
        if s.last_t == 0 || s.chunks.is_empty() {
            return None;
        }
        Some(Point {
            t: s.last_t,
            v: s.last_v,
            anomalous: s.last_a,
        })
    }

    /// Newest timestamp across all series, or 0.
    pub fn last_t(&self) -> i64 {
        self.all()
            .iter()
            .map(|s| s.lock().unwrap().last_t)
            .max()
            .unwrap_or(0)
    }

    /// Tier-0 points of a series in [after, before].
    pub fn points(&self, key: &str, after: i64, before: i64) -> Vec<Point> {
        match self.lookup(key) {
            Some(s) => points_of(&s.lock().unwrap(), after, before),
            None => Vec::new(),
        }
    }

    /// Rollups of tier 1 or 2 for the given keys, including the bucket still
    /// being aggregated.
    pub fn rollups(
        &self,
        tier: usize,
        keys: &[String],
        after: i64,
        before: i64,
    ) -> Result<HashMap<String, Vec<Rollup>>, Error> {
        if tier != 1 && tier != 2 {
            return Err(Error::Io("tier must be 1 or 2".into()));
        }
        let mut ids = HashSet::new();
        let mut id_key = HashMap::new();
        let mut live = Vec::new();
        for k in keys {
            if let Some(s) = self.lookup(k) {
                let id = s.lock().unwrap().id;
                ids.insert(id);
                id_key.insert(id, k.clone());
                live.push(s);
            }
        }
        let mut out: HashMap<String, Vec<Rollup>> = HashMap::new();
        self.tiers[tier - 1].read(&ids, after, before, &mut |id, r| {
            out.entry(id_key[&id].clone()).or_default().push(r);
        })?;
        for s in live {
            let s = s.lock().unwrap();
            let cur = s.agg[tier - 1];
            if cur.count > 0 && cur.start >= after && cur.start <= before {
                out.entry(id_key[&s.id].clone()).or_default().push(cur);
            }
        }
        for rows in out.values_mut() {
            rows.sort_by_key(|r| r.start);
        }
        Ok(out)
    }

    /// Tier-0 points newer than `after`, oldest first, stopping near
    /// `max_points`. The cursor returned is the newest second complete across
    /// every series in the batch.
    pub fn since(&self, after: i64, max_points: usize) -> (Vec<SeriesPoints>, i64) {
        let all = self.all();
        let newest = all
            .iter()
            .map(|s| s.lock().unwrap().last_t)
            .max()
            .unwrap_or(0);
        if newest <= after {
            return (Vec::new(), after);
        }
        let mut before = newest;
        if max_points > 0 && !all.is_empty() {
            let span = ((max_points / all.len()) as i64).max(1);
            let mut oldest = newest;
            for s in &all {
                let s = s.lock().unwrap();
                if let Some(c) = s.chunks.iter().find(|c| c.last > after) {
                    oldest = oldest.min(c.first.max(after + 1));
                }
            }
            if oldest + span - 1 < newest {
                before = oldest + span - 1;
            }
        }
        let mut out: Vec<SeriesPoints> = all
            .iter()
            .filter_map(|s| {
                let s = s.lock().unwrap();
                let pts = points_of(&s, after + 1, before);
                (!pts.is_empty()).then(|| SeriesPoints {
                    series: s.meta.clone(),
                    points: pts,
                })
            })
            .collect();
        out.sort_by_cached_key(|sp| sp.series.key());
        (out, before)
    }

    /// Evicts expired tier-0 chunks, drops idle series, flushes rollups,
    /// enforces retention and quota, and saves the index.
    pub fn maintain(&self, now: i64) -> Result<(), Error> {
        let cut = now - self.opts.tier0_retention.as_secs() as i64;
        let idle_cut = now - self.opts.tier1_retention.as_secs() as i64;
        {
            let mut idx = self.idx.write().unwrap();
            let mut dropped = Vec::new();
            for (key, s) in &idx.by_key {
                let mut s = s.lock().unwrap();
                let i = s.chunks.iter().take_while(|c| c.last < cut).count();
                s.chunks.drain(..i);
                if s.chunks.is_empty() && s.last_t != 0 && s.last_t < idle_cut {
                    dropped.push((key.clone(), s.id));
                }
            }
            for (key, id) in &dropped {
                idx.by_key.remove(key);
                idx.by_id.remove(id);
            }
            if !dropped.is_empty() {
                idx.dirty = true;
            }
        }
        let quota1 = self.opts.disk_quota_bytes * 8 / 10;
        let quota2 = self.opts.disk_quota_bytes - quota1;
        self.tiers[0].flush()?;
        self.tiers[1].flush()?;
        self.tiers[0].enforce(now, self.opts.tier1_retention.as_secs() as i64, quota1)?;
        self.tiers[1].enforce(now, self.opts.tier2_retention.as_secs() as i64, quota2)?;
        self.save_index()
    }

    pub fn stats(&self) -> Stats {
        let all = self.all();
        let mut st = Stats {
            series: all.len(),
            ..Default::default()
        };
        for s in &all {
            let s = s.lock().unwrap();
            st.memory_bytes += s.chunks.iter().map(|c| c.size_bytes() as i64).sum::<i64>() + 256;
            if s.first_t != 0 && (st.oldest_t == 0 || s.first_t < st.oldest_t) {
                st.oldest_t = s.first_t;
            }
            st.newest_t = st.newest_t.max(s.last_t);
        }
        let tiers = self.tiers[0].size_bytes() + self.tiers[1].size_bytes();
        if self.opts.dir.is_none() {
            st.memory_bytes += tiers;
        } else {
            st.disk_bytes = tiers;
        }
        st
    }

    pub fn retention(&self) -> [Duration; 3] {
        [
            self.opts.tier0_retention,
            self.opts.tier1_retention,
            self.opts.tier2_retention,
        ]
    }

    /// Flushes pending rollups and the index.
    pub fn close(&self) -> Result<(), Error> {
        for s in self.all() {
            let mut s = s.lock().unwrap();
            for i in 0..2 {
                if s.agg[i].count > 0 {
                    self.tiers[i].write(s.id, s.agg[i])?;
                    s.agg[i] = Rollup::default();
                }
            }
        }
        self.idx.write().unwrap().dirty = true;
        self.save_index()?;
        self.tiers[0].close()?;
        self.tiers[1].close()?;
        Ok(())
    }
}

fn points_of(s: &SeriesState, after: i64, before: i64) -> Vec<Point> {
    let mut out = Vec::new();
    for c in &s.chunks {
        if c.last < after || c.first > before {
            continue;
        }
        c.for_each(|t, v, a| {
            if t > before {
                return false;
            }
            if t >= after {
                out.push(Point { t, v, anomalous: a });
            }
            true
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn series(ctx: &str, dim: &str) -> Series {
        Series {
            context: ctx.into(),
            chart: ctx.into(),
            dimension: dim.into(),
            units: "x".into(),
            ..Default::default()
        }
    }

    fn sample(s: &Series, t: i64, v: f64) -> Sample {
        Sample {
            series: s.clone(),
            t,
            v,
            a: false,
        }
    }

    #[test]
    fn append_points_and_out_of_order() {
        let db = Db::open(Options::default()).unwrap();
        let s = series("system.cpu", "user");
        for t in 1000..1600 {
            db.append(&sample(&s, t, t as f64)).unwrap();
        }
        assert_eq!(db.append(&sample(&s, 1599, 0.0)), Err(Error::OutOfOrder));
        let pts = db.points(&s.key(), 1100, 1109);
        assert_eq!(pts.len(), 10);
        assert_eq!(pts[0].v, 1100.0);
        assert_eq!(db.last(&s.key()).unwrap().t, 1599);
        let r = db.rollups(1, &[s.key()], 0, 2000).unwrap();
        let rows = &r[&s.key()];
        assert_eq!(rows.len(), 11);
        assert_eq!(rows[1].count, 60);
    }

    #[test]
    fn append_points_matches_per_sample_append() {
        let (a, b) = (
            Db::open(Options::default()).unwrap(),
            Db::open(Options::default()).unwrap(),
        );
        let s = series("system.cpu", "user");
        let pts: Vec<(i64, f64, bool)> = (1000..1200).map(|t| (t, t as f64, t == 1100)).collect();
        for &(t, v, an) in &pts {
            a.append(&Sample {
                series: s.clone(),
                t,
                v,
                a: an,
            })
            .unwrap();
        }
        assert_eq!(b.append_points(&s, pts.iter().copied()), (200, None));
        assert_eq!(
            b.append_points(&s, [(1150, 0.0, false), (1199, 0.0, false)]),
            (0, None),
            "already-stored seconds are skipped"
        );
        assert_eq!(a.points(&s.key(), 0, 2000), b.points(&s.key(), 0, 2000));
        assert_eq!(
            a.rollups(1, &[s.key()], 0, 2000).unwrap()[&s.key()].len(),
            b.rollups(1, &[s.key()], 0, 2000).unwrap()[&s.key()].len()
        );
    }

    #[test]
    fn series_limit() {
        let db = Db::open(Options {
            max_series: 2,
            ..Default::default()
        })
        .unwrap();
        for (i, d) in ["a", "b", "c"].iter().enumerate() {
            let r = db.append(&sample(&series("c", d), 10, 1.0));
            assert_eq!(r.is_err(), i == 2);
        }
    }

    #[test]
    fn since_batches_and_cursor() {
        let db = Db::open(Options::default()).unwrap();
        let a = series("c", "a");
        let b = series("c", "b");
        for t in 1..=100 {
            db.append(&sample(&a, t, 1.0)).unwrap();
            db.append(&sample(&b, t, 2.0)).unwrap();
        }
        let (batch, cur) = db.since(0, 40);
        assert_eq!(cur, 20);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].points.len(), 20);
        let (batch, cur) = db.since(cur, 0);
        assert_eq!(cur, 100);
        assert_eq!(batch[0].points.len(), 80);
        assert!(db.since(100, 0).0.is_empty());
    }

    #[test]
    fn disk_tiers_survive_reopen_and_evict() {
        let dir = tempfile::tempdir().unwrap();
        let opts = Options {
            dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let s = series("disk.io", "reads");
        {
            let db = Db::open(opts.clone()).unwrap();
            for t in 0..7200 {
                db.append(&sample(&s, 1_000_000 + t, 5.0)).unwrap();
            }
            db.close().unwrap();
        }
        let db = Db::open(opts).unwrap();
        assert_eq!(db.list().len(), 1);
        let r = db.rollups(1, &[s.key()], 0, i64::MAX).unwrap();
        assert!(r[&s.key()].len() >= 119);
        assert!((r[&s.key()][3].avg() - 5.0).abs() < 1e-6);
        db.maintain(1_000_000 + 400 * 86400).unwrap();
        let r = db.rollups(1, &[s.key()], 0, i64::MAX).unwrap();
        assert!(r.get(&s.key()).is_none_or(|v| v.is_empty()));
    }
}
