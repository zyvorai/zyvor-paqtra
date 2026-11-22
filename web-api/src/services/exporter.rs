// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Background flow export pipeline
//
// Periodically reads active export configs from the in-memory cache and exports Hubble flows
// in the configured format (JSON, CSV, CEF) to the configured destination
// (local file, S3, syslog, or default path).

use crate::models::flow::Flow;
use crate::AppState;
use std::sync::Arc;
use tokio::time::{interval, Duration};

const EXPORT_CONFIGS_PREFIX: &str = "cv:export_configs:";
const DEFAULT_EXPORT_DIR: &str = "/var/lib/paqtra/exports";

/// Spawn the background export pipeline as a detached tokio task.
pub fn spawn_export_pipeline(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            if let Err(e) = run_exports(&state).await {
                tracing::warn!("Export pipeline cycle failed: {}", e);
            }
        }
    });
}

/// Run a single export cycle: read configs, fetch flows, export.
async fn run_exports(state: &AppState) -> anyhow::Result<()> {
    let configs = state.cache.list_values(EXPORT_CONFIGS_PREFIX).await?;
    if configs.is_empty() {
        return Ok(());
    }

    // Fetch flows once for all configs
    let flows = state.hubble.get_flows(1000, None).await?;
    if flows.is_empty() {
        tracing::debug!("Export pipeline: no flows to export");
        return Ok(());
    }

    for config in &configs {
        let status = config.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if status != "active" {
            continue;
        }

        let config_id = config
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let format = config
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("json");
        let destination = config
            .get("destination")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let payload = match format {
            "csv" => format_flows_csv(&flows),
            "cef" => format_flows_cef(&flows),
            _ => format_flows_json(&flows),
        };

        let ext = match format {
            "csv" => "csv",
            "cef" => "cef",
            _ => "json",
        };

        if let Err(e) = write_export(destination, config_id, ext, &payload).await {
            tracing::warn!(
                "Export to '{}' failed for config {}: {}",
                destination,
                config_id,
                e
            );
            continue;
        }

        // Update the config in cache with export metadata
        let mut updated = config.clone();
        let prev_count = config
            .get("exported_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        updated["exported_count"] = serde_json::json!(prev_count + flows.len() as u64);
        updated["last_export"] = serde_json::json!(chrono::Utc::now().to_rfc3339());

        let key = format!("{}{}", EXPORT_CONFIGS_PREFIX, config_id);
        if let Err(e) = state.cache.set_persistent(&key, &updated).await {
            tracing::warn!("Failed to update export metadata for {}: {}", config_id, e);
        }

        tracing::info!(
            "Exported {} flows for config {} (format={}, dest={})",
            flows.len(),
            config_id,
            format,
            if destination.is_empty() {
                "default"
            } else {
                destination
            }
        );
    }

    Ok(())
}

/// Write export payload to the appropriate destination.
async fn write_export(
    destination: &str,
    config_id: &str,
    ext: &str,
    payload: &str,
) -> anyhow::Result<()> {
    use crate::services::k8s::K8sService;

    if let Some(path) = destination.strip_prefix("file://") {
        // Append to a local file
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        file.write_all(payload.as_bytes()).await?;
        file.write_all(b"\n").await?;
    } else if destination.starts_with("s3://") {
        // Pipe to aws s3 cp
        let (ok, _stdout, stderr) =
            K8sService::run_cmd_stdin("aws", &["s3", "cp", "-", destination], payload).await;
        if !ok {
            anyhow::bail!("aws s3 cp failed: {}", stderr);
        }
    } else if let Some(addr) = destination.strip_prefix("syslog://") {
        // Send lines via UDP
        let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
        for line in payload.lines() {
            if !line.is_empty() {
                let _ = socket.send_to(line.as_bytes(), addr).await;
            }
        }
    } else {
        // Default: write to /var/lib/paqtra/exports/{config_id}.{ext}
        let dir = std::path::Path::new(DEFAULT_EXPORT_DIR);
        tokio::fs::create_dir_all(dir).await?;
        let path = dir.join(format!("{}.{}", config_id, ext));
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        file.write_all(payload.as_bytes()).await?;
        file.write_all(b"\n").await?;
    }

    Ok(())
}

/// Serialize flows as a JSON array.
fn format_flows_json(flows: &[Flow]) -> String {
    serde_json::to_string(flows).unwrap_or_else(|_| "[]".to_string())
}

/// Serialize flows as CSV with header.
fn format_flows_csv(flows: &[Flow]) -> String {
    let mut out = String::from("timestamp,src_ns,src_pod,dst_ns,dst_pod,verdict,protocol,port\n");
    for f in flows {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            csv_escape(&f.timestamp),
            csv_escape(&f.source.namespace),
            csv_escape(&f.source.pod),
            csv_escape(&f.destination.namespace),
            csv_escape(&f.destination.pod),
            csv_escape(&f.verdict),
            csv_escape(&f.protocol),
            f.port,
        ));
    }
    out
}

/// Escape a field value for CSV (quote if it contains comma, quote, or newline).
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Serialize flows in Common Event Format (CEF) for SIEM integration.
/// Format: CEF:0|Paqtra|FlowExport|1.0|flow|Flow Event|1|...
fn format_flows_cef(flows: &[Flow]) -> String {
    let mut out = String::new();
    for f in flows {
        let severity = if f.verdict == "DROPPED" { "7" } else { "1" };
        let line = format!(
            "CEF:0|Paqtra|FlowExport|1.0|flow|Flow Event|{}|\
             rt={} src={} spt=0 dst={} dpt={} proto={} \
             cs1={} cs1Label=SrcNamespace cs2={} cs2Label=SrcPod \
             cs3={} cs3Label=DstNamespace cs4={} cs4Label=DstPod \
             outcome={}\n",
            severity,
            f.timestamp,
            f.source.ip,
            f.destination.ip,
            f.port,
            f.protocol,
            f.source.namespace,
            f.source.pod,
            f.destination.namespace,
            f.destination.pod,
            f.verdict,
        );
        out.push_str(&line);
    }
    out
}
