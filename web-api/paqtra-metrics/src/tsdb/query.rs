use super::db::{Db, SeriesInfo, TIER0_RESOLUTION, TIER1_RESOLUTION, TIER2_RESOLUTION};
use super::series::{match_any, match_glob, NullFloat, Rollup};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

/// One node's DB in a multi-node query.
#[derive(Clone)]
pub struct Source {
    pub node: String,
    pub db: Arc<Db>,
}

/// Selects series of one context and reduces them to evenly spaced points.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    pub context: String,
    #[serde(default)]
    pub charts: Vec<String>,
    #[serde(default)]
    pub dimensions: Vec<String>,
    #[serde(default)]
    pub nodes: Vec<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// Unix seconds; <= 0 is relative to `before`.
    #[serde(default)]
    pub after: i64,
    /// Unix seconds; <= 0 is now plus `before`.
    #[serde(default)]
    pub before: i64,
    #[serde(default)]
    pub points: usize,
    /// Reduction inside one bucket: avg, min, max, sum, last, p50, p90, p95,
    /// p99. Percentiles need tier 0 and fall back to max.
    #[serde(default)]
    pub group: String,
    /// dimension (default), chart, node, instance, all, or label:<key>.
    #[serde(default)]
    pub group_by: String,
    /// Merge of series in one group: sum, avg, min, max. Default avg for
    /// percentage units, sum otherwise.
    #[serde(default)]
    pub aggregate: String,
    #[serde(default)]
    pub tier: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultDim {
    pub name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    pub values: Vec<NullFloat>,
    pub anomaly_rate: Vec<NullFloat>,
    pub series: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub context: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub units: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub family: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub chart_type: String,
    pub tier: usize,
    pub interval: i64,
    pub after: i64,
    pub before: i64,
    pub timestamps: Vec<i64>,
    pub dimensions: Vec<ResultDim>,
    pub matched: usize,
}

struct Matched<'a> {
    node: &'a str,
    db: &'a Arc<Db>,
    info: SeriesInfo,
}

impl Query {
    /// Resolves relative after/before against `now` and fills defaults.
    pub fn normalize(&mut self, now: i64) {
        if self.before <= 0 {
            self.before += now;
        }
        if self.after <= 0 {
            if self.after == 0 {
                self.after = -600;
            }
            self.after += self.before;
        }
        if self.after >= self.before {
            self.after = self.before - 1;
        }
        if self.points == 0 {
            self.points = (self.before - self.after).min(600) as usize;
        }
        self.points = self.points.min(3000);
        if self.group.is_empty() {
            self.group = "avg".into();
        }
        if self.group_by.is_empty() {
            self.group_by = "dimension".into();
        }
    }
}

fn labels_match(
    want: &BTreeMap<String, String>,
    have: &BTreeMap<String, String>,
    node: &str,
) -> bool {
    want.iter().all(|(k, v)| {
        let hv = if k == "node" {
            node
        } else {
            have.get(k).map(String::as_str).unwrap_or("")
        };
        match_glob(v, hv)
    })
}

fn pick_tier(q: &Query, now: i64, retention: [Duration; 3]) -> usize {
    if let Some(t) = q.tier.filter(|t| *t <= 2) {
        return t;
    }
    if q.after >= now - retention[0].as_secs() as i64 {
        0
    } else if q.after >= now - retention[1].as_secs() as i64 {
        1
    } else {
        2
    }
}

