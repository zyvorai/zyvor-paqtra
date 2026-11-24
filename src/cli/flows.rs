// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! `paqtra flows` — read-only views derived from stored flow history through
//! the API: service edges with golden signals, the Hubble metric cardinality
//! advisor, SLO burn rate, window-to-window diff and source fan-out. Nothing here changes the cluster.

use super::metrics::Api;
use anyhow::Result;
use owo_colors::OwoColorize;
use serde_json::Value;

fn print_json(v: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

/// Window and scope shared by the flow-history commands.
pub struct Window<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub namespace: &'a str,
}

impl Window<'_> {
    fn query(&self) -> Vec<(&'static str, String)> {
        vec![
            ("from", self.from.to_string()),
            ("to", self.to.to_string()),
            ("namespace", self.namespace.to_string()),
        ]
    }
}

fn pct(v: &Value) -> String {
    match v.as_f64() {
        Some(r) => format!("{:.2}%", r * 100.0),
        None => "-".into(),
    }
}

pub fn cmd_edges(
    api: &Api,
    w: &Window,
    granularity: &str,
    max_edges: usize,
    output: &str,
) -> Result<()> {
    let mut q = w.query();
    q.push(("granularity", granularity.to_string()));
    q.push(("max_edges", max_edges.to_string()));
    let v = api.get("/api/v1/flows/history/edges", &q)?;
    if output == "json" {
        return print_json(&v);
    }
    println!(
        "{} {} flows, {} edges listed, {} folded{}",
        "Service edges".bold(),
        v["total_flows"],
        v["edges"].as_array().map_or(0, Vec::len),
        v["folded"]["edges"],
        if v["truncated"] == true {
            " (store had more groups than were read: counts are lower bounds)"
        } else {
            ""
        }
    );
    println!(
        "  {:34} {:34} {:6} {:>9} {:>8}",
        "SOURCE", "DESTINATION", "PROTO", "FLOWS", "DROPPED"
    );
    for e in v["edges"].as_array().cloned().unwrap_or_default() {
        let dropped = pct(&e["drop_ratio"]);
        let dropped = if e["dropped"].as_u64().unwrap_or(0) > 0 {
            dropped.red().to_string()
        } else {
            dropped
        };
        println!(
            "  {:34} {:34} {:6} {:>9} {:>8}",
            e["source"].as_str().unwrap_or("-"),
            e["destination"].as_str().unwrap_or("-"),
            e["protocol"].as_str().unwrap_or("-"),
            e["total"],
            dropped
        );
        if let Some(r) = e["drop_reasons"].as_array().and_then(|r| r.first()) {
            println!(
                "      top drop reason: {} ({})",
                r["reason"].as_str().unwrap_or("-"),
                r["dropped"]
            );
        }
    }
    if let Some(n) = v["not_measured"].as_str() {
        println!("\n  {}", n.dimmed());
    }
    Ok(())
}

pub fn cmd_cardinality(api: &Api, w: &Window, budget: u64, output: &str) -> Result<()> {
    let mut q = w.query();
    q.push(("budget", budget.to_string()));
    let v = api.get("/api/v1/flows/history/cardinality", &q)?;
    if output == "json" {
        return print_json(&v);
    }
    println!(
        "{} budget {} series, {} pods / {} workloads / {} namespaces seen",
        "Hubble metric cardinality".bold(),
        v["budget"],
        v["observed"]["pods"],
        v["observed"]["workloads"],
        v["observed"]["namespaces"]
    );
    println!(
        "  {:10} {:>10} {:>10} {:>10}  RISK",
        "CONTEXT", "FLOW", "DROP", "TOTAL"
    );
    for c in v["contexts"].as_array().cloned().unwrap_or_default() {
        let risk = c["risk"].as_str().unwrap_or("-");
        let tag = match risk {
            "extreme" | "high" => risk.red().to_string(),
            "ok" => risk.yellow().to_string(),
            _ => risk.green().to_string(),
        };
        println!(
            "  {:10} {:>10} {:>10} {:>10}  {}",
            c["context"].as_str().unwrap_or("-"),
            c["flow_series"],
            c["drop_series"],
            c["total_series"],
            tag
        );
    }
    match v["recommended"].as_str() {
        Some(r) => println!(
            "\n  most detailed context within budget: {} (labelsContext={})",
            r.bold(),
            v["recommended_labels_context"].as_str().unwrap_or("")
        ),
        None => println!("\n  {}", "no labelsContext fits this budget".yellow()),
    }
    for n in v["notes"].as_array().cloned().unwrap_or_default() {
        println!("  - {}", n.as_str().unwrap_or("").dimmed());
    }
    Ok(())
}

