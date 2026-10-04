//! `paqtra metrics` — read the per-second metrics platform through the API:
//! status, nodes, contexts, queries, a node top view, anomalies and metric
//! alerts. Read-only.

use anyhow::{bail, Context, Result};
use owo_colors::OwoColorize;
use serde_json::Value;
use std::time::Duration;

/// Where and how to reach the API.
pub struct Api {
    pub url: String,
    pub token: Option<String>,
}

impl Api {
    fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let agent = ureq::Agent::new_with_config(
            ureq::config::Config::builder()
                .timeout_global(Some(Duration::from_secs(30)))
                .http_status_as_error(false)
                .build(),
        );
        let url = format!("{}{path}", self.url.trim_end_matches('/'));
        let mut req = agent.get(&url);
        for (k, v) in query {
            if !v.is_empty() {
                req = req.query(*k, v);
            }
        }
        if let Some(t) = &self.token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        let mut resp = req.call().with_context(|| format!("GET {url}"))?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().read_to_string().unwrap_or_default();
        let v: Value = serde_json::from_str(&body).unwrap_or(Value::String(body.clone()));
        if status >= 300 {
            let msg = v
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or(body.trim());
            bail!("GET {path}: HTTP {status}: {msg}");
        }
        Ok(v)
    }
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn fmt_num(v: Option<f64>) -> String {
    match v {
        None => "-".into(),
        Some(x) if x.abs() >= 1e9 => format!("{:.2}G", x / 1e9),
        Some(x) if x.abs() >= 1e6 => format!("{:.2}M", x / 1e6),
        Some(x) if x.abs() >= 1e4 => format!("{:.1}k", x / 1e3),
        Some(x) if x.abs() >= 100.0 || x == x.trunc() => format!("{x:.0}"),
        Some(x) => format!("{x:.2}"),
    }
}

/// Unicode sparkline of the non-null values.
pub fn sparkline(vals: &[Option<f64>]) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let present: Vec<f64> = vals.iter().flatten().copied().collect();
    if present.is_empty() {
        return String::new();
    }
    let (lo, hi) = present
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| {
            (a.min(x), b.max(x))
        });
    vals.iter()
        .map(|v| match v {
            None => ' ',
            Some(x) if hi > lo => BARS[(((x - lo) / (hi - lo)) * 7.0).round() as usize],
            Some(_) => BARS[0],
        })
        .collect()
}

fn print_json(v: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

pub fn cmd_status(api: &Api, output: &str) -> Result<()> {
    let v = api.get("/api/v1/metrics/status", &[])?;
    if output == "json" {
        return print_json(&v);
    }
    println!("{}", "Metrics platform".bold());
    println!("  nodes streaming   {}", v["nodes"]);
    println!(
        "  ingest auth       {}",
        v["ingestAuth"].as_str().unwrap_or("-")
    );
    println!(
        "  hubble flows seen {}",
        v["hubble"]["flowsObserved"].as_u64().unwrap_or(0)
    );
    if let Some(a) = v["alerts"].as_object() {
        println!(
            "  metric alerts     {} rules, {} warning, {} critical",
            a["rules"], a["warning"], a["critical"]
        );
    } else if let Some(e) = v["alertsError"].as_str() {
        println!("  metric alerts     {}", e.yellow());
    }
    let ex = v["exporters"].as_array().cloned().unwrap_or_default();
    if ex.is_empty() {
        println!("  exporters         {}", "none configured".dimmed());
    }
    for e in ex {
        println!(
            "  exporter {:24} sent {} points, {} failures {}",
            e["sink"].as_str().unwrap_or("-"),
            e["pointsSent"],
            e["failures"],
            e["lastError"].as_str().unwrap_or("").red()
        );
    }
    Ok(())
}

pub fn cmd_nodes(api: &Api, output: &str) -> Result<()> {
    let v = api.get("/api/v1/metrics/nodes", &[])?;
    if output == "json" {
        return print_json(&v);
    }
    println!(
        "{:32} {:>8} {:>12} {}",
        "NODE".bold(),
        "SERIES".bold(),
        "MEMORY".bold(),
        "LAST INGEST".bold()
    );
    let now = chrono::Utc::now().timestamp();
    for n in v["nodes"].as_array().cloned().unwrap_or_default() {
        let last = n["lastIngest"].as_i64().unwrap_or(0);
        let age = if last > 0 {
            format!("{}s ago", now - last)
        } else {
            "-".into()
        };
        println!(
            "{:32} {:>8} {:>12} {}",
            n["node"].as_str().unwrap_or("-"),
            n["stats"]["series"],
            fmt_num(num(&n["stats"]["memoryBytes"])),
            age
        );
    }
    Ok(())
}

pub fn cmd_contexts(api: &Api, filter: &str, nodes: &str, output: &str) -> Result<()> {
    let v = api.get(
        "/api/v1/metrics/contexts",
        &[("q", filter.to_string()), ("nodes", nodes.to_string())],
    )?;
    if output == "json" {
        return print_json(&v);
    }
    println!(
        "{:40} {:>7} {:>6} {:12} {}",
        "CONTEXT".bold(),
        "CHARTS".bold(),
        "NODES".bold(),
        "UNITS".bold(),
        "TITLE".bold()
    );
    for c in v["contexts"].as_array().cloned().unwrap_or_default() {
        let charts = c["charts"].as_array().cloned().unwrap_or_default();
        let nodes: std::collections::BTreeSet<&str> =
            charts.iter().filter_map(|ch| ch["node"].as_str()).collect();
        println!(
            "{:40} {:>7} {:>6} {:12} {}",
            c["context"].as_str().unwrap_or("-"),
            charts.len(),
            nodes.len(),
            c["units"].as_str().unwrap_or(""),
            c["title"].as_str().unwrap_or("").dimmed()
        );
    }
    println!("{} {} contexts", "→".cyan(), v["total"]);
    Ok(())
}

pub struct QueryOpts<'a> {
    pub context: &'a str,
    pub after: i64,
    pub points: usize,
    pub dimensions: &'a str,
    pub nodes: &'a str,
    pub charts: &'a str,
    pub labels: &'a str,
    pub group_by: &'a str,
    pub group: &'a str,
}

