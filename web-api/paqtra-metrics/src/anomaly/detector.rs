use crate::tsdb::{Db, Sample, SeriesInfo};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// Training knobs. Zero values pick the defaults.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Seconds of history per training, clipped to what is stored. 6h.
    pub train_window: i64,
    /// Seconds between retrains. 3h.
    pub train_every: i64,
    /// Retrain interval for models trained on less than a full window, so a
    /// fresh node gets useful models within minutes. 10m.
    pub young_every: i64,
    pub min_train_points: usize, // 300
    pub max_train_points: usize, // 3600
    pub lag: usize,              // 5, so feature vectors have 6 values
    pub smooth: usize,           // 3
    pub quantile: f64,           // 0.99
    /// Models trained per cycle, spreading the cost across the interval. 200.
    pub per_cycle: usize,
}

impl Options {
    fn defaults(mut self) -> Self {
        if self.train_window <= 0 {
            self.train_window = 6 * 3600;
        }
        if self.train_every <= 0 {
            self.train_every = 3 * 3600;
        }
        if self.young_every <= 0 {
            self.young_every = 600;
        }
        if self.min_train_points == 0 {
            self.min_train_points = 300;
        }
        if self.max_train_points == 0 {
            self.max_train_points = 3600;
        }
        if self.lag == 0 {
            self.lag = 5;
        }
        if self.smooth == 0 {
            self.smooth = 3;
        }
        if self.quantile <= 0.0 || self.quantile >= 1.0 {
            self.quantile = 0.99;
        }
        if self.per_cycle == 0 {
            self.per_cycle = 200;
        }
        self
    }
}

struct Model {
    centers: [Vec<f64>; 2],
    threshold: f64,
    trained_at: i64,
    span: i64,
}

impl Model {
    fn score(&self, f: &[f64]) -> f64 {
        dist(f, &self.centers[0]).min(dist(f, &self.centers[1]))
    }
}

struct Recent {
    vals: Vec<f64>,
    n: usize,
    pos: usize,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub models: usize,
    pub trained: u64,
    pub scored: u64,
    pub anomalous: u64,
    pub last_train: i64,
}

#[derive(Default)]
struct State {
    models: HashMap<String, Model>,
    recent: HashMap<String, Recent>,
    retry: HashMap<String, i64>,
    stats: Stats,
}

/// Unsupervised per-dimension anomaly detection in the style of Netdata's
/// ML: each dimension gets a k-means model (k=2) trained on differenced,
/// smoothed, lagged values. A sample is anomalous when its distance to the
/// nearest centre exceeds the 99th percentile of training distances.
pub struct Detector {
    opts: Options,
    st: Mutex<State>,
}

/// Differenced, smoothed, lagged feature vectors.
pub(crate) fn features(raw: &[f64], lag: usize, smooth: usize) -> Vec<Vec<f64>> {
    if raw.len() < lag + smooth + 1 {
        return Vec::new();
    }
    let diffs: Vec<f64> = raw.windows(2).map(|w| w[1] - w[0]).collect();
    let sm: Vec<f64> = diffs
        .windows(smooth)
        .map(|w| w.iter().sum::<f64>() / smooth as f64)
        .collect();
    (lag..sm.len()).map(|i| sm[i - lag..=i].to_vec()).collect()
}

fn dist(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f64>()
        .sqrt()
}