pub fn cmd_burn_rate(api: &Api, id: &str, output: &str) -> Result<()> {
    let v = api.get(&format!("/api/v1/slo/targets/{id}/burn-rate"), &[])?;
    if output == "json" {
        return print_json(&v);
    }
    let b = &v["burn_rate"];
    let status = b["status"].as_str().unwrap_or("-");
    let tag = match status {
        "page" => status.red().bold().to_string(),
        "ticket" => status.yellow().to_string(),
        "ok" => status.green().to_string(),
        _ => status.dimmed().to_string(),
    };
    println!(
        "{} {} ({}% over {}): {}",
        "SLO burn rate".bold(),
        v["name"].as_str().unwrap_or("-"),
        b["target"],
        v["window"].as_str().unwrap_or("-"),
        tag
    );
    println!(
        "  {:7} {:>8} {:>8} {:>9} {:>9} {:>8}  STATE",
        "TIER", "LONG", "SHORT", "LONG BURN", "SHORT", "THRESH"
    );
    for t in b["tiers"].as_array().cloned().unwrap_or_default() {
        let secs = |x: &Value| {
            let s = x.as_i64().unwrap_or(0);
            if s >= 86_400 {
                format!("{}d", s / 86_400)
            } else if s >= 3_600 {
                format!("{}h", s / 3_600)
            } else {
                format!("{}m", s / 60)
            }
        };
        let burn = |x: &Value| x.as_f64().map_or("-".to_string(), |n| format!("{n:.1}x"));
        println!(
            "  {:7} {:>8} {:>8} {:>9} {:>9} {:>7}x  {}{}",
            t["severity"].as_str().unwrap_or("-"),
            secs(&t["long"]["seconds"]),
            secs(&t["short"]["seconds"]),
            burn(&t["long"]["burn_rate"]),
            burn(&t["short"]["burn_rate"]),
            t["burn_threshold"],
            t["status"].as_str().unwrap_or("-"),
            if t["firing"] == true {
                " FIRING".red().to_string()
            } else {
                String::new()
            }
        );
    }
    if let Some(m) = b["measurement"].as_str() {
        println!("\n  {}", m.dimmed());
    }
    Ok(())
}

pub struct DiffArgs<'a> {
    pub baseline_from: &'a str,
    pub baseline_to: &'a str,
    pub granularity: &'a str,
    pub max_items: usize,
}

fn edge_name(e: &Value) -> String {
    format!(
        "{} -> {} ({})",
        e["source"].as_str().unwrap_or("-"),
        e["destination"].as_str().unwrap_or("-"),
        e["protocol"].as_str().unwrap_or("-")
    )
}