fn query(api: &Api, o: &QueryOpts) -> Result<Value> {
    api.get(
        "/api/v1/metrics/data",
        &[
            ("context", o.context.to_string()),
            ("after", o.after.to_string()),
            ("points", o.points.to_string()),
            ("dimensions", o.dimensions.to_string()),
            ("nodes", o.nodes.to_string()),
            ("charts", o.charts.to_string()),
            ("labels", o.labels.to_string()),
            ("group_by", o.group_by.to_string()),
            ("group", o.group.to_string()),
        ],
    )
}

fn values(d: &Value) -> Vec<Option<f64>> {
    d["values"]
        .as_array()
        .map(|a| a.iter().map(num).collect())
        .unwrap_or_default()
}

pub fn cmd_query(api: &Api, o: &QueryOpts, output: &str) -> Result<()> {
    let v = query(api, o)?;
    if output == "json" {
        return print_json(&v);
    }
    println!(
        "{} {} ({}, tier {}, {}s per point)",
        v["context"].as_str().unwrap_or(o.context).bold(),
        v["title"].as_str().unwrap_or("").dimmed(),
        v["units"].as_str().unwrap_or(""),
        v["tier"],
        v["interval"]
    );
    for d in v["dimensions"].as_array().cloned().unwrap_or_default() {
        let vals = values(&d);
        let last = vals.iter().rev().flatten().next().copied();
        let present: Vec<f64> = vals.iter().flatten().copied().collect();
        let avg = (!present.is_empty()).then(|| present.iter().sum::<f64>() / present.len() as f64);
        let max = present
            .iter()
            .copied()
            .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))));
        let tail = &vals[vals.len().saturating_sub(60)..];
        println!(
            "  {:28} last {:>9} avg {:>9} max {:>9}  {}",
            d["name"].as_str().unwrap_or("-"),
            fmt_num(last),
            fmt_num(avg),
            fmt_num(max),
            sparkline(tail).cyan()
        );
    }
    Ok(())
}

/// Latest CPU, memory, load and network per node.
pub fn cmd_top(api: &Api, output: &str) -> Result<()> {
    let q = |context: &'static str, dimensions: &'static str| QueryOpts {
        context,
        after: -30,
        points: 1,
        dimensions,
        nodes: "",
        charts: "",
        labels: "",
        group_by: "node",
        group: "avg",
    };
    let cpu = query(api, &q("system.cpu", ""))?;
    let mem = query(api, &q("mem.used_percent", ""))?;
    let load = query(api, &q("system.load", "load1"))?;
    let rx = query(api, &q("system.net", "received"))?;
    let tx = query(api, &q("system.net", "sent"))?;
    let pick = |v: &Value| -> std::collections::BTreeMap<String, Option<f64>> {
        v["dimensions"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|d| {
                (
                    d["name"].as_str().unwrap_or("").to_string(),
                    values(d).last().copied().flatten(),
                )
            })
            .collect()
    };
    let (cpu, mem, load, rx, tx) = (pick(&cpu), pick(&mem), pick(&load), pick(&rx), pick(&tx));
    let mut nodes: Vec<&String> = cpu.keys().chain(mem.keys()).collect();
    nodes.sort();
    nodes.dedup();
    if output == "json" {
        let rows: Vec<Value> = nodes
            .iter()
            .map(|n| serde_json::json!({ "node": n, "cpu": cpu.get(*n), "memory": mem.get(*n), "load1": load.get(*n), "netIn": rx.get(*n), "netOut": tx.get(*n) }))
            .collect();
        return print_json(&Value::Array(rows));
    }
    println!(
        "{:32} {:>7} {:>7} {:>7} {:>12} {:>12}",
        "NODE".bold(),
        "CPU%".bold(),
        "MEM%".bold(),
        "LOAD1".bold(),
        "NET IN".bold(),
        "NET OUT".bold()
    );
    for n in nodes {
        let g = |m: &std::collections::BTreeMap<String, Option<f64>>| m.get(n).copied().flatten();
        println!(
            "{:32} {:>7} {:>7} {:>7} {:>12} {:>12}",
            n,
            fmt_num(g(&cpu)),
            fmt_num(g(&mem)),
            fmt_num(g(&load)),
            fmt_num(g(&rx).map(f64::abs)) + "b",
            fmt_num(g(&tx).map(f64::abs)) + "b"
        );
    }
    Ok(())
}