pub fn run(sources: &[Source], mut q: Query, now: i64) -> Result<QueryResult, String> {
    if q.context.is_empty() {
        return Err("context is required".into());
    }
    q.normalize(now);
    let mut ms = Vec::new();
    for src in sources {
        if !match_any(&q.nodes, &src.node) {
            continue;
        }
        for info in src.db.list() {
            let s = &info.series;
            if s.context != q.context
                || !match_any(&q.charts, &s.chart)
                || !match_any(&q.dimensions, &s.dimension)
                || !labels_match(&q.labels, &s.labels, &src.node)
                || (info.last_t != 0 && info.last_t < q.after)
            {
                continue;
            }
            ms.push(Matched {
                node: &src.node,
                db: &src.db,
                info,
            });
        }
    }
    let mut res = QueryResult {
        context: q.context.clone(),
        after: q.after,
        before: q.before,
        matched: ms.len(),
        ..Default::default()
    };
    if ms.is_empty() {
        return Ok(res);
    }
    let first = ms[0].info.series.clone();
    res.title = first.title.clone();
    res.units = first.units.clone();
    res.family = first.family.clone();
    res.chart_type = first.chart_type.clone();
    let tier = pick_tier(&q, now, ms[0].db.retention());
    res.tier = tier;
    let resolution = [TIER0_RESOLUTION, TIER1_RESOLUTION, TIER2_RESOLUTION][tier];
    let mut step = (q.before - q.after + q.points as i64 - 1) / q.points as i64;
    step = resolution.max((step + resolution - 1) / resolution * resolution);
    let start = q.after - q.after.rem_euclid(step);
    let n = ((q.before - start) / step) as usize + 1;
    res.interval = step;
    res.timestamps = (0..n).map(|i| start + i as i64 * step).collect();
    let agg = if q.aggregate.is_empty() {
        if first.units == "%" || first.units.starts_with("percent") {
            "avg".to_string()
        } else {
            "sum".to_string()
        }
    } else {
        q.aggregate.clone()
    };

    let mut rollups: HashMap<*const Db, HashMap<String, Vec<Rollup>>> = HashMap::new();
    if tier > 0 {
        let mut keys_by_db: HashMap<*const Db, (&Arc<Db>, Vec<String>)> = HashMap::new();
        for m in &ms {
            keys_by_db
                .entry(Arc::as_ptr(m.db))
                .or_insert_with(|| (m.db, Vec::new()))
                .1
                .push(m.info.key.clone());
        }
        for (ptr, (db, keys)) in keys_by_db {
            let r = db
                .rollups(tier, &keys, start, q.before)
                .map_err(|e| e.to_string())?;
            rollups.insert(ptr, r);
        }
    }

    struct Group {
        dim: ResultDim,
        vals: Vec<Vec<f64>>,
        anom: Vec<u32>,
        count: Vec<u32>,
        members: usize,
    }
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for m in &ms {
        let (name, labels) = group_key(&q.group_by, m);
        let g = groups.entry(name.clone()).or_insert_with(|| Group {
            dim: ResultDim {
                name,
                labels,
                values: Vec::new(),
                anomaly_rate: Vec::new(),
                series: 0,
            },
            vals: vec![Vec::new(); n],
            anom: vec![0; n],
            count: vec![0; n],
            members: 0,
        });
        g.members += 1;
        let per = bucket_series(m, tier, &rollups, start, step, n, &q);
        for i in 0..n {
            if per.count[i] == 0 {
                continue;
            }
            g.vals[i].push(per.value[i]);
            g.anom[i] += per.anom[i];
            g.count[i] += per.count[i];
        }
    }
    for (_, mut g) in groups {
        g.dim.series = g.members;
        g.dim.values = g.vals.iter().map(|v| NullFloat(combine(&agg, v))).collect();
        g.dim.anomaly_rate = (0..n)
            .map(|i| {
                if g.count[i] == 0 {
                    NullFloat(f64::NAN)
                } else {
                    NullFloat(100.0 * g.anom[i] as f64 / g.count[i] as f64)
                }
            })
            .collect();
        res.dimensions.push(g.dim);
    }
    Ok(res)
}

fn group_key(by: &str, m: &Matched) -> (String, BTreeMap<String, String>) {
    let s = &m.info.series;
    let node = || BTreeMap::from([("node".to_string(), m.node.to_string())]);
    match by {
        "chart" => (s.chart.clone(), node()),
        "node" => (m.node.to_string(), node()),
        "instance" => {
            let mut l = node();
            l.insert("chart".into(), s.chart.clone());
            (format!("{}/{}/{}", m.node, s.chart, s.dimension), l)
        }
        "all" => ("all".into(), BTreeMap::new()),
        _ if by.starts_with("label:") => {
            let k = &by["label:".len()..];
            let mut v = if k == "node" {
                m.node.to_string()
            } else {
                s.labels.get(k).cloned().unwrap_or_default()
            };
            if v.is_empty() {
                v = "(none)".into();
            }
            (v.clone(), BTreeMap::from([(k.to_string(), v)]))
        }
        _ => (s.dimension.clone(), BTreeMap::new()),
    }
}