pub fn cmd_diff(api: &Api, w: &Window, d: &DiffArgs, output: &str) -> Result<()> {
    let mut q = w.query();
    q.push(("baseline_from", d.baseline_from.to_string()));
    q.push(("baseline_to", d.baseline_to.to_string()));
    q.push(("granularity", d.granularity.to_string()));
    q.push(("max_items", d.max_items.to_string()));
    let v = api.get("/api/v1/flows/history/diff", &q)?;
    if output == "json" {
        return print_json(&v);
    }
    let c = &v["counts"];
    println!(
        "{} {} new, {} gone, {} drop regressions, {} rate changes ({} vs {} edges)",
        "Flow diff".bold(),
        c["new_edges"],
        c["gone_edges"],
        c["drop_regressions"],
        c["rate_changes"],
        c["current_edges"],
        c["baseline_edges"]
    );
    println!(
        "  baseline {} to {}",
        v["baseline"]["from"].as_str().unwrap_or("-"),
        v["baseline"]["to"].as_str().unwrap_or("-")
    );
    let list = |title: &str, key: &str, line: &dyn Fn(&Value) -> String| {
        let items = v[key].as_array().cloned().unwrap_or_default();
        if items.is_empty() {
            return;
        }
        println!("\n  {}", title.bold());
        for i in items {
            println!("    {}", line(&i));
        }
    };
    list("New edges", "new_edges", &|e| {
        format!(
            "{}  {} flows, {} dropped",
            edge_name(e),
            e["flows"],
            e["dropped"]
        )
    });
    list("Gone edges", "gone_edges", &|e| {
        format!("{}  {} flows before", edge_name(e), e["flows"])
    });
    list("Drop regressions", "drop_regressions", &|e| {
        format!(
            "{}  {} -> {}",
            edge_name(e),
            pct(&e["baseline_drop_ratio"]),
            pct(&e["current_drop_ratio"]).red()
        )
    });
    list("Traffic rate changes", "rate_changes", &|e| {
        format!(
            "{}  {}/s -> {}/s ({}x)",
            edge_name(e),
            e["baseline_flows_per_sec"],
            e["current_flows_per_sec"],
            e["factor"]
        )
    });
    if c["omitted"].as_u64().unwrap_or(0) > 0 {
        println!("\n  {} more not listed (raise --max-items)", c["omitted"]);
    }
    for n in v["warnings"].as_array().cloned().unwrap_or_default() {
        println!("  ! {}", n.as_str().unwrap_or("").yellow());
    }
    Ok(())
}

pub struct FanoutArgs<'a> {
    pub granularity: &'a str,
    pub min_targets: usize,
    pub min_drop_ratio: f64,
    pub limit: usize,
}

pub fn cmd_fanout(api: &Api, w: &Window, f: &FanoutArgs, output: &str) -> Result<()> {
    let mut q = w.query();
    q.push(("granularity", f.granularity.to_string()));
    q.push(("min_targets", f.min_targets.to_string()));
    q.push(("min_drop_ratio", f.min_drop_ratio.to_string()));
    q.push(("limit", f.limit.to_string()));
    let v = api.get("/api/v1/flows/history/fanout", &q)?;
    if output == "json" {
        return print_json(&v);
    }
    println!(
        "{} {} of {} sources reach {}+ targets, {} suspect",
        "Fan-out".bold(),
        v["sources_over_threshold"],
        v["sources_seen"],
        v["min_targets"],
        v["suspects"]
    );
    println!(
        "  {:36} {:>6} {:>8} {:>8} {:>8}",
        "SOURCE", "DESTS", "PORTS/1", "FLOWS", "DROPPED"
    );
    for s in v["sources"].as_array().cloned().unwrap_or_default() {
        let name = s["source"].as_str().unwrap_or("-").to_string();
        let name = if s["suspect"] == true {
            format!("{name} (suspect)").red().to_string()
        } else {
            name
        };
        println!(
            "  {:36} {:>6} {:>8} {:>8} {:>8}",
            name,
            s["destinations"],
            s["max_ports_on_one_destination"],
            s["flows"],
            pct(&s["drop_ratio"])
        );
    }
    if let Some(n) = v["caveat"].as_str() {
        println!("\n  {}", n.dimmed());
    }
    Ok(())
}

