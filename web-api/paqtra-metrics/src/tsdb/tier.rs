//! Rollup tiers: a capped in-memory ring, or append-only segment files.

use super::series::Rollup;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;

pub trait TierStore: Send + Sync {
    fn write(&self, id: u32, r: Rollup) -> io::Result<()>;
    fn read(
        &self,
        ids: &HashSet<u32>,
        after: i64,
        before: i64,
        f: &mut dyn FnMut(u32, Rollup),
    ) -> io::Result<()>;
    fn enforce(&self, now: i64, retention: i64, quota: i64) -> io::Result<()>;
    fn flush(&self) -> io::Result<()>;
    fn size_bytes(&self) -> i64;
    fn close(&self) -> io::Result<()>;
}

/// Capped ring of rollups per series; used when the DB has no directory.
pub struct MemTier {
    cap: usize,
    rows: Mutex<HashMap<u32, Vec<Rollup>>>,
}

impl MemTier {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            rows: Mutex::new(HashMap::new()),
        }
    }
}

impl TierStore for MemTier {
    fn write(&self, id: u32, r: Rollup) -> io::Result<()> {
        let mut rows = self.rows.lock().unwrap();
        let v = rows.entry(id).or_default();
        v.push(r);
        if v.len() > self.cap {
            let excess = v.len() - self.cap;
            v.drain(..excess);
        }
        Ok(())
    }

    fn read(
        &self,
        ids: &HashSet<u32>,
        after: i64,
        before: i64,
        f: &mut dyn FnMut(u32, Rollup),
    ) -> io::Result<()> {
        let rows = self.rows.lock().unwrap();
        for id in ids {
            if let Some(v) = rows.get(id) {
                for r in v.iter().filter(|r| r.start >= after && r.start <= before) {
                    f(*id, *r);
                }
            }
        }
        Ok(())
    }

    fn enforce(&self, now: i64, retention: i64, _quota: i64) -> io::Result<()> {
        let mut rows = self.rows.lock().unwrap();
        rows.retain(|_, v| {
            let i = v.partition_point(|r| r.start < now - retention);
            v.drain(..i);
            !v.is_empty()
        });
        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        Ok(())
    }

    fn size_bytes(&self) -> i64 {
        self.rows
            .lock()
            .unwrap()
            .values()
            .map(|v| v.len() as i64 * 40)
            .sum()
    }

    fn close(&self) -> io::Result<()> {
        Ok(())
    }
}

/// Fixed-size records in one file per segment window, named by the window's
/// start second.
///
/// Record (little endian, 24 bytes): series id u32, offset from segment start
/// u32, min f32, max f32, avg f32, count u16, anomalous u16.
pub struct DiskTier {
    dir: PathBuf,
    seg_span: i64,
    cur: Mutex<Option<(i64, BufWriter<File>)>>,
}

const RECORD: usize = 24;

impl DiskTier {
    pub fn new(dir: PathBuf, seg_span: i64) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            seg_span,
            cur: Mutex::new(None),
        })
    }

    fn seg_path(&self, start: i64) -> PathBuf {
        self.dir.join(format!("{start}.seg"))
    }

    fn segments(&self) -> io::Result<Vec<i64>> {
        let mut out = Vec::new();
        for e in fs::read_dir(&self.dir)? {
            let name = e?.file_name();
            let name = name.to_string_lossy();
            if let Some(n) = name.strip_suffix(".seg") {
                if let Ok(v) = n.parse::<i64>() {
                    out.push(v);
                }
            }
        }
        out.sort_unstable();
        Ok(out)
    }

    fn read_segment(
        &self,
        seg: i64,
        ids: &HashSet<u32>,
        after: i64,
        before: i64,
        f: &mut dyn FnMut(u32, Rollup),
    ) -> io::Result<()> {
        let file = match File::open(self.seg_path(seg)) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        let mut r = BufReader::with_capacity(256 << 10, file);
        let mut buf = [0u8; RECORD];
        loop {
            match r.read_exact(&mut buf) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(e) => return Err(e),
            }
            let u32at = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
            let f32at = |o: usize| f32::from_bits(u32at(o)) as f64;
            let u16at = |o: usize| u16::from_le_bytes(buf[o..o + 2].try_into().unwrap());
            let id = u32at(0);
            if !ids.contains(&id) {
                continue;
            }
            let start = seg + u32at(4) as i64;
            if start < after || start > before {
                continue;
            }
            let count = u16at(20) as u32;
            let avg = f32at(16);
            f(
                id,
                Rollup {
                    start,
                    min: f32at(8),
                    max: f32at(12),
                    sum: avg * count as f64,
                    count,
                    anomalous: u16at(22) as u32,
                },
            );
        }
    }
}

