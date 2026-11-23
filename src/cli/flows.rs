// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! `paqtra flows` — read-only views derived from stored flow history through
//! the API: service edges with golden signals, the Hubble metric cardinality
//! advisor and SLO burn rate. Nothing here changes the cluster.

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
}
