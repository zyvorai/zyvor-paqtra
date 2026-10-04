//! Generic Prometheus text-format scraper and the presets built on it.

use super::apps::{App, AppConfig, HttpGet};
use super::Emitter;
use crate::tsdb::{match_any, match_glob};
use std::collections::HashMap;

pub(crate) const ENVOY_INCLUDE: &[&str] = &[
    "envoy_server_live",
    "envoy_server_uptime",
    "envoy_server_memory_allocated",
    "envoy_server_total_connections",
    "envoy_cluster_upstream_cx_active",
    "envoy_cluster_upstream_cx_total",
    "envoy_cluster_upstream_cx_connect_fail",
    "envoy_cluster_upstream_rq_total",
    "envoy_cluster_upstream_rq_active",
    "envoy_cluster_upstream_rq_pending_active",
    "envoy_cluster_upstream_rq_timeout",
    "envoy_cluster_upstream_rq_retry",
    "envoy_cluster_upstream_rq_xx",
    "envoy_cluster_membership_healthy",
    "envoy_cluster_membership_total",
    "envoy_http_downstream_cx_active",
    "envoy_http_downstream_rq_total",
    "envoy_http_downstream_rq_xx",
    "envoy_http_downstream_rq_active",
    "envoy_listener_downstream_cx_active",
    "envoy_listener_downstream_cx_total",
];

pub(crate) const COREDNS_INCLUDE: &[&str] = &[
    "coredns_dns_requests_total",
    "coredns_dns_responses_total",
    "coredns_dns_request_duration_seconds",
    "coredns_cache_entries",
    "coredns_cache_hits_total",
    "coredns_cache_misses_total",
    "coredns_forward_requests_total",
    "coredns_forward_responses_total",
    "coredns_forward_healthcheck_failures_total",
    "coredns_panics_total",
    "coredns_plugin_enabled",
];

pub(crate) const ETCD_INCLUDE: &[&str] = &[
    "etcd_server_has_leader",
    "etcd_server_leader_changes_seen_total",
    "etcd_server_proposals_*",
    "etcd_mvcc_db_total_size_in_bytes",
    "etcd_mvcc_db_total_size_in_use_in_bytes",
    "etcd_debugging_mvcc_keys_total",
    "etcd_disk_wal_fsync_duration_seconds",
    "etcd_disk_backend_commit_duration_seconds",
    "etcd_network_peer_round_trip_time_seconds",
    "etcd_network_client_grpc_*_bytes_total",
    "grpc_server_handled_total",
];

#[derive(Debug, Clone, PartialEq)]
pub struct PromSample {
    pub name: String,
    pub labels: Vec<(String, String)>,
    pub value: f64,
}

/// Parses the text exposition format into samples plus the declared TYPE of
/// each metric family.
pub fn parse_prometheus_text(text: &str) -> (Vec<PromSample>, HashMap<String, String>) {
    let mut types = HashMap::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() >= 4 && f[1] == "TYPE" {
                types.insert(f[2].to_string(), f[3].to_string());
            }
            continue;
        }
        if let Some(s) = parse_line(line) {
            out.push(s);
        }
    }
    (out, types)
}

fn parse_line(line: &str) -> Option<PromSample> {
    let i = line.find(['{', ' ', '\t'])?;
    if i == 0 {
        return None;
    }
    let name = line[..i].to_string();
    let mut rest = &line[i..];
    let mut labels = Vec::new();
    if rest.starts_with('{') {
        let b = rest.as_bytes();
        let (mut j, mut in_q, mut end) = (1, false, None);
        while j < b.len() {
            match b[j] {
                b'\\' => j += 1,
                b'"' => in_q = !in_q,
                b'}' if !in_q => {
                    end = Some(j);
                    break;
                }
                _ => {}
            }
            j += 1;
        }
        let end = end?;
        labels = parse_labels(&rest[1..end]);
        rest = &rest[end + 1..];
    }
    let value = rest.split_whitespace().next()?;
    let value = match value {
        "+Inf" => f64::INFINITY,
        "-Inf" => f64::NEG_INFINITY,
        v => v.parse().ok()?,
    };
    Some(PromSample {
        name,
        labels,
        value,
    })
}

fn parse_labels(mut s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    loop {
        s = s.trim_start_matches([' ', ',']);
        let Some(eq) = s.find('=') else { break };
        if eq == 0 || !s[eq + 1..].starts_with('"') {
            break;
        }
        let k = s[..eq].trim().to_string();
        let mut v = String::new();
        let mut chars = s[eq + 2..].char_indices();
        let mut end = None;
        while let Some((j, c)) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some((_, 'n')) => v.push('\n'),
                    Some((_, o)) => v.push(o),
                    None => break,
                },
                '"' => {
                    end = Some(eq + 2 + j);
                    break;
                }
                c => v.push(c),
            }
        }
        out.push((k, v));
        match end {
            Some(e) => s = &s[e + 1..],
            None => break,
        }
    }
    out.sort();
    out
}

/// Strips histogram/summary suffixes to find the declared family.
fn family_name<'a>(name: &'a str, types: &HashMap<String, String>) -> (&'a str, &'static str) {
    for suf in ["_bucket", "_sum", "_count", "_total", "_created"] {
        if let Some(base) = name.strip_suffix(suf) {
            if types.contains_key(base) {
                return (base, suf);
            }
        }
    }
    (name, "")
}