pub fn cmd_drops(
    api: &Api,
    w: &Window,
    granularity: &str,
    max_reasons: usize,
    output: &str,
) -> Result<()> {
    let mut q = w.query();
    q.push(("granularity", granularity.to_string()));
    q.push(("max_reasons", max_reasons.to_string()));
    let v = api.get("/api/v1/flows/history/drops", &q)?;
    if output == "json" {
        return print_json(&v);
    }
    println!(
        "{} {} dropped of {} flows ({})",
        "Drops".bold(),
        v["total_dropped"],
        v["total_flows"],
        pct(&v["drop_ratio"])
    );
    for c in v["classes"].as_array().cloned().unwrap_or_default() {
        println!(
            "  {:20} {:>8} {:>8}",
            c["class"].as_str().unwrap_or("-"),
            c["dropped"],
            pct(&c["share"])
        );
    }
    println!(
        "\n  {:44} {:18} {:>8} {:>6}",
        "REASON", "CLASS", "DROPPED", "EDGES"
    );
    for r in v["reasons"].as_array().cloned().unwrap_or_default() {
        println!(
            "  {:44} {:18} {:>8} {:>6}",
            r["reason"].as_str().unwrap_or("-"),
            r["class"].as_str().unwrap_or("-"),
            r["dropped"],
            r["edges"]
        );
        for e in r["top_edges"].as_array().cloned().unwrap_or_default() {
            println!(
                "      {} -> {} {}/{} ({})",
                e["source"].as_str().unwrap_or("-"),
                e["destination"].as_str().unwrap_or("-"),
                e["protocol"].as_str().unwrap_or("-"),
                e["port"],
                e["dropped"]
            );
        }
    }
    if v["omitted_reasons"].as_u64().unwrap_or(0) > 0 {
        println!(
            "\n  {} more reasons not listed (raise --max-reasons)",
            v["omitted_reasons"]
        );
    }
    if let Some(n) = v["caveat"].as_str() {
        println!("\n  {}", n.dimmed());
    }
    Ok(())
}

pub fn cmd_top(api: &Api, w: &Window, granularity: &str, limit: usize, output: &str) -> Result<()> {
    let mut q = w.query();
    q.push(("granularity", granularity.to_string()));
    q.push(("limit", limit.to_string()));
    let v = api.get("/api/v1/flows/history/top", &q)?;
    if output == "json" {
        return print_json(&v);
    }
    println!("{} {} flows", "Top talkers".bold(), v["total_flows"]);
    for (title, key) in [
        ("SOURCES", "sources"),
        ("DESTINATIONS", "destinations"),
        ("PORTS", "ports"),
    ] {
        println!(
            "\n  {:36} {:>9} {:>7} {:>8}",
            title, "FLOWS", "SHARE", "DROPPED"
        );
        for e in v[key]["entries"].as_array().cloned().unwrap_or_default() {
            println!(
                "  {:36} {:>9} {:>7} {:>8}",
                e["name"].as_str().unwrap_or("-"),
                e["flows"],
                pct(&e["share"]),
                pct(&e["drop_ratio"])
            );
        }
    }
    if let Some(n) = v["note"].as_str() {
        println!("\n  {}", n.dimmed());
    }
    Ok(())
}

pub fn cmd_audit(
    api: &Api,
    w: &Window,
    granularity: &str,
    max_items: usize,
    output: &str,
) -> Result<()> {
    let mut q = w.query();
    q.push(("granularity", granularity.to_string()));
    q.push(("max_items", max_items.to_string()));
    let v = api.get("/api/v1/flows/history/audit", &q)?;
    if output == "json" {
        return print_json(&v);
    }
    println!(
        "{} {} would-be-denied flows on {} edges",
        "Policy audit".bold(),
        v["audit_flows"],
        v["edges_total"]
    );
    println!(
        "  {:34} {:34} {:>8}  PORTS",
        "SOURCE", "DESTINATION", "FLOWS"
    );
    for e in v["edges"].as_array().cloned().unwrap_or_default() {
        let ports: Vec<String> = e["ports"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|p| p.to_string())
            .collect();
        println!(
            "  {:34} {:34} {:>8}  {}/{}",
            e["source"].as_str().unwrap_or("-"),
            e["destination"].as_str().unwrap_or("-"),
            e["flows"],
            e["protocol"].as_str().unwrap_or("-"),
            ports.join(",")
        );
    }
    if let Some(n) = v["guidance"].as_str() {
        println!("\n  {}", n.dimmed());
    }
    Ok(())
}

pub struct SpikeArgs {
    pub bucket_secs: Option<i64>,
    pub sensitivity: f64,
    pub min_drops: u64,
}