pub fn cmd_anomalies(api: &Api, after: i64, top: usize, output: &str) -> Result<()> {
    let v = api.get(
        "/api/v1/metrics/anomalies",
        &[("after", after.to_string()), ("top", top.to_string())],
    )?;
    if output == "json" {
        return print_json(&v);
    }
    let s = &v["summary"];
    println!("{}", "Anomaly rate by node".bold());
    for n in s["nodes"].as_array().cloned().unwrap_or_default() {
        let tl: Vec<Option<f64>> = n["timeline"]
            .as_array()
            .map(|a| a.iter().map(|p| num(&p["rate"])).collect())
            .unwrap_or_default();
        println!(
            "  {:32} {:>6}% of {} dimensions  {}",
            n["node"].as_str().unwrap_or("-"),
            fmt_num(num(&n["anomalyRate"])),
            n["dimensions"],
            sparkline(&tl).yellow()
        );
    }
    println!();
    println!("{}", "Most anomalous dimensions".bold());
    for r in s["ranked"].as_array().cloned().unwrap_or_default() {
        println!(
            "  {:>6}%  {} {} {}/{}",
            fmt_num(num(&r["anomalyRate"])),
            r["node"].as_str().unwrap_or("-"),
            r["context"].as_str().unwrap_or("-").cyan(),
            r["chart"].as_str().unwrap_or("-"),
            r["dimension"].as_str().unwrap_or("-")
        );
    }
    Ok(())
}

pub fn cmd_alerts(api: &Api, all: bool, output: &str) -> Result<()> {
    let v = api.get(
        "/api/v1/metrics/alerts",
        &[("all", all.to_string()), ("history", "20".into())],
    )?;
    if output == "json" {
        return print_json(&v);
    }
    let st = &v["stats"];
    println!(
        "{} {} rules, {} instances, {} warning, {} critical",
        "Metric alerts".bold(),
        st["rules"],
        st["instances"],
        st["warning"],
        st["critical"]
    );
    let active = v["active"].as_array().cloned().unwrap_or_default();
    if active.is_empty() {
        println!("  {}", "nothing raised".green());
    }
    for a in active {
        let status = a["status"].as_str().unwrap_or("-");
        let tag = match status {
            "critical" => status.red().bold().to_string(),
            "warning" => status.yellow().to_string(),
            "clear" => status.green().to_string(),
            _ => status.dimmed().to_string(),
        };
        let subject = match a["dimension"].as_str() {
            Some(d) => format!("{}/{d}", a["chart"].as_str().unwrap_or("")),
            None => a["chart"].as_str().unwrap_or("").to_string(),
        };
        println!(
            "  {:9} {:28} {:24} {} {} {}",
            tag,
            a["rule"].as_str().unwrap_or("-"),
            a["node"].as_str().unwrap_or("-"),
            subject,
            fmt_num(num(&a["value"])),
            a["units"].as_str().unwrap_or("")
        );
    }
    let hist = v["history"].as_array().cloned().unwrap_or_default();
    if !hist.is_empty() {
        println!();
        println!("{}", "Recent transitions".bold());
        for h in hist {
            let t = chrono::DateTime::from_timestamp(h["time"].as_i64().unwrap_or(0), 0)
                .map(|d| d.format("%H:%M:%S").to_string())
                .unwrap_or_default();
            println!(
                "  {t} {:28} {} {} → {}",
                h["rule"].as_str().unwrap_or("-"),
                h["node"].as_str().unwrap_or("-"),
                h["from"].as_str().unwrap_or("-"),
                h["to"].as_str().unwrap_or("-")
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparkline_scales() {
        assert_eq!(sparkline(&[Some(0.0), Some(7.0), None, Some(3.5)]), "▁█ ▅");
        assert_eq!(sparkline(&[Some(1.0), Some(1.0)]), "▁▁");
        assert_eq!(sparkline(&[None]), "");
    }

    #[test]
    fn number_formatting() {
        assert_eq!(fmt_num(None), "-");
        assert_eq!(fmt_num(Some(12.345)), "12.35");
        assert_eq!(fmt_num(Some(1500.0)), "1500");
        assert_eq!(fmt_num(Some(2_500_000.0)), "2.50M");
    }
}