fn dim_name(labels: &[(String, String)]) -> String {
    if labels.is_empty() {
        return "value".into();
    }
    labels
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Scrapes a Prometheus text endpoint. Counters become per-second rates,
/// gauges stay absolute; histograms and summaries contribute `_sum` and
/// `_count` rates plus a lifetime mean. Buckets and quantiles are skipped to
/// bound cardinality.
pub(crate) struct PromApp {
    cfg: AppConfig,
}

impl PromApp {
    pub fn new(cfg: AppConfig) -> Self {
        Self { cfg }
    }

    fn wanted(&self, name: &str) -> bool {
        (self.cfg.include.is_empty() || match_any(&self.cfg.include, name))
            && !self.cfg.exclude.iter().any(|x| match_glob(x, name))
    }
}

impl App for PromApp {
    fn collect(&mut self, http: &dyn HttpGet, e: &mut Emitter) -> Result<(), String> {
        let body = self.cfg.get(http, &self.cfg.url)?;
        let (samples, types) = parse_prometheus_text(&String::from_utf8_lossy(&body));
        if samples.is_empty() {
            return Err(format!("{}: no metrics in response", self.cfg.url));
        }
        let c = &self.cfg;
        let mut hists: HashMap<(String, String), (f64, f64, u8)> = HashMap::new();
        let mut n = 0;
        for s in &samples {
            let (fam, suf) = family_name(&s.name, &types);
            if !self.wanted(fam) && !self.wanted(&s.name) {
                continue;
            }
            if n >= c.max_series {
                break;
            }
            if s.value.is_nan() {
                continue;
            }
            let dim = dim_name(&s.labels);
            match types.get(fam).map(String::as_str) {
                Some("histogram" | "summary") => {
                    let quantile = s.labels.iter().any(|(k, _)| k == "quantile");
                    if suf == "_bucket" || suf == "_created" || (suf.is_empty() && quantile) {
                        continue;
                    }
                    let h = hists.entry((fam.to_string(), dim.clone())).or_default();
                    match suf {
                        "_sum" => {
                            h.0 = s.value;
                            h.2 |= 1;
                            e.incremental(
                                &c.chart(
                                    &format!("{fam}_sum"),
                                    "apps",
                                    "units/s",
                                    &format!("{fam} sum rate"),
                                    "line",
                                ),
                                &dim,
                                s.value,
                                1.0,
                            );
                        }
                        "_count" => {
                            h.1 = s.value;
                            h.2 |= 2;
                            e.incremental(
                                &c.chart(
                                    &format!("{fam}_count"),
                                    "apps",
                                    "events/s",
                                    &format!("{fam} event rate"),
                                    "line",
                                ),
                                &dim,
                                s.value,
                                1.0,
                            );
                        }
                        _ => {}
                    }
                }
                Some("counter") => e.incremental(
                    &c.chart(&s.name, "apps", "events/s", &s.name, "line"),
                    &dim,
                    s.value,
                    1.0,
                ),
                _ => e.gauge(
                    &c.chart(&s.name, "apps", "value", &s.name, "line"),
                    &dim,
                    s.value,
                ),
            }
            n += 1;
        }
        for ((fam, dim), (sum, count, has)) in hists {
            if has == 3 && count > 0.0 {
                e.gauge(
                    &c.chart(
                        &format!("{fam}_mean"),
                        "apps",
                        "value",
                        &format!("{fam} lifetime mean"),
                        "line",
                    ),
                    &dim,
                    sum / count,
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::apps::{AppKind, HttpGetFn};
    use super::*;

    const TEXT: &str = r#"
# HELP http_requests_total Requests.
# TYPE http_requests_total counter
http_requests_total{code="200",method="get"} 100
http_requests_total{method="post",code="500"} 3
# TYPE temp gauge
temp 21.5
# TYPE rq_duration histogram
rq_duration_bucket{le="0.1"} 5
rq_duration_bucket{le="+Inf"} 10
rq_duration_sum 2.5
rq_duration_count 10
weird{a="x\"y",b="line\nbreak"} 1 1700000000
"#;

    #[test]
    fn parses_text_format() {
        let (s, types) = parse_prometheus_text(TEXT);
        assert_eq!(types["http_requests_total"], "counter");
        assert_eq!(
            s[1].labels,
            vec![
                ("code".into(), "500".into()),
                ("method".into(), "post".into())
            ]
        );
        assert_eq!(s[4].labels, vec![("le".into(), "+Inf".into())]);
        assert_eq!(s[5].value, 2.5);
        assert_eq!(parse_line("x +Inf").unwrap().value, f64::INFINITY);
        let w = s.iter().find(|x| x.name == "weird").unwrap();
        assert_eq!(w.labels[0].1, "x\"y");
        assert_eq!(w.labels[1].1, "line\nbreak");
    }

    #[test]
    fn scrape_rates_gauges_and_histogram_mean() {
        let http = HttpGetFn(|_: &str, _: &[(String, String)], _, _| Ok(TEXT.as_bytes().to_vec()));
        let mut cfg = AppConfig::new(AppKind::Prometheus, "svc", "http://x/metrics");
        cfg.normalize().unwrap();
        cfg.exclude = vec!["weird".into()];
        let mut p = PromApp::new(cfg);
        let mut e = Emitter::new();
        e.begin(1000);
        p.collect(&http, &mut e).unwrap();
        let ctxs: Vec<_> = e
            .samples()
            .iter()
            .map(|s| s.series.context.as_str())
            .collect();
        assert!(ctxs.contains(&"prometheus.temp"));
        assert!(ctxs.contains(&"prometheus.rq_duration_mean"));
        assert!(!ctxs
            .iter()
            .any(|c| c.contains("bucket") || c.contains("weird")));
        assert!(
            !ctxs.contains(&"prometheus.http_requests_total"),
            "first scrape has no rate yet"
        );
    }
}
