use crate::tsdb::{match_any, Db, NullFloat, SeriesInfo, Source};
use serde::Serialize;
use std::collections::BTreeMap;

/// A node's anomaly rate over a window.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRate {
    pub node: String,
    /// Percent of samples.
    pub anomaly_rate: f64,
    pub dimensions: usize,
    #[serde(rename = "anomalousDimensions")]
    pub anomalous: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub timeline: Vec<TimelinePoint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelinePoint {
    pub t: i64,
    pub rate: NullFloat,
}

/// One dimension ordered by how anomalous or changed it is.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ranked {
    pub node: String,
    pub context: String,
    pub chart: String,
    pub dimension: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub units: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    pub anomaly_rate: f64,
    /// The anomaly rate for plain ranking, or the two-sample
    /// Kolmogorov-Smirnov statistic (0..1) for highlight correlation.
    pub score: f64,
}

/// The answer to GET /api/v1/metrics/anomalies.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub after: i64,
    pub before: i64,
    pub nodes: Vec<NodeRate>,
    pub ranked: Vec<Ranked>,
    #[serde(skip_serializing_if = "is_zero")]
    pub highlight_after: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub highlight_before: i64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub correlated: Vec<Ranked>,
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}

#[derive(Default, Clone, Copy)]
struct Counts {
    anom: usize,
    total: usize,
}

fn ranked(node: &str, info: &SeriesInfo, rate: f64, score: f64) -> Ranked {
    let s = &info.series;
    Ranked {
        node: node.into(),
        context: s.context.clone(),
        chart: s.chart.clone(),
        dimension: s.dimension.clone(),
        units: s.units.clone(),
        labels: s.labels.clone(),
        anomaly_rate: rate,
        score,
    }
}

/// Anomalous and total sample counts plus per-bucket counts, from tier 0
/// inside its retention and tier-1 rollups otherwise.
fn series_counts(
    db: &Db,
    info: &SeriesInfo,
    after: i64,
    before: i64,
    now: i64,
    buckets: usize,
) -> (Counts, Vec<Counts>) {
    let mut c = Counts::default();
    let mut per = vec![Counts::default(); buckets];
    let step = ((before - after + buckets as i64 - 1) / buckets as i64).max(1);
    let mut add = |t: i64, anom: usize, total: usize| {
        c.anom += anom;
        c.total += total;
        let i = (t - after) / step;
        if i >= 0 && (i as usize) < buckets {
            per[i as usize].anom += anom;
            per[i as usize].total += total;
        }
    };
    if after >= now - db.retention()[0].as_secs() as i64 {
        for p in db.points(&info.key, after, before) {
            add(p.t, usize::from(p.anomalous), 1);
        }
    } else if let Ok(rs) = db.rollups(1, std::slice::from_ref(&info.key), after, before) {
        for r in rs.get(&info.key).into_iter().flatten() {
            add(r.start, r.anomalous as usize, r.count as usize);
        }
    }
    (c, per)
}

/// Per-node anomaly rates, a timeline and the most anomalous dimensions in
/// [after, before].
pub fn summarize(
    sources: &[Source],
    nodes: &[String],
    after: i64,
    before: i64,
    top: usize,
    now: i64,
) -> Summary {
    let top = if top == 0 { 30 } else { top };
    const BUCKETS: usize = 60;
    let step = ((before - after + BUCKETS as i64 - 1) / BUCKETS as i64).max(1);
    let mut sum = Summary {
        after,
        before,
        ..Default::default()
    };
    let mut all = Vec::new();
    for src in sources {
        if !match_any(nodes, &src.node) {
            continue;
        }
        let mut nr = NodeRate {
            node: src.node.clone(),
            anomaly_rate: 0.0,
            dimensions: 0,
            anomalous: 0,
            timeline: Vec::new(),
        };
        let mut total = Counts::default();
        let mut per = vec![Counts::default(); BUCKETS];
        for info in src.db.list() {
            if info.last_t != 0 && info.last_t < after {
                continue;
            }
            let (c, pb) = series_counts(&src.db, &info, after, before, now, BUCKETS);
            if c.total == 0 {
                continue;
            }
            nr.dimensions += 1;
            total.anom += c.anom;
            total.total += c.total;
            for (p, b) in per.iter_mut().zip(&pb) {
                p.anom += b.anom;
                p.total += b.total;
            }
            if c.anom > 0 {
                nr.anomalous += 1;
                let rate = 100.0 * c.anom as f64 / c.total as f64;
                all.push(ranked(&src.node, &info, rate, rate));
            }
        }
        if total.total > 0 {
            nr.anomaly_rate = 100.0 * total.anom as f64 / total.total as f64;
        }
        nr.timeline = per
            .iter()
            .enumerate()
            .map(|(i, b)| TimelinePoint {
                t: after + i as i64 * step,
                rate: NullFloat(if b.total > 0 {
                    100.0 * b.anom as f64 / b.total as f64
                } else {
                    f64::NAN
                }),
            })
            .collect();
        sum.nodes.push(nr);
    }
    all.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| (a.chart.clone() + &a.dimension).cmp(&(b.chart.clone() + &b.dimension)))
    });
    all.truncate(top);
    sum.ranked = all;
    sum
}