struct Bucketed {
    value: Vec<f64>,
    anom: Vec<u32>,
    count: Vec<u32>,
}

fn bucket_series(
    m: &Matched,
    tier: usize,
    rollups: &HashMap<*const Db, HashMap<String, Vec<Rollup>>>,
    start: i64,
    step: i64,
    n: usize,
    q: &Query,
) -> Bucketed {
    let mut out = Bucketed {
        value: vec![0.0; n],
        anom: vec![0; n],
        count: vec![0; n],
    };
    let idx = |t: i64| -> Option<usize> {
        let i = (t - start).div_euclid(step);
        (i >= 0 && (i as usize) < n).then_some(i as usize)
    };
    if tier == 0 {
        let mut vals: Vec<Vec<f64>> = vec![Vec::new(); n];
        for p in m.db.points(&m.info.key, start, q.before) {
            let Some(i) = idx(p.t) else { continue };
            vals[i].push(p.v);
            out.count[i] += 1;
            if p.anomalous {
                out.anom[i] += 1;
            }
        }
        for (i, v) in vals.iter().enumerate() {
            if !v.is_empty() {
                out.value[i] = reduce(&q.group, v);
            }
        }
        return out;
    }
    let mut acc = vec![Rollup::default(); n];
    let mut last = vec![0.0; n];
    let rows = rollups
        .get(&Arc::as_ptr(m.db))
        .and_then(|r| r.get(&m.info.key));
    for r in rows.into_iter().flatten() {
        let Some(i) = idx(r.start) else { continue };
        if r.count == 0 {
            continue;
        }
        let a = &mut acc[i];
        if a.count == 0 {
            a.min = r.min;
            a.max = r.max;
        } else {
            a.min = a.min.min(r.min);
            a.max = a.max.max(r.max);
        }
        a.sum += r.sum;
        a.count += r.count;
        a.anomalous += r.anomalous;
        last[i] = r.avg();
    }
    for (i, a) in acc.iter().enumerate() {
        if a.count == 0 {
            continue;
        }
        out.count[i] = a.count;
        out.anom[i] = a.anomalous;
        out.value[i] = match q.group.as_str() {
            "min" => a.min,
            "max" | "p50" | "p90" | "p95" | "p99" => a.max,
            "sum" => a.sum,
            "last" => last[i],
            _ => a.avg(),
        };
    }
    out
}

pub fn reduce(group: &str, vals: &[f64]) -> f64 {
    match group {
        "min" => vals.iter().copied().fold(f64::INFINITY, f64::min),
        "max" => vals.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        "sum" => vals.iter().sum(),
        "last" => *vals.last().unwrap(),
        "p50" => percentile(vals, 0.5),
        "p90" => percentile(vals, 0.9),
        "p95" => percentile(vals, 0.95),
        "p99" => percentile(vals, 0.99),
        _ => vals.iter().sum::<f64>() / vals.len() as f64,
    }
}

fn combine(agg: &str, vals: &[f64]) -> f64 {
    if vals.is_empty() {
        return f64::NAN;
    }
    match agg {
        "avg" | "min" | "max" => reduce(agg, vals),
        _ => reduce("sum", vals),
    }
}