pub fn cmd_spikes(api: &Api, w: &Window, s: &SpikeArgs, output: &str) -> Result<()> {
    let mut q = w.query();
    if let Some(b) = s.bucket_secs {
        q.push(("bucket_secs", b.to_string()));
    }
    q.push(("sensitivity", s.sensitivity.to_string()));
    q.push(("min_drops", s.min_drops.to_string()));
    let v = api.get("/api/v1/flows/history/spikes", &q)?;
    if output == "json" {
        return print_json(&v);
    }
    println!(
        "{} {} ({} buckets of {}s, typical {} drops, threshold {})",
        "Drop spikes".bold(),
        v["status"].as_str().unwrap_or("-"),
        v["buckets"],
        v["bucket_secs"],
        v["typical_dropped_per_bucket"],
        v["threshold_dropped_per_bucket"]
    );
    for e in v["episodes"].as_array().cloned().unwrap_or_default() {
        println!(
            "  {} .. {}  {} drops over {} bucket(s), peak {}",
            e["start"], e["end"], e["dropped"], e["buckets"], e["peak_dropped"]
        );
    }
    if let Some(n) = v["note"].as_str() {
        println!("\n  {}", n.dimmed());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{Read, Write};

    /// One canned HTTP reply on loopback; returns the base URL and the request line.
    fn stub(body: &'static str) -> (String, std::sync::mpsc::Receiver<String>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut buf = [0u8; 8192];
            let n = c.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let _ = tx.send(req.lines().next().unwrap_or("").to_string());
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            c.write_all(resp.as_bytes()).unwrap();
        });
        (base, rx)
    }

    #[test]
    fn ratios_format_as_percent_and_missing_is_a_dash() {
        assert_eq!(pct(&json!(0.1018)), "10.18%");
        assert_eq!(pct(&json!(0)), "0.00%");
        assert_eq!(pct(&Value::Null), "-");
        assert_eq!(pct(&json!("x")), "-");
    }

    #[test]
    fn edges_sends_window_granularity_and_cap_and_tolerates_sparse_replies() {
        let (url, rx) =
            stub(r#"{"edges":[{"source":"a/x"}],"folded":{"edges":0},"total_flows":1}"#);
        let api = Api { url, token: None };
        let w = Window {
            from: "2026-11-01T00:00:00Z",
            to: "",
            namespace: "prod",
        };
        cmd_edges(&api, &w, "pod", 7, "summary").unwrap();
        let line = rx.recv().unwrap();
        assert!(
            line.starts_with("GET /api/v1/flows/history/edges?"),
            "{line}"
        );
        for want in [
            "from=2026-11-01",
            "namespace=prod",
            "granularity=pod",
            "max_edges=7",
        ] {
            assert!(line.contains(want), "{line}");
        }
        // An empty `to` is not sent.
        assert!(!line.contains("to="), "{line}");
    }

    #[test]
    fn burn_rate_reads_the_slo_path_and_renders_a_minimal_reply() {
        let (url, rx) = stub(
            r#"{"name":"n","window":"30d","burn_rate":{"status":"ok","target":99.9,"tiers":[]}}"#,
        );
        let api = Api {
            url,
            token: Some("t".into()),
        };
        cmd_burn_rate(&api, "slo-1", "summary").unwrap();
        assert!(rx
            .recv()
            .unwrap()
            .starts_with("GET /api/v1/slo/targets/slo-1/burn-rate "));
    }

    #[test]
    fn diff_sends_baseline_and_renders_every_list() {
        let (url, rx) = stub(
            r#"{"counts":{"new_edges":1,"gone_edges":1,"drop_regressions":1,"rate_changes":1,"omitted":2},
"baseline":{"from":"a","to":"b"},
"new_edges":[{"source":"p/a","destination":"p/b","protocol":"TCP","flows":5,"dropped":1}],
"gone_edges":[{"source":"p/c","destination":"p/d","protocol":"UDP","flows":9}],
"drop_regressions":[{"source":"p/e","destination":"p/f","protocol":"TCP","baseline_drop_ratio":null,"current_drop_ratio":0.2}],
"rate_changes":[{"source":"p/g","destination":"p/h","protocol":"TCP","baseline_flows_per_sec":1.0,"current_flows_per_sec":4.0,"factor":4.0}],
"warnings":["w"]}"#,
        );
        let api = Api { url, token: None };
        let w = Window {
            from: "",
            to: "",
            namespace: "",
        };
        let d = DiffArgs {
            baseline_from: "2026-11-01T00:00:00Z",
            baseline_to: "2026-11-01T01:00:00Z",
            granularity: "pod",
            max_items: 9,
        };
        cmd_diff(&api, &w, &d, "summary").unwrap();
        let line = rx.recv().unwrap();
        assert!(
            line.starts_with("GET /api/v1/flows/history/diff?"),
            "{line}"
        );
        for want in [
            "baseline_from=2026-11-01",
            "baseline_to=2026-11-01",
            "granularity=pod",
            "max_items=9",
        ] {
            assert!(line.contains(want), "{line}");
        }
    }

    #[test]
    fn fanout_sends_thresholds_and_tolerates_an_empty_reply() {
        let (url, rx) = stub(r#"{"sources":[]}"#);
        let api = Api { url, token: None };
        let w = Window {
            from: "",
            to: "",
            namespace: "prod",
        };
        let f = FanoutArgs {
            granularity: "pod",
            min_targets: 12,
            min_drop_ratio: 0.5,
            limit: 3,
        };
        cmd_fanout(&api, &w, &f, "summary").unwrap();
        let line = rx.recv().unwrap();
        assert!(
            line.starts_with("GET /api/v1/flows/history/fanout?"),
            "{line}"
        );
        for want in [
            "namespace=prod",
            "min_targets=12",
            "min_drop_ratio=0.5",
            "limit=3",
        ] {
            assert!(line.contains(want), "{line}");
        }
    }

    #[test]
    fn drops_top_audit_and_spikes_send_their_parameters_and_render() {
        let w = Window {
            from: "",
            to: "",
            namespace: "prod",
        };
        let (url, rx) = stub(
            r#"{"total_dropped":3,"total_flows":9,"drop_ratio":0.5,"classes":[{"class":"policy","dropped":3,"share":1.0}],
"reasons":[{"reason":"Policy denied","class":"policy","dropped":3,"edges":1,"top_edges":[{"source":"a","destination":"b","protocol":"TCP","port":80,"dropped":3}]}],"omitted_reasons":2}"#,
        );
        cmd_drops(&Api { url, token: None }, &w, "pod", 7, "summary").unwrap();
        let line = rx.recv().unwrap();
        assert!(
            line.starts_with("GET /api/v1/flows/history/drops?"),
            "{line}"
        );
        for want in ["namespace=prod", "granularity=pod", "max_reasons=7"] {
            assert!(line.contains(want), "{line}");
        }

        let (url, rx) = stub(
            r#"{"total_flows":1,"sources":{"entries":[{"name":"a","flows":1,"share":1.0,"drop_ratio":null}]},"destinations":{"entries":[]},"ports":{"entries":[]}}"#,
        );
        cmd_top(&Api { url, token: None }, &w, "namespace", 4, "summary").unwrap();
        let line = rx.recv().unwrap();
        assert!(line.starts_with("GET /api/v1/flows/history/top?"), "{line}");
        assert!(line.contains("limit=4") && line.contains("granularity=namespace"));

        let (url, rx) = stub(
            r#"{"audit_flows":2,"edges_total":1,"edges":[{"source":"a","destination":"b","protocol":"TCP","flows":2,"ports":[80,81]}]}"#,
        );
        cmd_audit(&Api { url, token: None }, &w, "workload", 5, "summary").unwrap();
        let line = rx.recv().unwrap();
        assert!(
            line.starts_with("GET /api/v1/flows/history/audit?"),
            "{line}"
        );
        assert!(line.contains("max_items=5"));

        let (url, rx) = stub(
            r#"{"status":"spikes","buckets":60,"bucket_secs":60,"episodes":[{"start":1,"end":2,"dropped":9,"buckets":1,"peak_dropped":9}]}"#,
        );
        let s = SpikeArgs {
            bucket_secs: Some(300),
            sensitivity: 4.5,
            min_drops: 12,
        };
        cmd_spikes(&Api { url, token: None }, &w, &s, "summary").unwrap();
        let line = rx.recv().unwrap();
        assert!(
            line.starts_with("GET /api/v1/flows/history/spikes?"),
            "{line}"
        );
        for want in ["bucket_secs=300", "sensitivity=4.5", "min_drops=12"] {
            assert!(line.contains(want), "{line}");
        }
    }
}