impl TierStore for DiskTier {
    fn write(&self, id: u32, r: Rollup) -> io::Result<()> {
        let seg = r.start - r.start.rem_euclid(self.seg_span);
        let mut cur = self.cur.lock().unwrap();
        if cur.as_ref().map(|c| c.0) != Some(seg) {
            if let Some((_, mut w)) = cur.take() {
                w.flush()?;
            }
            let f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.seg_path(seg))?;
            *cur = Some((seg, BufWriter::with_capacity(64 << 10, f)));
        }
        let mut buf = [0u8; RECORD];
        buf[0..4].copy_from_slice(&id.to_le_bytes());
        buf[4..8].copy_from_slice(&((r.start - seg) as u32).to_le_bytes());
        buf[8..12].copy_from_slice(&(r.min as f32).to_bits().to_le_bytes());
        buf[12..16].copy_from_slice(&(r.max as f32).to_bits().to_le_bytes());
        buf[16..20].copy_from_slice(&(r.avg() as f32).to_bits().to_le_bytes());
        buf[20..22].copy_from_slice(&(r.count.min(u16::MAX as u32) as u16).to_le_bytes());
        buf[22..24].copy_from_slice(&(r.anomalous.min(u16::MAX as u32) as u16).to_le_bytes());
        cur.as_mut().unwrap().1.write_all(&buf)
    }

    fn read(
        &self,
        ids: &HashSet<u32>,
        after: i64,
        before: i64,
        f: &mut dyn FnMut(u32, Rollup),
    ) -> io::Result<()> {
        let segs = {
            let mut cur = self.cur.lock().unwrap();
            if let Some((_, w)) = cur.as_mut() {
                w.flush()?;
            }
            self.segments()?
        };
        for seg in segs {
            if seg + self.seg_span <= after || seg > before {
                continue;
            }
            self.read_segment(seg, ids, after, before, f)?;
        }
        Ok(())
    }

    /// Deletes segments older than retention, then the oldest ones until the
    /// tier fits in quota. The segment being written is kept.
    fn enforce(&self, now: i64, retention: i64, quota: i64) -> io::Result<()> {
        let cur = self.cur.lock().unwrap();
        let cur_seg = cur.as_ref().map(|c| c.0);
        let mut all = Vec::new();
        let mut total = 0i64;
        for s in self.segments()? {
            let Ok(st) = fs::metadata(self.seg_path(s)) else {
                continue;
            };
            if s + self.seg_span < now - retention && Some(s) != cur_seg {
                fs::remove_file(self.seg_path(s))?;
                continue;
            }
            all.push((s, st.len() as i64));
            total += st.len() as i64;
        }
        for (s, size) in all {
            if quota <= 0 || total <= quota {
                break;
            }
            if Some(s) == cur_seg {
                continue;
            }
            fs::remove_file(self.seg_path(s))?;
            total -= size;
        }
        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        if let Some((_, w)) = self.cur.lock().unwrap().as_mut() {
            w.flush()?;
        }
        Ok(())
    }

    fn size_bytes(&self) -> i64 {
        self.segments()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|s| fs::metadata(self.seg_path(s)).ok())
            .map(|m| m.len() as i64)
            .sum()
    }

    fn close(&self) -> io::Result<()> {
        if let Some((_, mut w)) = self.cur.lock().unwrap().take() {
            w.flush()?;
        }
        Ok(())
    }
}