/// Two clusters, seeded deterministically with the vectors closest to and
/// farthest from the origin.
fn kmeans2(feats: &[Vec<f64>]) -> [Vec<f64>; 2] {
    let dim = feats[0].len();
    let zero = vec![0.0; dim];
    let (mut lo, mut hi) = (0, 0);
    for (i, f) in feats.iter().enumerate() {
        if dist(f, &zero) < dist(&feats[lo], &zero) {
            lo = i;
        }
        if dist(f, &zero) > dist(&feats[hi], &zero) {
            hi = i;
        }
    }
    let mut c = [feats[lo].clone(), feats[hi].clone()];
    let mut assign = vec![0usize; feats.len()];
    for iter in 0..20 {
        let mut changed = false;
        for (i, f) in feats.iter().enumerate() {
            let k = usize::from(dist(f, &c[1]) < dist(f, &c[0]));
            if assign[i] != k {
                changed = true;
                assign[i] = k;
            }
        }
        let mut sums = [vec![0.0; dim], vec![0.0; dim]];
        let mut counts = [0usize; 2];
        for (i, f) in feats.iter().enumerate() {
            counts[assign[i]] += 1;
            for (s, v) in sums[assign[i]].iter_mut().zip(f) {
                *s += v;
            }
        }
        for k in 0..2 {
            if counts[k] > 0 {
                c[k] = sums[k].iter().map(|s| s / counts[k] as f64).collect();
            }
        }
        if !changed && iter > 0 {
            break;
        }
    }
    c
}

impl Detector {
    pub fn new(opts: Options) -> Self {
        Self {
            opts: opts.defaults(),
            st: Mutex::new(State::default()),
        }
    }

    fn window(&self) -> usize {
        self.opts.lag + self.opts.smooth + 1
    }

    /// Fits a model to raw values covering `span` seconds. False when there
    /// is not enough data.
    pub fn train_series(&self, key: &str, raw: &[f64], span: i64, now: i64) -> bool {
        let mut feats = features(raw, self.opts.lag, self.opts.smooth);
        if feats.len() < self.opts.min_train_points {
            return false;
        }
        if feats.len() > self.opts.max_train_points {
            let stride = feats.len() as f64 / self.opts.max_train_points as f64;
            feats = (0..self.opts.max_train_points)
                .map(|i| feats[(i as f64 * stride) as usize].clone())
                .collect();
        }
        let mut m = Model {
            centers: kmeans2(&feats),
            threshold: 0.0,
            trained_at: now,
            span,
        };
        let mut scores: Vec<f64> = feats.iter().map(|f| m.score(f)).collect();
        scores.sort_by(|a, b| a.total_cmp(b));
        let idx = ((self.opts.quantile * scores.len() as f64).ceil() as isize - 1)
            .clamp(0, scores.len() as isize - 1);
        // A flat window has threshold 0, so any movement is anomalous: the
        // intended reading of "this never changes".
        m.threshold = scores[idx as usize];
        let mut st = self.st.lock().unwrap();
        st.models.insert(key.to_string(), m);
        st.stats.trained += 1;
        st.stats.last_train = now;
        true
    }

    /// Updates each series' recent values and marks samples whose series has
    /// a model and whose score is above its threshold.
    pub fn annotate(&self, samples: &mut [Sample]) {
        let w = self.window();
        let mut st = self.st.lock().unwrap();
        let st = &mut *st;
        for s in samples.iter_mut() {
            let key = s.series.key();
            let r = st.recent.entry(key.clone()).or_insert_with(|| Recent {
                vals: vec![0.0; w],
                n: 0,
                pos: 0,
            });
            r.vals[r.pos] = s.v;
            r.pos = (r.pos + 1) % w;
            r.n = (r.n + 1).min(w);
            let Some(m) = st.models.get(&key) else {
                continue;
            };
            if r.n < w {
                continue;
            }
            let ordered: Vec<f64> = (0..w).map(|j| r.vals[(r.pos + j) % w]).collect();
            let f = features(&ordered, self.opts.lag, self.opts.smooth);
            let Some(last) = f.last() else { continue };
            st.stats.scored += 1;
            if m.score(last) > m.threshold {
                s.a = true;
                st.stats.anomalous += 1;
            }
        }
    }

    fn due(&self, key: &str, now: i64) -> bool {
        let st = self.st.lock().unwrap();
        if st.retry.get(key).is_some_and(|until| now < *until) {
            return false;
        }
        match st.models.get(key) {
            None => true,
            Some(m) => {
                let every = if m.span < self.opts.train_window * 9 / 10 {
                    self.opts.young_every
                } else {
                    self.opts.train_every
                };
                now - m.trained_at >= every
            }
        }
    }