/// Nearest-rank p-quantile (0..1).
pub fn percentile(vals: &[f64], p: f64) -> f64 {
    if vals.is_empty() {
        return f64::NAN;
    }
    let mut c = vals.to_vec();
    c.sort_by(|a, b| a.total_cmp(b));
    let idx = ((p * c.len() as f64).ceil() as isize - 1).clamp(0, c.len() as isize - 1);
    c[idx as usize]
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextInfo {
    pub context: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub family: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub units: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub chart_type: String,
    pub charts: Vec<ChartInfo>,
    pub first_t: i64,
    pub last_t: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChartInfo {
    pub node: String,
    pub chart: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    pub dimensions: Vec<String>,
}

/// Every context with its chart instances.
pub fn contexts(sources: &[Source], nodes: &[String]) -> Vec<ContextInfo> {
    let mut by_ctx: BTreeMap<String, ContextInfo> = BTreeMap::new();
    let mut charts: BTreeMap<(String, String, String), ChartInfo> = BTreeMap::new();
    for src in sources {
        if !match_any(nodes, &src.node) {
            continue;
        }
        for info in src.db.list() {
            let s = &info.series;
            let ci = by_ctx
                .entry(s.context.clone())
                .or_insert_with(|| ContextInfo {
                    context: s.context.clone(),
                    first_t: info.first_t,
                    ..Default::default()
                });
            if !s.family.is_empty() {
                ci.family.clone_from(&s.family);
                ci.title.clone_from(&s.title);
                ci.units.clone_from(&s.units);
                ci.chart_type.clone_from(&s.chart_type);
            }
            if info.first_t != 0 && (ci.first_t == 0 || info.first_t < ci.first_t) {
                ci.first_t = info.first_t;
            }
            ci.last_t = ci.last_t.max(info.last_t);
            charts
                .entry((s.context.clone(), src.node.clone(), s.chart.clone()))
                .or_insert_with(|| ChartInfo {
                    node: src.node.clone(),
                    chart: s.chart.clone(),
                    labels: s.labels.clone(),
                    dimensions: Vec::new(),
                })
                .dimensions
                .push(s.dimension.clone());
        }
    }
    for ((ctx, _, _), mut ch) in charts {
        ch.dimensions.sort();
        if let Some(ci) = by_ctx.get_mut(&ctx) {
            ci.charts.push(ch);
        }
    }
    by_ctx.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsdb::{Options, Sample, Series};

    fn db_with(node_vals: &[(&str, f64)]) -> Vec<Source> {
        node_vals
            .iter()
            .map(|(node, base)| {
                let db = Db::open(Options::default()).unwrap();
                for dim in ["user", "system"] {
                    let s = Series {
                        context: "system.cpu".into(),
                        chart: "system.cpu".into(),
                        dimension: dim.into(),
                        units: "percentage".into(),
                        family: "cpu".into(),
                        ..Default::default()
                    };
                    for t in 0..120 {
                        db.append(&Sample {
                            series: s.clone(),
                            t: 1000 + t,
                            v: base + t as f64,
                            a: t % 10 == 0,
                        })
                        .unwrap();
                    }
                }
                Source {
                    node: node.to_string(),
                    db: Arc::new(db),
                }
            })
            .collect()
    }

    #[test]
    fn query_buckets_groups_and_anomaly_rate() {
        let src = db_with(&[("n1", 0.0), ("n2", 100.0)]);
        let q = Query {
            context: "system.cpu".into(),
            after: 1000,
            before: 1119,
            points: 12,
            ..Default::default()
        };
        let r = run(&src, q.clone(), 1120).unwrap();
        assert_eq!(r.matched, 4);
        assert_eq!(r.interval, 10);
        assert_eq!(r.tier, 0);
        assert_eq!(r.dimensions.len(), 2);
        let user = r.dimensions.iter().find(|d| d.name == "user").unwrap();
        assert_eq!(user.series, 2);
        // avg of node buckets (percentage units), each bucket avg of 10 seconds
        assert!((user.values[0].0 - 54.5).abs() < 1e-9);
        assert!((user.anomaly_rate[0].0 - 10.0).abs() < 1e-9);

        let mut by_node = q.clone();
        by_node.group_by = "node".into();
        let r = run(&src, by_node, 1120).unwrap();
        assert_eq!(
            r.dimensions
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>(),
            ["n1", "n2"]
        );

        let mut tier1 = q;
        tier1.tier = Some(1);
        tier1.group = "max".into();
        let r = run(&src, tier1, 1120).unwrap();
        assert_eq!(r.interval, 60);
        assert_eq!(r.tier, 1);
    }

    #[test]
    fn contexts_list_charts_per_node() {
        let src = db_with(&[("n1", 0.0), ("n2", 0.0)]);
        let c = contexts(&src, &[]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].charts.len(), 2);
        assert_eq!(c[0].charts[0].dimensions, ["system", "user"]);
        assert_eq!(contexts(&src, &["n2".into()])[0].charts.len(), 1);
    }

    #[test]
    fn percentiles() {
        let v = [5.0, 1.0, 3.0, 2.0, 4.0];
        assert_eq!(percentile(&v, 0.5), 3.0);
        assert_eq!(percentile(&v, 0.99), 5.0);
        assert!(percentile(&[], 0.5).is_nan());
    }
}
