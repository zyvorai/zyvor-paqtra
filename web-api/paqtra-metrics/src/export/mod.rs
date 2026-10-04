//! Ships the per-second store to external systems: Prometheus remote write
//! (hand-encoded protobuf + in-tree snappy), OTLP/HTTP JSON and Graphite
//! plaintext. This module builds batches and request bodies; the host sends
//! them with its own HTTP/TCP client. Exporters only read the store.

pub mod snappy;

use crate::tsdb::{match_any, match_glob, Series, Source};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;

/// One downsampled value: unix seconds at the end of the bucket.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportPoint {
    pub t: i64,
    pub v: f64,
}

/// One exported series with its points in time order.
#[derive(Debug, Clone)]
pub struct ExportSeries {
    pub node: String,
    pub series: Series,
    pub points: Vec<ExportPoint>,
}

#[derive(Debug, Clone)]
pub struct Options {
    /// Exported step in seconds; points inside a step are averaged. Default 10.
    pub resolution: i64,
    /// Keeps only matching contexts (globs); empty exports all.
    pub contexts: Vec<String>,
    /// Drops matching contexts after `contexts` is applied.
    pub exclude: Vec<String>,
    /// Caps series per batch. Default 20000.
    pub max_series: usize,
    /// Backlog older than this many seconds is dropped after an outage. Default 900.
    pub max_lag: i64,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            resolution: 10,
            contexts: Vec::new(),
            exclude: Vec::new(),
            max_series: 20_000,
            max_lag: 900,
        }
    }
}

/// Exporter self-report.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub sink: String,
    pub points_sent: u64,
    pub batches: u64,
    pub failures: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_error: String,
    pub last_send: i64,
    pub resolution_seconds: i64,
}

/// Per-series cursors so each bucket is exported once. A failed send leaves
/// the cursors where they were and the same window is retried (bounded by
/// `max_lag`).
pub struct Exporter {
    opts: Options,
    cursor: Mutex<HashMap<String, i64>>,
    status: Mutex<Status>,
}

/// Cursor positions to apply once a batch was delivered.
pub struct Commit(HashMap<String, i64>);

impl Exporter {
    pub fn new(sink: &str, mut opts: Options) -> Self {
        if opts.resolution <= 0 {
            opts.resolution = 10;
        }
        if opts.max_series == 0 {
            opts.max_series = 20_000;
        }
        if opts.max_lag <= 0 {
            opts.max_lag = 900;
        }
        let status = Status {
            sink: sink.into(),
            resolution_seconds: opts.resolution,
            ..Default::default()
        };
        Exporter {
            opts,
            cursor: Mutex::new(HashMap::new()),
            status: Mutex::new(status),
        }
    }

    pub fn resolution(&self) -> i64 {
        self.opts.resolution
    }

    fn wanted(&self, context: &str) -> bool {
        (self.opts.contexts.is_empty() || match_any(&self.opts.contexts, context))
            && !self.opts.exclude.iter().any(|x| match_glob(x, context))
    }