    /// Trains up to `limit` due models from `db`, oldest model first.
    pub fn train_due(&self, db: &Db, now: i64, limit: usize) -> usize {
        let infos = db.list();
        let mut cands: Vec<(i64, &str)> = {
            let st = self.st.lock().unwrap();
            infos
                .iter()
                .map(|i| {
                    (
                        st.models.get(&i.key).map_or(0, |m| m.trained_at),
                        i.key.as_str(),
                    )
                })
                .collect()
        };
        cands.sort();
        let mut n = 0;
        let after = now - self.opts.train_window;
        for (_, key) in cands {
            if n >= limit {
                break;
            }
            if !self.due(key, now) {
                continue;
            }
            let pts = db.points(key, after, now);
            if pts.is_empty() {
                continue;
            }
            let raw: Vec<f64> = pts.iter().map(|p| p.v).collect();
            let span = pts[pts.len() - 1].t - pts[0].t;
            if self.train_series(key, &raw, span, now) {
                n += 1;
            } else {
                // Not enough data yet: retry in a minute, not every cycle.
                self.st
                    .lock()
                    .unwrap()
                    .retry
                    .insert(key.to_string(), now + 60);
            }
        }
        self.prune(&infos);
        n
    }

    pub fn per_cycle(&self) -> usize {
        self.opts.per_cycle
    }

    fn prune(&self, infos: &[SeriesInfo]) {
        let live: HashSet<&str> = infos.iter().map(|i| i.key.as_str()).collect();
        let mut st = self.st.lock().unwrap();
        st.models.retain(|k, _| live.contains(k.as_str()));
        st.recent.retain(|k, _| live.contains(k.as_str()));
        st.retry.retain(|k, _| live.contains(k.as_str()));
    }

    pub fn stats(&self) -> Stats {
        let st = self.st.lock().unwrap();
        Stats {
            models: st.models.len(),
            ..st.stats
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsdb::{Options as DbOptions, Series};

    fn sample(v: f64, t: i64) -> Sample {
        Sample {
            series: Series {
                context: "c".into(),
                chart: "c".into(),
                dimension: "d".into(),
                ..Default::default()
            },
            t,
            v,
            a: false,
        }
    }

    #[test]
    fn features_shape() {
        let f = features(
            &[1.0, 2.0, 4.0, 7.0, 11.0, 16.0, 22.0, 29.0, 37.0, 46.0],
            5,
            3,
        );
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].len(), 6);
        assert!(features(&[1.0, 2.0], 5, 3).is_empty());
    }

    #[test]
    fn flags_a_spike_after_training() {
        let db = Db::open(DbOptions::default()).unwrap();
        let det = Detector::new(Options {
            min_train_points: 100,
            ..Default::default()
        });
        for t in 0..1200 {
            let v = 50.0 + ((t as f64) * 0.3).sin();
            db.append(&sample(v, 10_000 + t)).unwrap();
        }
        assert_eq!(det.train_due(&db, 11_200, 10), 1);
        let mut quiet: Vec<Sample> = (0..20)
            .map(|i| sample(50.0 + ((1200 + i) as f64 * 0.3).sin(), 11_200 + i))
            .collect();
        det.annotate(&mut quiet);
        assert!(quiet.iter().filter(|s| s.a).count() <= 2);
        let mut spike = vec![sample(500.0, 11_300)];
        det.annotate(&mut spike);
        assert!(spike[0].a);
        assert!(det.stats().anomalous >= 1);
        assert_eq!(det.train_due(&db, 11_201, 10), 0, "young model not due yet");
    }

    #[test]
    fn short_history_backs_off() {
        let db = Db::open(DbOptions::default()).unwrap();
        for t in 0..20 {
            db.append(&sample(1.0, t + 1)).unwrap();
        }
        let det = Detector::new(Options::default());
        assert_eq!(det.train_due(&db, 100, 10), 0);
        assert!(!det.due("c|c|d", 120));
        assert!(det.due("c|c|d", 161));
    }
}