/// Ranks dimensions by how much their distribution in the highlighted window
/// differs from the baseline just before it (four times as long), by the
/// two-sample Kolmogorov-Smirnov statistic: the "what changed here" view.
pub fn correlate(
    sources: &[Source],
    nodes: &[String],
    h_after: i64,
    h_before: i64,
    top: usize,
    now: i64,
) -> Vec<Ranked> {
    let top = if top == 0 { 30 } else { top };
    let span = h_before - h_after;
    if span <= 0 {
        return Vec::new();
    }
    let b_after = h_after - 4 * span;
    let mut out = Vec::new();
    for src in sources {
        if !match_any(nodes, &src.node) {
            continue;
        }
        let tier0 = b_after >= now - src.db.retention()[0].as_secs() as i64;
        for info in src.db.list() {
            if info.last_t != 0 && info.last_t < h_after {
                continue;
            }
            let (mut base, mut high) = (Vec::new(), Vec::new());
            let (mut anom, mut total) = (0usize, 0usize);
            if tier0 {
                for p in src.db.points(&info.key, b_after, h_before) {
                    if p.t < h_after {
                        base.push(p.v);
                    } else {
                        high.push(p.v);
                        total += 1;
                        anom += usize::from(p.anomalous);
                    }
                }
            } else if let Ok(rs) =
                src.db
                    .rollups(1, std::slice::from_ref(&info.key), b_after, h_before)
            {
                for r in rs.get(&info.key).into_iter().flatten() {
                    if r.start < h_after {
                        base.push(r.avg());
                    } else {
                        high.push(r.avg());
                        total += r.count as usize;
                        anom += r.anomalous as usize;
                    }
                }
            }
            if base.len() < 3 || high.len() < 3 {
                continue;
            }
            let d = ks(&base, &high);
            if d <= 0.0 {
                continue;
            }
            let rate = if total > 0 {
                100.0 * anom as f64 / total as f64
            } else {
                0.0
            };
            out.push(ranked(&src.node, &info, rate, d));
        }
    }
    out.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(b.anomaly_rate.total_cmp(&a.anomaly_rate))
    });
    out.truncate(top);
    out
}

/// Two-sample Kolmogorov-Smirnov statistic: the largest distance between the
/// empirical distribution functions of `a` and `b`.
pub fn ks(a: &[f64], b: &[f64]) -> f64 {
    let mut x = a.to_vec();
    let mut y = b.to_vec();
    x.sort_by(|p, q| p.total_cmp(q));
    y.sort_by(|p, q| p.total_cmp(q));
    let (mut i, mut j, mut d) = (0, 0, 0.0f64);
    while i < x.len() && j < y.len() {
        let v = x[i].min(y[j]);
        while i < x.len() && x[i] <= v {
            i += 1;
        }
        while j < y.len() && y[j] <= v {
            j += 1;
        }
        d = d.max((i as f64 / x.len() as f64 - j as f64 / y.len() as f64).abs());
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsdb::{Options, Sample, Series};
    use std::sync::Arc;

    fn src() -> Vec<Source> {
        let db = Db::open(Options::default()).unwrap();
        let mk = |dim: &str| Series {
            context: "c".into(),
            chart: "c".into(),
            dimension: dim.into(),
            ..Default::default()
        };
        for t in 0..500 {
            let shift = if t >= 400 { 100.0 } else { 0.0 };
            db.append(&Sample {
                series: mk("moves"),
                t: 1000 + t,
                v: shift + (t % 7) as f64,
                a: t >= 400 && t % 2 == 0,
            })
            .unwrap();
            db.append(&Sample {
                series: mk("flat"),
                t: 1000 + t,
                v: (t % 7) as f64,
                a: false,
            })
            .unwrap();
        }
        vec![Source {
            node: "n1".into(),
            db: Arc::new(db),
        }]
    }

    #[test]
    fn summary_rates_and_ranking() {
        let s = summarize(&src(), &[], 1000, 1499, 10, 1500);
        assert_eq!(s.nodes[0].dimensions, 2);
        assert_eq!(s.nodes[0].anomalous, 1);
        assert!((s.nodes[0].anomaly_rate - 5.0).abs() < 1e-9);
        assert_eq!(s.ranked[0].dimension, "moves");
        assert_eq!(s.nodes[0].timeline.len(), 60);
    }

    #[test]
    fn correlate_finds_the_shift() {
        let c = correlate(&src(), &[], 1400, 1499, 10, 1500);
        assert_eq!(c[0].dimension, "moves");
        assert!(c[0].score > 0.9);
        assert!(c.iter().all(|r| r.dimension != "flat" || r.score < 0.2));
    }

    #[test]
    fn ks_bounds() {
        assert_eq!(ks(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]), 0.0);
        assert_eq!(ks(&[1.0, 2.0], &[10.0, 11.0]), 1.0);
    }
}