    /// Builds the next batch up to `now` without advancing cursors.
    pub fn collect(&self, sources: &[Source], now: i64) -> (Vec<ExportSeries>, Commit) {
        let res = self.opts.resolution;
        // Only complete buckets: the current one is still filling.
        let end = now - now.rem_euclid(res);
        let floor = end - self.opts.max_lag;
        let cursor = self.cursor.lock().unwrap();
        let mut out = Vec::new();
        let mut next = HashMap::new();
        'nodes: for src in sources {
            let mut infos = src.db.list();
            infos.sort_by(|a, b| a.key.cmp(&b.key));
            for info in infos {
                if out.len() >= self.opts.max_series {
                    break 'nodes;
                }
                if !self.wanted(&info.series.context) {
                    continue;
                }
                let ck = format!("{}|{}", src.node, info.key);
                // New series start with the latest complete bucket.
                let from = cursor.get(&ck).copied().unwrap_or(end - res).max(floor);
                if from >= end {
                    continue;
                }
                let mut points = Vec::new();
                let (mut sum, mut n, mut bucket) = (0.0, 0usize, i64::MIN);
                for p in src.db.points(&info.key, from + 1, end) {
                    if !p.v.is_finite() {
                        continue;
                    }
                    let b = p.t + (res - p.t.rem_euclid(res)) % res;
                    if b != bucket {
                        if n > 0 {
                            points.push(ExportPoint {
                                t: bucket,
                                v: sum / n as f64,
                            });
                        }
                        (sum, n, bucket) = (0.0, 0, b);
                    }
                    sum += p.v;
                    n += 1;
                }
                if n > 0 {
                    points.push(ExportPoint {
                        t: bucket,
                        v: sum / n as f64,
                    });
                }
                if points.is_empty() {
                    continue;
                }
                out.push(ExportSeries {
                    node: src.node.clone(),
                    series: info.series,
                    points,
                });
                next.insert(ck, end);
            }
        }
        (out, Commit(next))
    }

    /// Records a delivered batch and advances its cursors.
    pub fn commit(&self, c: Commit, batch: &[ExportSeries], now: i64) {
        self.cursor.lock().unwrap().extend(c.0);
        let mut st = self.status.lock().unwrap();
        st.points_sent += batch.iter().map(|s| s.points.len() as u64).sum::<u64>();
        st.batches += 1;
        st.last_error.clear();
        st.last_send = now;
    }

    pub fn fail(&self, err: &str) {
        let mut st = self.status.lock().unwrap();
        st.failures += 1;
        st.last_error = err.chars().take(512).collect();
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
}

/// Maps a context to a Prometheus-style name: `paqtra_system_cpu`.
pub fn metric_name(prefix: &str, context: &str) -> String {
    let mut b = String::from(prefix);
    if !prefix.is_empty() && !prefix.ends_with('_') {
        b.push('_');
    }
    b.extend(context.chars().map(|c| {
        if c.is_ascii_alphanumeric() || c == '_' {
            c
        } else {
            '_'
        }
    }));
    b
}

