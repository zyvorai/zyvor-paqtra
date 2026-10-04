//! Node agent: health, read-only BPF attachment / drift HTTP, and the
//! per-second metrics collectors streaming to the API.

use anyhow::{Context, Result};
use owo_colors::OwoColorize;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::signal;

use crate::ebpf::attachments::{collect_inventory, drift_findings};
use crate::metrics::{spawn_pod_watch, AgentMetrics, MetricsConfig};

/// Long-running node agent for the DaemonSet.
/// Serves `/health`, `/attachments`, `/drift` and `/metrics/status`
/// (observe-only).
pub async fn run_agent(listen: SocketAddr) -> Result<()> {
    let node = std::env::var("NODE_NAME").unwrap_or_else(|_| "unknown".into());
    let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "paqtra-agent".into());

    println!(
        "{} Paqtra agent starting on {} (node={}, pod={})",
        "→".cyan(),
        listen,
        node,
        hostname
    );

    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("bind {listen}"))?;

    let metrics = start_metrics();

    println!(
        "{} Agent ready — GET /health /attachments /drift /metrics/status",
        "✔".green()
    );

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                println!("{} Shutting down agent", "→".cyan());
                break;
            }
            accept = listener.accept() => {
                match accept {
                    Ok((mut sock, _)) => {
                        let metrics = metrics.clone();
                        tokio::spawn(async move {
                            let mut buf = [0u8; 2048];
                            let n = sock.read(&mut buf).await.unwrap_or(0);
                            let req = String::from_utf8_lossy(&buf[..n]);
                            let path = parse_path(&req);
                            let (status, body) = handle_path(path, metrics.as_deref());
                            let resp = format!(
                                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len(),
                            );
                            let _ = sock.write_all(resp.as_bytes()).await;
                        });
                    }
                    Err(e) => tracing::warn!("accept error: {e}"),
                }
            }
        }
    }
    if let Some(m) = &metrics {
        m.stop();
    }
    Ok(())
}

fn start_metrics() -> Option<Arc<AgentMetrics>> {
    let cfg = MetricsConfig::from_env();
    if !cfg.enabled {
        return None;
    }
    let pods = spawn_pod_watch(cfg.node.clone());
    let api = cfg.api.clone();
    match AgentMetrics::start(cfg, pods) {
        Ok(m) => {
            if api.is_empty() {
                println!(
                    "{} Metrics collecting locally (set PAQTRA_METRICS_API to stream)",
                    "→".cyan()
                );
            } else {
                println!("{} Metrics streaming to {api}", "→".cyan());
            }
            Some(Arc::new(m))
        }
        Err(e) => {
            tracing::warn!("metrics disabled: {e}");
            None
        }
    }
}

fn parse_path(req: &str) -> &str {
    let line = req.lines().next().unwrap_or("");
    let mut parts = line.split_whitespace();
    let _method = parts.next();
    let path = parts.next().unwrap_or("/");
    path.split('?').next().unwrap_or("/")
}

fn handle_path(path: &str, metrics: Option<&AgentMetrics>) -> (&'static str, String) {
    match path {
        "/health" | "/" => (
            "200 OK",
            r#"{"status":"ok","component":"agent"}"#.to_string(),
        ),
        "/attachments" => {
            let inv = collect_inventory();
            match serde_json::to_string(&inv) {
                Ok(s) => ("200 OK", s),
                Err(_) => (
                    "500 Internal Server Error",
                    r#"{"error":"serialize"}"#.into(),
                ),
            }
        }
        "/drift" => {
            let inv = collect_inventory();
            let findings = drift_findings(&inv);
            match serde_json::to_string(&findings) {
                Ok(s) => ("200 OK", s),
                Err(_) => (
                    "500 Internal Server Error",
                    r#"{"error":"serialize"}"#.into(),
                ),
            }
        }
        "/metrics/status" => match metrics {
            Some(m) => ("200 OK", m.status_json().to_string()),
            None => (
                "200 OK",
                r#"{"enabled":false,"hint":"PAQTRA_METRICS=false or startup failed"}"#.into(),
            ),
        },
        _ => ("404 Not Found", r#"{"error":"not found"}"#.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::time::timeout;

    async fn get(addr: SocketAddr, path: &str) -> String {
        let mut sock = timeout(Duration::from_secs(2), TcpStream::connect(addr))
            .await
            .expect("connect timeout")
            .expect("connect");
        let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        sock.write_all(req.as_bytes()).await.unwrap();
        let mut buf = vec![0u8; 65536];
        let n = timeout(Duration::from_secs(2), sock.read(&mut buf))
            .await
            .expect("read timeout")
            .unwrap();
        String::from_utf8_lossy(&buf[..n]).to_string()
    }

    #[tokio::test]
    async fn agent_health_and_routes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let handle = tokio::spawn(async move {
            let _ = run_agent(addr).await;
        });

        tokio::time::sleep(Duration::from_millis(150)).await;

        let health = get(addr, "/health").await;
        assert!(health.contains("200"), "{health}");
        assert!(health.contains("agent"), "{health}");

        let att = get(addr, "/attachments").await;
        assert!(att.contains("200"), "{att}");
        assert!(
            att.contains("attachments") || att.contains("source"),
            "{att}"
        );

        let drift = get(addr, "/drift").await;
        assert!(drift.contains("200"), "{drift}");
        assert!(drift.contains("kind") || drift.contains("[]"), "{drift}");

        let status = get(addr, "/metrics/status").await;
        assert!(status.contains("200"), "{status}");

        let missing = get(addr, "/nope").await;
        assert!(missing.contains("404"), "{missing}");

        handle.abort();
    }

    #[test]
    fn parse_path_basic() {
        assert_eq!(
            parse_path("GET /attachments HTTP/1.1\r\nHost: x\r\n\r\n"),
            "/attachments"
        );
        assert_eq!(parse_path("GET /drift?x=1 HTTP/1.1\r\n\r\n"), "/drift");
    }
}