fn label_name(k: &str) -> String {
    k.chars()
        .enumerate()
        .map(|(i, c)| {
            if c.is_ascii_alphabetic() || c == '_' || (i > 0 && c.is_ascii_digit()) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Labels shared by remote write and OTLP: series labels plus node
/// (instance), chart, dimension and family, sorted by name.
pub fn labels_of(s: &ExportSeries) -> Vec<(String, String)> {
    const RESERVED: [&str; 5] = ["__name__", "instance", "chart", "dimension", "family"];
    let mut ls: Vec<(String, String)> = s
        .series
        .labels
        .iter()
        .map(|(k, v)| (label_name(k), v.clone()))
        .filter(|(k, v)| !v.is_empty() && !RESERVED.contains(&k.as_str()))
        .collect();
    ls.push(("instance".into(), s.node.clone()));
    ls.push(("chart".into(), s.series.chart.clone()));
    ls.push(("dimension".into(), s.series.dimension.clone()));
    if !s.series.family.is_empty() {
        ls.push(("family".into(), s.series.family.clone()));
    }
    ls.sort();
    ls
}

fn pb_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn pb_bytes(out: &mut Vec<u8>, field: u64, b: &[u8]) {
    pb_varint(out, field << 3 | 2);
    pb_varint(out, b.len() as u64);
    out.extend_from_slice(b);
}

/// Hand-encodes `prometheus.WriteRequest`:
///
/// ```text
/// WriteRequest { repeated TimeSeries timeseries = 1; }
/// TimeSeries   { repeated Label labels = 1; repeated Sample samples = 2; }
/// Label        { string name = 1; string value = 2; }
/// Sample       { double value = 1; int64 timestamp = 2; } // milliseconds
/// ```
pub fn encode_write_request(prefix: &str, batch: &[ExportSeries]) -> Vec<u8> {
    let mut out = Vec::new();
    for s in batch {
        let mut ts = Vec::new();
        let mut ls = labels_of(s);
        ls.push(("__name__".into(), metric_name(prefix, &s.series.context)));
        ls.sort();
        for (k, v) in ls {
            let mut l = Vec::new();
            pb_bytes(&mut l, 1, k.as_bytes());
            pb_bytes(&mut l, 2, v.as_bytes());
            pb_bytes(&mut ts, 1, &l);
        }
        for p in &s.points {
            let mut sm = vec![1 << 3 | 1];
            sm.extend_from_slice(&p.v.to_bits().to_le_bytes());
            pb_varint(&mut sm, 2 << 3);
            pb_varint(&mut sm, (p.t * 1000) as u64);
            pb_bytes(&mut ts, 2, &sm);
        }
        pb_bytes(&mut out, 1, &ts);
    }
    out
}

/// Snappy-compressed remote-write bodies, at most 2000 series each so a
/// single request stays well under typical receiver limits.
pub fn remote_write_bodies(prefix: &str, batch: &[ExportSeries]) -> Vec<Vec<u8>> {
    batch
        .chunks(2000)
        .map(|c| snappy::encode(&encode_write_request(prefix, c)))
        .collect()
}

pub const REMOTE_WRITE_HEADERS: [(&str, &str); 3] = [
    ("Content-Encoding", "snappy"),
    ("Content-Type", "application/x-protobuf"),
    ("X-Prometheus-Remote-Write-Version", "0.1.0"),
];

fn otlp_attr(k: &str, v: &str) -> serde_json::Value {
    serde_json::json!({"key": k, "value": {"stringValue": v}})
}

/// An OTLP/HTTP JSON `ExportMetricsServiceRequest` grouping series by
/// metric name into gauges.
pub fn otlp_body(
    prefix: &str,
    resource: &[(String, String)],
    batch: &[ExportSeries],
) -> serde_json::Value {
    let mut order: Vec<String> = Vec::new();
    let mut by_name: HashMap<String, serde_json::Value> = HashMap::new();
    for s in batch {
        let name = metric_name(prefix, &s.series.context).replace('_', ".");
        let m = by_name.entry(name.clone()).or_insert_with(|| {
            order.push(name.clone());
            serde_json::json!({"name": name, "unit": s.series.units, "description": s.series.title, "gauge": {"dataPoints": []}})
        });
        let attrs: Vec<_> = labels_of(s).iter().map(|(k, v)| otlp_attr(k, v)).collect();
        let dps = m["gauge"]["dataPoints"]
            .as_array_mut()
            .expect("dataPoints array");
        for p in &s.points {
            dps.push(serde_json::json!({"timeUnixNano": (p.t as i128 * 1_000_000_000).to_string(), "asDouble": p.v, "attributes": attrs}));
        }
    }
    let metrics: Vec<_> = order.iter().filter_map(|n| by_name.remove(n)).collect();
    let mut res = vec![otlp_attr("service.name", "paqtra")];
    res.extend(resource.iter().map(|(k, v)| otlp_attr(k, v)));
    serde_json::json!({"resourceMetrics": [{
        "resource": {"attributes": res},
        "scopeMetrics": [{"scope": {"name": "paqtra.metrics"}, "metrics": metrics}],
    }]})
}

/// OTLP bodies of at most 1000 series each.
pub fn otlp_bodies(
    prefix: &str,
    resource: &[(String, String)],
    batch: &[ExportSeries],
) -> Vec<Vec<u8>> {
    batch
        .chunks(1000)
        .map(|c| serde_json::to_vec(&otlp_body(prefix, resource, c)).unwrap_or_default())
        .collect()
}

/// `{endpoint}/v1/metrics` unless already present.
pub fn otlp_url(endpoint: &str) -> String {
    let u = endpoint.trim_end_matches('/');
    if u.ends_with("/v1/metrics") {
        u.into()
    } else {
        format!("{u}/v1/metrics")
    }
}

fn graphite_part(s: &str) -> String {
    let p: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if p.is_empty() {
        "_".into()
    } else {
        p
    }
}

/// Graphite plaintext: `<prefix>.<node>.<chart>.<dimension> <value> <unix>\n`.
pub fn graphite_lines(prefix: &str, batch: &[ExportSeries]) -> String {
    let mut out = String::new();
    for s in batch {
        let path = [prefix, &s.node, &s.series.chart, &s.series.dimension]
            .map(graphite_part)
            .join(".");
        for p in &s.points {
            out.push_str(&format!("{path} {} {}\n", p.v, p.t));
        }
    }
    out
}

/// Reads `K=V,K2=V2` (the OTEL_EXPORTER_OTLP_HEADERS style).
pub fn parse_headers(s: &str) -> Vec<(String, String)> {
    s.split(',')
        .filter_map(|kv| {
            let (k, v) = kv.trim().split_once('=')?;
            let k = k.trim();
            (!k.is_empty()).then(|| (k.to_string(), v.trim().to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsdb::{Db, Options as DbOptions, Sample};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn sources() -> Vec<Source> {
        let db = Db::open(DbOptions::default()).unwrap();
        let mk = |ctx: &str, dim: &str| Series {
            context: ctx.into(),
            chart: ctx.into(),
            dimension: dim.into(),
            units: "%".into(),
            labels: BTreeMap::from([("k8s.namespace".into(), "prod".into())]),
            ..Default::default()
        };
        for t in 1000..1030 {
            db.append(&Sample {
                series: mk("system.cpu", "user"),
                t,
                v: t as f64,
                a: false,
            })
            .unwrap();
            db.append(&Sample {
                series: mk("mem.swap", "used"),
                t,
                v: 1.0,
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
    fn buckets_cursors_and_filters() {
        let src = sources();
        let ex = Exporter::new(
            "test",
            Options {
                contexts: vec!["system.*".into()],
                ..Default::default()
            },
        );
        let (b, c) = ex.collect(&src, 1025);
        assert_eq!(b.len(), 1, "mem.swap filtered out");
        // First batch is the latest complete bucket: (1010, 1020].
        assert_eq!(b[0].points, vec![ExportPoint { t: 1020, v: 1015.5 }]);
        ex.commit(c, &b, 1025);
        let (b2, _) = ex.collect(&src, 1025);
        assert!(b2.is_empty(), "nothing new until the next bucket completes");
        let (b3, _) = ex.collect(&src, 1031);
        assert_eq!(b3[0].points.len(), 1);
        assert_eq!(b3[0].points[0].t, 1030);
        assert_eq!(ex.status().points_sent, 1);
        ex.fail("boom");
        assert_eq!(ex.status().failures, 1);
    }

    #[test]
    fn remote_write_protobuf_shape() {
        let src = sources();
        let ex = Exporter::new(
            "rw",
            Options {
                contexts: vec!["system.cpu".into()],
                ..Default::default()
            },
        );
        let (b, _) = ex.collect(&src, 1025);
        let body = encode_write_request("paqtra", &b);
        let raw = snappy::decode(&remote_write_bodies("paqtra", &b)[0]).unwrap();
        assert_eq!(raw, body);
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("paqtra_system_cpu"));
        assert!(text.contains("k8s_namespace"));
        assert!(text.contains("instance"));
        assert_eq!(body[0], 0x0a, "field 1, length-delimited");
    }

    #[test]
    fn otlp_and_graphite() {
        let src = sources();
        let ex = Exporter::new("x", Options::default());
        let (b, _) = ex.collect(&src, 1025);
        let v = otlp_body("paqtra", &[("k8s.cluster.name".into(), "lab".into())], &b);
        let metrics = &v["resourceMetrics"][0]["scopeMetrics"][0]["metrics"];
        assert_eq!(metrics.as_array().unwrap().len(), 2);
        assert!(metrics
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["name"] == "paqtra.system.cpu"));
        assert_eq!(
            metrics[0]["gauge"]["dataPoints"][0]["timeUnixNano"],
            "1020000000000"
        );
        let g = graphite_lines("paqtra", &b);
        assert!(g.contains("paqtra.n1.system_cpu.user 1015.5 1020\n"), "{g}");
        assert_eq!(otlp_url("http://c:4318/"), "http://c:4318/v1/metrics");
        assert_eq!(
            parse_headers("a=1, b = 2,bad"),
            vec![("a".into(), "1".into()), ("b".into(), "2".into())]
        );
    }
}
