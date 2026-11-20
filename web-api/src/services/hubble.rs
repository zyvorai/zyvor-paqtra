// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Hubble service client
//
// Flows are read from the Hubble Observer gRPC API (see `hubble_grpc`). The
// `hubble` CLI is only a fallback, used when the gRPC call fails and the binary
// is on PATH, so the API works in an image that does not ship the CLI.

pub use crate::config::HubbleMode;
use crate::models::flow::{Flow, FlowEndpoint, FlowStats};
use crate::services::flow_store::FlowSource;
use crate::services::hubble_grpc;
use anyhow::Result;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// What a live flow stream yields.
#[derive(Debug)]
pub enum LiveEvent {
    Flow(Box<Flow>),
    /// The stream ended or failed; no more events follow.
    Ended(String),
}

/// True when an executable called `name` is in a directory on PATH.
fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|d| d.join(name).is_file()))
        .unwrap_or(false)
}

pub struct HubbleService {
    address: String,
    clusters: Vec<(String, String)>, // (cluster_name, address)
    mode: HubbleMode,
}

impl HubbleService {
    /// Create a new HubbleService.
    ///
    /// `clusters` is a list of `(cluster_name, host:port)` pairs for
    /// multi-cluster flow aggregation. Each address is validated at startup.
    ///
    /// # Errors
    /// Returns an error if `address` or any cluster address is not in `host:port` format,
    /// or if any cluster name is empty.
    pub fn new(address: &str, clusters: Vec<(String, String)>) -> Result<Self> {
        Self::validate_address(address)?;
        for (name, addr) in &clusters {
            if name.is_empty() {
                anyhow::bail!("Cluster name must not be empty");
            }
            Self::validate_address(addr)?;
        }
        Ok(Self {
            address: address.to_string(),
            clusters,
            mode: HubbleMode::default(),
        })
    }

    pub fn with_mode(mut self, mode: HubbleMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn mode(&self) -> HubbleMode {
        self.mode
    }

    fn validate_address(address: &str) -> Result<()> {
        let parts: Vec<&str> = address.rsplitn(2, ':').collect();
        if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
            anyhow::bail!(
                "Invalid Hubble address '{}': expected host:port format (e.g. hubble-relay:4245)",
                address
            );
        }
        if parts[0].parse::<u16>().is_err() {
            anyhow::bail!(
                "Invalid Hubble address '{}': port must be a valid u16",
                address
            );
        }
        Ok(())
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    /// Return the configured cluster list.
    pub fn clusters(&self) -> &[(String, String)] {
        &self.clusters
    }

    /// Whether Hubble at the primary address is usable.
    pub async fn is_healthy(&self) -> bool {
        self.is_address_healthy(&self.address).await
    }

    /// Whether Hubble at `address` is usable. Over gRPC this means the Observer
    /// answers `ServerStatus`; a listening port alone does not count. Only the
    /// CLI mode falls back to a plain TCP connect, since that is all the CLI needs.
    pub async fn is_address_healthy(&self, address: &str) -> bool {
        let tcp = || async { tokio::net::TcpStream::connect(address).await.is_ok() };
        match self.mode {
            HubbleMode::Cli => tcp().await,
            HubbleMode::Grpc => hubble_grpc::is_healthy(address).await,
            HubbleMode::Auto => {
                hubble_grpc::is_healthy(address).await || (on_path("hubble") && tcp().await)
            }
        }
    }

    /// Get flows from all configured clusters, tagged with cluster_name.
    /// Each cluster is queried concurrently. Returns a Vec of (cluster_name, flows).
    pub async fn get_flows_multi_cluster(
        &self,
        limit: usize,
        namespace: Option<&str>,
    ) -> Vec<(String, Vec<Flow>)> {
        let mut handles = Vec::new();

        for (name, addr) in &self.clusters {
            let name = name.clone();
            let addr = addr.clone();
            let namespace = namespace.map(|s| s.to_string());
            let mode = self.mode;

            handles.push(tokio::spawn(async move {
                let flows =
                    fetch_flows(mode, &addr, limit, namespace.as_deref(), None, Some(&name))
                        .await
                        .map(|(flows, _)| flows)
                        .unwrap_or_default();
                (name, flows)
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            if let Ok(result) = handle.await {
                results.push(result);
            }
        }
        results
    }

    /// Retrieve the most recent flows from the primary Hubble address.
    pub async fn get_flows(&self, limit: usize, namespace: Option<&str>) -> Result<Vec<Flow>> {
        Ok(self.get_flows_with_source(limit, namespace).await?.0)
    }

    /// Same as [`get_flows`], plus which backend produced the rows.
    pub async fn get_flows_with_source(
        &self,
        limit: usize,
        namespace: Option<&str>,
    ) -> Result<(Vec<Flow>, FlowSource)> {
        fetch_flows(self.mode, &self.address, limit, namespace, None, None).await
    }

    /// The most recent `limit` flows that have `verdict`, filtered by Hubble
    /// itself. Filtering the last N flows afterwards would return only the few
    /// drops that happen to be among them.
    pub async fn get_flows_by_verdict(
        &self,
        limit: usize,
        namespace: Option<&str>,
        verdict: Option<&str>,
    ) -> Result<Vec<Flow>> {
        Ok(
            fetch_flows(self.mode, &self.address, limit, namespace, verdict, None)
                .await?
                .0,
        )
    }

    /// Follow new flows as they happen. Connection errors are returned here;
    /// later failures arrive as a final [`LiveEvent::Ended`]. Dropping the
    /// receiver stops the stream (and kills the CLI process, if one is used).
    pub async fn stream_flows(
        &self,
        namespace: Option<&str>,
    ) -> Result<tokio::sync::mpsc::Receiver<LiveEvent>> {
        validate_namespace(namespace)?;
        match self.mode {
            HubbleMode::Cli => stream_cli(&self.address, namespace),
            HubbleMode::Grpc => stream_grpc(&self.address, namespace).await,
            HubbleMode::Auto => match stream_grpc(&self.address, namespace).await {
                Ok(rx) => Ok(rx),
                Err(e) if on_path("hubble") => {
                    tracing::warn!("Hubble gRPC stream failed ({e:#}); using the hubble CLI");
                    stream_cli(&self.address, namespace)
                }
                Err(e) => Err(e),
            },
        }
    }

    /// Compute aggregate flow statistics
    pub async fn get_flow_stats(&self) -> Result<FlowStats> {
        let flows = self.get_flows(1000, None).await?;

        let total = flows.len() as u64;
        let forwarded = flows.iter().filter(|f| f.verdict == "FORWARDED").count() as u64;
        let dropped = flows.iter().filter(|f| f.verdict == "DROPPED").count() as u64;

        Ok(FlowStats {
            total_flows: total,
            forwarded,
            dropped,
            requests_per_second: 0.0, // Would need time-window sampling
            avg_latency_ms: 0.0,      // Would need L7 data
        })
    }
}

/// Reject namespaces that could be read as CLI flags. Also applied on the gRPC
/// path so both backends accept exactly the same input.
fn validate_namespace(namespace: Option<&str>) -> Result<()> {
    if let Some(ns) = namespace {
        if ns.starts_with('-') || ns.contains(char::is_whitespace) {
            anyhow::bail!("Invalid namespace: '{}'", ns);
        }
    }
    Ok(())
}

/// The most recent `limit` flows from `address`, using the backend `mode` selects.
async fn fetch_flows(
    mode: HubbleMode,
    address: &str,
    limit: usize,
    namespace: Option<&str>,
    verdict: Option<&str>,
    cluster: Option<&str>,
) -> Result<(Vec<Flow>, FlowSource)> {
    // Cap the limit to prevent excessive resource consumption
    let limit = limit.min(10_000);
    validate_namespace(namespace)?;

    match mode {
        HubbleMode::Cli => Ok((
            cli_flows(address, limit, namespace, verdict, cluster).await,
            FlowSource::HubbleCli,
        )),
        HubbleMode::Grpc => {
            let flows =
                hubble_grpc::last_flows(address, limit, namespace, verdict, cluster).await?;
            Ok((flows, FlowSource::HubbleGrpc))
        }
        HubbleMode::Auto => {
            match hubble_grpc::last_flows(address, limit, namespace, verdict, cluster).await {
                Ok(flows) => Ok((flows, FlowSource::HubbleGrpc)),
                Err(e) if on_path("hubble") => {
                    tracing::debug!("Hubble gRPC failed for {address} ({e:#}); trying the CLI");
                    Ok((
                        cli_flows(address, limit, namespace, verdict, cluster).await,
                        FlowSource::HubbleCli,
                    ))
                }
                Err(e) => {
                    tracing::debug!("Hubble gRPC failed for {address} and no hubble CLI: {e:#}");
                    Ok((Vec::new(), FlowSource::Unavailable))
                }
            }
        }
    }
}

/// `hubble observe --last N`; empty when the binary is missing or fails.
async fn cli_flows(
    address: &str,
    limit: usize,
    namespace: Option<&str>,
    verdict: Option<&str>,
    cluster: Option<&str>,
) -> Vec<Flow> {
    let mut cmd = Command::new("hubble");
    cmd.arg("observe")
        .arg("--output")
        .arg("json")
        .arg("--last")
        .arg(limit.to_string())
        .arg("--server")
        .arg(address);
    if let Some(ns) = namespace {
        cmd.arg("--namespace").arg(ns);
    }
    // Only a verdict Hubble knows is passed; the caller post-filters the rest.
    if let Some(v) = verdict.and_then(hubble_grpc::parse_verdict) {
        cmd.arg("--verdict").arg(v.as_str_name());
    }

    let output = match cmd.output().await {
        Ok(out) => out,
        Err(e) => {
            tracing::debug!("hubble CLI not available for {}: {}", address, e);
            return Vec::new();
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::debug!(
            "hubble observe ({}) returned non-zero: {}",
            address,
            stderr.trim()
        );
        return Vec::new();
    }
    parse_hubble_output(&String::from_utf8_lossy(&output.stdout), cluster)
}

async fn stream_grpc(
    address: &str,
    namespace: Option<&str>,
) -> Result<tokio::sync::mpsc::Receiver<LiveEvent>> {
    let mut stream = hubble_grpc::follow_flows(address, namespace).await?;
    let (tx, rx) = tokio::sync::mpsc::channel(256);
    tokio::spawn(async move {
        loop {
            let ended = match stream.message().await {
                Ok(Some(msg)) => {
                    if let Some(flow) = hubble_grpc::flow_from_response(&msg) {
                        // The receiver is gone when the client disconnected.
                        if tx.send(LiveEvent::Flow(Box::new(flow))).await.is_err() {
                            return;
                        }
                    }
                    continue;
                }
                Ok(None) => "Hubble closed the stream".to_string(),
                Err(status) => format!("Hubble stream error: {}", status.message()),
            };
            let _ = tx.send(LiveEvent::Ended(ended)).await;
            return;
        }
    });
    Ok(rx)
}

fn stream_cli(
    address: &str,
    namespace: Option<&str>,
) -> Result<tokio::sync::mpsc::Receiver<LiveEvent>> {
    let mut cmd = Command::new("hubble");
    cmd.arg("observe")
        .arg("--follow")
        .arg("--output")
        .arg("json")
        .arg("--server")
        .arg(address);
    if let Some(ns) = namespace {
        cmd.arg("--namespace").arg(ns);
    }
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to start hubble observe: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to capture hubble output"))?;

    let (tx, rx) = tokio::sync::mpsc::channel(256);
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let ended = loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                        continue;
                    };
                    // Lost-event and node-status lines are not flows.
                    let Some(flow) = flow_from_hubble_line(0, &v) else {
                        continue;
                    };
                    if tx.send(LiveEvent::Flow(Box::new(flow))).await.is_err() {
                        break None; // client gone; `child` is killed on drop
                    }
                }
                Ok(None) => break Some("hubble observe process exited".to_string()),
                Err(e) => break Some(format!("Error reading hubble output: {e}")),
            }
        };
        if let Some(msg) = ended {
            let _ = tx.send(LiveEvent::Ended(msg)).await;
        }
        let _ = child.kill().await;
    });
    Ok(rx)
}

/// Parse the stdout of `hubble observe --output json`.
///
/// Hubble's `json` output is an alias for `jsonpb`: one `GetFlowsResponse` per
/// line, i.e. `{"flow": {...}, "node_name": "...", "time": "..."}`. Only a CLI
/// configured with `compat.legacy-json-output` prints the bare flow instead.
/// Both are accepted. Lines that are not flows (lost-event and node-status
/// messages) are skipped, so they cannot become empty "UNKNOWN" flows.
pub fn parse_hubble_output(stdout: &str, cluster: Option<&str>) -> Vec<Flow> {
    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|v| flow_from_hubble_line(0, &v))
        .map(|mut flow| {
            if let Some(c) = cluster {
                flow.cluster = Some(c.to_string());
            }
            flow
        })
        .collect()
}

/// One line of Hubble output as a flow, or None if the line is not a flow.
pub fn flow_from_hubble_line(index: usize, line: &serde_json::Value) -> Option<Flow> {
    let flow = match line.get("flow") {
        Some(f) if f.is_object() => f,
        // The bare-flow (legacy) shape has the verdict at the top level.
        _ if line.get("verdict").is_some() => line,
        _ => return None,
    };
    let mut out = hubble_json_to_flow(index, flow);
    if out.timestamp.is_empty() {
        // The response envelope carries a time too.
        if let Some(t) = line.get("time").and_then(|x| x.as_str()) {
            out.timestamp = t.to_string();
        }
    }
    Some(out)
}

/// FNV-1a over the parts, with a separator so ("ab","c") differs from ("a","bc").
/// Chosen over `DefaultHasher` because its output is specified and will not
/// change with a Rust upgrade.
fn fnv1a(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for part in parts {
        for b in part.bytes().chain(std::iter::once(0xff)) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// Convert one Hubble flow object into our canonical Flow struct.
///
/// `_index` is unused: an id derived from a position in a batch changes as the
/// window slides, which duplicated history rows.
pub fn hubble_json_to_flow(_index: usize, v: &serde_json::Value) -> Flow {
    let src = v.get("source").unwrap_or(v);
    let dst = v.get("destination").unwrap_or(v);
    // Endpoints carry no IP in real Hubble output: addresses live in a separate
    // `IP: {source, destination}` object, which is the only place a flow to or
    // from the outside world has one.
    let ip_obj = v.get("IP");
    let ip_of = |ep: &serde_json::Value, side: &str| {
        json_str(ep, "ip")
            .or_else(|| json_str(ep, "IP"))
            .or_else(|| ip_obj.map(|o| json_str(o, side)).unwrap_or_default())
    };

    // Extract L7 HTTP fields if present
    let l7_http = v.get("l7").and_then(|l7| {
        l7.get("http")
            .or_else(|| l7.get("Http"))
            .or_else(|| l7.get("HTTP"))
    });

    let http_method = l7_http.and_then(|http| {
        http.get("method")
            .or_else(|| http.get("Method"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
    });

    let http_url = l7_http.and_then(|http| {
        http.get("url")
            .or_else(|| http.get("Url"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
    });

    let http_code = l7_http.and_then(|http| {
        http.get("code")
            .or_else(|| http.get("Code"))
            .and_then(|x| x.as_u64())
            .map(|c| c as u16)
    });

    let timestamp = v
        .get("time")
        .or_else(|| v.get("timestamp"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let source = FlowEndpoint {
        namespace: json_str(src, "namespace"),
        pod: json_str(src, "pod_name").or_else(|| json_str(src, "pod")),
        ip: ip_of(src, "source"),
    };
    let destination = FlowEndpoint {
        namespace: json_str(dst, "namespace"),
        pod: json_str(dst, "pod_name").or_else(|| json_str(dst, "pod")),
        ip: ip_of(dst, "destination"),
    };
    let verdict = v
        .get("verdict")
        .and_then(|x| x.as_str())
        .unwrap_or("UNKNOWN")
        .to_string();
    let protocol = v
        .get("l4")
        .and_then(|l4| {
            if l4.get("TCP").is_some() {
                Some("TCP")
            } else if l4.get("UDP").is_some() {
                Some("UDP")
            } else if l4.get("ICMPv4").is_some() {
                Some("ICMPv4")
            } else {
                None
            }
        })
        .unwrap_or("UNKNOWN")
        .to_string();
    let port = v
        .get("l4")
        .and_then(|l4| {
            l4.get("TCP")
                .or_else(|| l4.get("UDP"))
                .and_then(|proto| proto.get("destination_port"))
                .and_then(|p| p.as_u64())
        })
        .unwrap_or(0) as u16;

    // Hubble's own id when it sends one (`uuid`, Cilium 1.14+); otherwise one
    // derived from the flow's content, which is the same on every poll that
    // returns this flow.
    let id = v
        .get("uuid")
        .or_else(|| v.get("id"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            content_id(&timestamp, &source, &destination, &verdict, &protocol, port)
        });

    let l7_dns = v.get("l7").and_then(|l7| {
        l7.get("dns")
            .or_else(|| l7.get("Dns"))
            .or_else(|| l7.get("DNS"))
    });
    let dns_query = l7_dns
        .and_then(|d| d.get("query").or_else(|| d.get("Query")))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let dns_qtypes = l7_dns.and_then(|d| {
        d.get("qtypes")
            .or_else(|| d.get("Qtypes"))
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty())
    });
    let dns_rcode = l7_dns
        .and_then(|d| d.get("rcode").or_else(|| d.get("Rcode")))
        .and_then(|x| x.as_u64())
        .map(|n| n as u32);
    let dns_rcode_name = dns_rcode
        .map(crate::models::flow::dns_rcode_name)
        .map(str::to_string);
    let dns_ips = l7_dns.and_then(|d| {
        d.get("ips")
            .or_else(|| d.get("Ips"))
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty())
    });
    let dns_latency_ns = v
        .get("l7")
        .and_then(|l7| l7.get("latency_ns").or_else(|| l7.get("latencyNs")))
        .and_then(|x| x.as_u64())
        .filter(|&n| n > 0);
    let drop_reason = v
        .get("drop_reason_desc")
        .or_else(|| v.get("drop_reason"))
        .and_then(|x| {
            x.as_str()
                .map(|s| s.to_string())
                .or_else(|| x.as_u64().map(|n| n.to_string()))
        })
        .filter(|s| !s.is_empty() && s != "DROP_REASON_UNKNOWN" && s != "0");

    let hubble = hubble_json_meta(v);

    Flow {
        id,
        timestamp,
        source,
        destination,
        verdict,
        protocol,
        port,
        hubble,
        http_method,
        http_url,
        http_code,
        cluster: v
            .get("cluster")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
        dns_query,
        dns_qtypes,
        dns_rcode,
        dns_rcode_name,
        dns_ips,
        dns_latency_ns,
        drop_reason,
    }
}

/// Context fields of a `hubble observe -o json` flow (same data the gRPC path
/// reads from the protobuf). Enum fields arrive as names, `policy_match_type`
/// as a number; anything absent stays `None`.
fn hubble_json_meta(v: &serde_json::Value) -> Option<crate::models::flow::FlowMeta> {
    use crate::models::flow::{policy_match_name, FlowMeta, MAX_FLOW_LABELS};
    let s = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .filter(|x| !x.is_empty() && !x.ends_with("UNKNOWN") && *x != "UNKNOWN_POINT")
            .map(str::to_string)
    };
    let identity = |ep: &str| {
        v.get(ep)
            .and_then(|e| e.get("identity"))
            .and_then(|x| x.as_u64())
            .filter(|&i| i != 0)
            .and_then(|i| u32::try_from(i).ok())
    };
    let labels = |ep: &str| -> Vec<String> {
        v.get(ep)
            .and_then(|e| e.get("labels"))
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|l| l.as_str().map(str::to_string))
                    .take(MAX_FLOW_LABELS)
                    .collect()
            })
            .unwrap_or_default()
    };
    FlowMeta {
        traffic_direction: s("traffic_direction"),
        is_reply: v.get("is_reply").and_then(|x| x.as_bool()),
        policy_match_type: v
            .get("policy_match_type")
            .and_then(|x| x.as_u64())
            .and_then(|n| policy_match_name(n as u32)),
        trace_observation_point: s("trace_observation_point"),
        node_name: s("node_name"),
        source_identity: identity("source"),
        destination_identity: identity("destination"),
        source_labels: labels("source"),
        destination_labels: labels("destination"),
    }
    .or_none()
}

/// An id derived from a flow's content, for flows Hubble sent without a `uuid`.
/// Shared by the CLI and gRPC paths so the same flow gets the same id from both.
pub(crate) fn content_id(
    timestamp: &str,
    source: &FlowEndpoint,
    destination: &FlowEndpoint,
    verdict: &str,
    protocol: &str,
    port: u16,
) -> String {
    format!(
        "h-{:016x}",
        fnv1a(&[
            timestamp,
            &source.namespace,
            &source.pod,
            &source.ip,
            &destination.namespace,
            &destination.pod,
            &destination.ip,
            verdict,
            protocol,
            &port.to_string(),
        ])
    )
}

/// Helper: extract a string field from a JSON value, returning empty string if absent
fn json_str(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

/// Extension trait so we can chain .or_else on String (not Option)
trait StringOrElse {
    fn or_else(self, f: impl FnOnce() -> String) -> String;
}

impl StringOrElse for String {
    fn or_else(self, f: impl FnOnce() -> String) -> String {
        if self.is_empty() {
            f()
        } else {
            self
        }
    }
}

#[cfg(test)]
mod hubble_format_tests {
    use super::*;
    use serde_json::json;

    /// A drop as `hubble observe --output json` prints it: a GetFlowsResponse with
    /// the flow under `flow`, proto field names (snake_case), and addresses in `IP`.
    fn wrapped_drop() -> serde_json::Value {
        json!({
            "flow": {
                "time": "2026-09-24T04:34:47.290678123Z",
                "uuid": "0f9c3a2e-7b1d-4c55-9a40-1e2d3c4b5a69",
                "verdict": "DROPPED",
                "drop_reason": 133,
                "ethernet": { "source": "aa:bb:cc:dd:ee:01", "destination": "aa:bb:cc:dd:ee:02" },
                "IP": { "source": "10.0.1.5", "destination": "10.0.2.7", "ipVersion": "IPv4" },
                "l4": { "TCP": { "source_port": 41234, "destination_port": 8080, "flags": { "SYN": true } } },
                "source": { "ID": 1234, "identity": 54321, "cluster_name": "default", "namespace": "shop",
                            "labels": ["k8s:app=web"], "pod_name": "web-1",
                            "workloads": [{ "name": "web", "kind": "Deployment" }] },
                "destination": { "ID": 88, "identity": 12345, "namespace": "pay", "labels": ["k8s:app=gw"], "pod_name": "gw-1" },
                "Type": "L3_L4",
                "node_name": "kind-worker",
                "event_type": { "type": 1, "sub_type": 133 },
                "traffic_direction": "INGRESS",
                "trace_observation_point": "TO_ENDPOINT",
                "drop_reason_desc": "POLICY_DENIED",
                "is_reply": false,
                "Summary": "TCP Flags: SYN"
            },
            "node_name": "kind-worker",
            "time": "2026-09-24T04:34:47.290678123Z"
        })
    }

    #[test]
    fn parses_the_default_wrapped_output() {
        let f = flow_from_hubble_line(0, &wrapped_drop()).expect("a flow");
        assert_eq!(f.id, "0f9c3a2e-7b1d-4c55-9a40-1e2d3c4b5a69");
        assert_eq!(f.timestamp, "2026-09-24T04:34:47.290678123Z");
        assert_eq!(f.verdict, "DROPPED");
        assert_eq!((f.protocol.as_str(), f.port), ("TCP", 8080));
        assert_eq!(
            (f.source.namespace.as_str(), f.source.pod.as_str()),
            ("shop", "web-1")
        );
        assert_eq!(
            (f.destination.namespace.as_str(), f.destination.pod.as_str()),
            ("pay", "gw-1")
        );
    }

    #[test]
    fn takes_addresses_from_the_ip_object_because_endpoints_have_none() {
        let f = flow_from_hubble_line(0, &wrapped_drop()).unwrap();
        assert_eq!(f.source.ip, "10.0.1.5");
        assert_eq!(f.destination.ip, "10.0.2.7");
    }

    #[test]
    fn reads_identities_labels_direction_and_node() {
        let mut line = wrapped_drop();
        line["flow"]["policy_match_type"] = json!(4);
        let meta = flow_from_hubble_line(0, &line)
            .unwrap()
            .hubble
            .expect("context");
        assert_eq!(meta.traffic_direction.as_deref(), Some("INGRESS"));
        assert_eq!(meta.is_reply, Some(false));
        assert_eq!(meta.policy_match_type.as_deref(), Some("all"));
        assert_eq!(meta.trace_observation_point.as_deref(), Some("TO_ENDPOINT"));
        assert_eq!(meta.node_name.as_deref(), Some("kind-worker"));
        assert_eq!(meta.source_identity, Some(54321));
        assert_eq!(meta.destination_identity, Some(12345));
        assert_eq!(meta.source_labels, vec!["k8s:app=web".to_string()]);
        assert_eq!(meta.destination_labels, vec!["k8s:app=gw".to_string()]);
    }

    #[test]
    fn unknown_enum_names_are_dropped_not_shown() {
        let mut line = wrapped_drop();
        line["flow"]["traffic_direction"] = json!("TRAFFIC_DIRECTION_UNKNOWN");
        line["flow"]["trace_observation_point"] = json!("UNKNOWN_POINT");
        let meta = flow_from_hubble_line(0, &line).unwrap().hubble.unwrap();
        assert_eq!(meta.traffic_direction, None);
        assert_eq!(meta.trace_observation_point, None);
    }

    #[test]
    fn a_flow_from_outside_the_cluster_has_an_address_but_no_namespace_or_pod() {
        let mut line = wrapped_drop();
        line["flow"]["source"] = json!({ "identity": 2, "labels": ["reserved:world"] });
        line["flow"]["IP"]["source"] = json!("203.0.113.9");
        let f = flow_from_hubble_line(0, &line).unwrap();
        assert_eq!(
            (
                f.source.namespace.as_str(),
                f.source.pod.as_str(),
                f.source.ip.as_str()
            ),
            ("", "", "203.0.113.9")
        );
    }

    #[test]
    fn still_accepts_the_bare_legacy_shape() {
        let bare = wrapped_drop()["flow"].clone();
        let legacy = flow_from_hubble_line(0, &bare).expect("legacy flow");
        let wrapped = flow_from_hubble_line(0, &wrapped_drop()).unwrap();
        assert_eq!(
            serde_json::to_value(&legacy).unwrap(),
            serde_json::to_value(&wrapped).unwrap()
        );
    }

    #[test]
    fn skips_lines_that_are_not_flows() {
        for line in [
            json!({ "lost_events": { "source": "HUBBLE_RING_BUFFER", "num_events_lost": 5 }, "node_name": "n" }),
            json!({ "node_status": { "state_changes": [], "node_names": ["a"] } }),
            json!({}),
            json!({ "flow": "not an object" }),
            json!([1, 2, 3]),
        ] {
            assert!(flow_from_hubble_line(0, &line).is_none(), "{line}");
        }
    }

    #[test]
    fn parse_output_keeps_flows_in_order_and_drops_everything_else() {
        let a = wrapped_drop();
        let mut b = wrapped_drop();
        b["flow"]["uuid"] = json!("second");
        b["flow"]["verdict"] = json!("FORWARDED");
        let stdout = format!(
            "{}\n\n{}\nnot json at all\n{}\n{}\n",
            a,
            json!({ "lost_events": { "num_events_lost": 1 } }),
            b,
            json!({ "node_status": {} })
        );
        let flows = parse_hubble_output(&stdout, Some("east"));
        assert_eq!(flows.len(), 2);
        assert_eq!(
            (flows[0].verdict.as_str(), flows[1].verdict.as_str()),
            ("DROPPED", "FORWARDED")
        );
        assert_eq!(flows[1].id, "second");
        assert!(flows.iter().all(|f| f.cluster.as_deref() == Some("east")));
        assert!(parse_hubble_output("", None).is_empty());
    }

    #[test]
    fn falls_back_to_the_envelope_time_when_the_flow_has_none() {
        let mut line = wrapped_drop();
        line["flow"].as_object_mut().unwrap().remove("time");
        line["time"] = json!("2026-09-24T05:00:00Z");
        assert_eq!(
            flow_from_hubble_line(0, &line).unwrap().timestamp,
            "2026-09-24T05:00:00Z"
        );
        // A flow's own time wins over the envelope's.
        let mut both = wrapped_drop();
        both["time"] = json!("2030-01-01T00:00:00Z");
        assert_eq!(
            flow_from_hubble_line(0, &both).unwrap().timestamp,
            "2026-09-24T04:34:47.290678123Z"
        );
    }

    #[test]
    fn derived_ids_are_stable_wherever_the_flow_sits_in_the_batch() {
        let mut line = wrapped_drop();
        line["flow"].as_object_mut().unwrap().remove("uuid");
        let at_0 = flow_from_hubble_line(0, &line).unwrap();
        let at_7 = flow_from_hubble_line(7, &line).unwrap();
        assert_eq!(
            at_0.id, at_7.id,
            "a sliding window must not change a flow's id"
        );
        assert!(
            at_0.id.starts_with("h-") && at_0.id.len() == 18,
            "{}",
            at_0.id
        );
        assert!(at_0.id[2..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn derived_ids_differ_when_any_identifying_field_differs() {
        let base = || {
            let mut l = wrapped_drop();
            l["flow"].as_object_mut().unwrap().remove("uuid");
            l
        };
        let id = |l: &serde_json::Value| flow_from_hubble_line(0, l).unwrap().id;
        let original = id(&base());
        for mutate in [
            |l: &mut serde_json::Value| l["flow"]["time"] = json!("2026-09-24T04:34:47.290678124Z"),
            |l: &mut serde_json::Value| l["flow"]["verdict"] = json!("FORWARDED"),
            |l: &mut serde_json::Value| l["flow"]["l4"]["TCP"]["destination_port"] = json!(8081),
            |l: &mut serde_json::Value| l["flow"]["source"]["pod_name"] = json!("web-2"),
            |l: &mut serde_json::Value| l["flow"]["destination"]["namespace"] = json!("other"),
            |l: &mut serde_json::Value| l["flow"]["IP"]["destination"] = json!("10.0.2.8"),
        ] {
            let mut l = base();
            mutate(&mut l);
            assert_ne!(id(&l), original);
        }
    }

    #[test]
    fn a_uuid_is_used_as_the_id_when_hubble_sends_one() {
        assert_eq!(
            flow_from_hubble_line(3, &wrapped_drop()).unwrap().id,
            "0f9c3a2e-7b1d-4c55-9a40-1e2d3c4b5a69"
        );
    }

    #[test]
    fn hash_separates_fields_so_boundaries_matter() {
        assert_ne!(fnv1a(&["ab", "c"]), fnv1a(&["a", "bc"]));
        assert_eq!(fnv1a(&["x", "y"]), fnv1a(&["x", "y"]));
        // FNV-1a is a fixed algorithm: pin one value so an accidental change to it
        // (which would re-key every stored flow) fails loudly.
        assert_eq!(fnv1a(&[]), 0xcbf29ce484222325);
    }

    #[test]
    fn reads_udp_icmp_and_http_details() {
        let mut udp = wrapped_drop();
        udp["flow"]["l4"] = json!({ "UDP": { "source_port": 5353, "destination_port": 53 } });
        let f = flow_from_hubble_line(0, &udp).unwrap();
        assert_eq!((f.protocol.as_str(), f.port), ("UDP", 53));

        let mut icmp = wrapped_drop();
        icmp["flow"]["l4"] = json!({ "ICMPv4": { "type": 8, "code": 0 } });
        let f = flow_from_hubble_line(0, &icmp).unwrap();
        assert_eq!((f.protocol.as_str(), f.port), ("ICMPv4", 0));

        let mut http = wrapped_drop();
        http["flow"]["l7"] = json!({ "type": "RESPONSE", "http": { "code": 503, "method": "GET", "url": "http://gw/pay", "protocol": "HTTP/1.1" } });
        let f = flow_from_hubble_line(0, &http).unwrap();
        assert_eq!(
            (f.http_method.as_deref(), f.http_url.as_deref(), f.http_code),
            (Some("GET"), Some("http://gw/pay"), Some(503))
        );
    }
}

#[cfg(test)]
mod backend_tests {
    use super::*;
    use crate::services::hubble_grpc::pb::flow::Verdict;
    use crate::services::hubble_grpc::testing::{serve, tcp_flow};

    fn svc(addr: &str, mode: HubbleMode) -> HubbleService {
        HubbleService::new(addr, vec![("east".into(), addr.into())])
            .unwrap()
            .with_mode(mode)
    }

    #[test]
    fn mode_parsing() {
        assert_eq!(HubbleMode::parse("GRPC"), Some(HubbleMode::Grpc));
        assert_eq!(HubbleMode::parse(" cli "), Some(HubbleMode::Cli));
        assert_eq!(HubbleMode::parse("auto"), Some(HubbleMode::Auto));
        assert_eq!(HubbleMode::parse(""), Some(HubbleMode::Auto));
        assert_eq!(HubbleMode::parse("grcp"), None);
        assert_eq!(HubbleMode::default(), HubbleMode::Auto);
    }

    #[tokio::test]
    async fn grpc_mode_reads_flows_health_and_clusters_without_the_cli() {
        let (addr, _) = serve(vec![
            tcp_flow("a", Verdict::Forwarded, 80),
            tcp_flow("b", Verdict::Dropped, 81),
        ])
        .await;
        let s = svc(&addr, HubbleMode::Grpc);
        assert!(s.is_healthy().await);
        let flows = s.get_flows(10, None).await.unwrap();
        assert_eq!(flows.len(), 2);
        assert_eq!(flows[1].verdict, "DROPPED");

        let multi = s.get_flows_multi_cluster(10, None).await;
        assert_eq!(multi.len(), 1);
        assert_eq!(multi[0].0, "east");
        assert!(multi[0]
            .1
            .iter()
            .all(|f| f.cluster.as_deref() == Some("east")));

        let stats = s.get_flow_stats().await.unwrap();
        assert_eq!((stats.total_flows, stats.dropped), (2, 1));
    }

    #[tokio::test]
    async fn grpc_mode_reports_errors_instead_of_pretending_there_are_no_flows() {
        let closed = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().to_string()
        };
        let s = svc(&closed, HubbleMode::Grpc);
        assert!(!s.is_healthy().await);
        assert!(s.get_flows(5, None).await.is_err());
        assert!(s.stream_flows(None).await.is_err());
    }

    #[tokio::test]
    async fn both_backends_reject_flag_like_namespaces() {
        let (addr, seen) = serve(vec![]).await;
        for mode in [HubbleMode::Grpc, HubbleMode::Cli] {
            let s = svc(&addr, mode);
            assert!(s.get_flows(5, Some("--all")).await.is_err());
            assert!(s.get_flows(5, Some("a b")).await.is_err());
            assert!(s.stream_flows(Some("-x")).await.is_err());
        }
        assert!(seen.lock().unwrap().is_empty(), "nothing may reach Hubble");
    }

    #[tokio::test]
    async fn live_stream_yields_flows_then_ends() {
        let (addr, seen) = serve(vec![
            tcp_flow("a", Verdict::Forwarded, 80),
            tcp_flow("b", Verdict::Forwarded, 81),
        ])
        .await;
        let s = svc(&addr, HubbleMode::Grpc);
        let mut rx = s.stream_flows(Some("shop")).await.unwrap();
        let mut ids = Vec::new();
        let ended = loop {
            match rx.recv().await {
                Some(LiveEvent::Flow(f)) => ids.push(f.id),
                Some(LiveEvent::Ended(why)) => break why,
                None => panic!("channel closed without an Ended event"),
            }
        };
        assert_eq!(ids, ["a", "b"]);
        assert!(ended.contains("closed"), "{ended}");
        let req = seen.lock().unwrap()[0].clone();
        assert!(req.follow);
        assert_eq!(req.whitelist.len(), 2);
    }

    #[tokio::test]
    async fn cli_mode_never_touches_grpc() {
        let (addr, seen) = serve(vec![tcp_flow("a", Verdict::Forwarded, 80)]).await;
        let s = svc(&addr, HubbleMode::Cli);
        // Whether or not a hubble binary exists here, the fake must see no request.
        let _ = s.get_flows(5, None).await;
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn grpc_mode_health_needs_a_real_observer_not_just_an_open_port() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            loop {
                let _ = l.accept().await; // accept and hold: a port that speaks no gRPC
            }
        });
        assert!(!svc(&addr, HubbleMode::Grpc).is_healthy().await);
        assert!(
            svc(&addr, HubbleMode::Cli).is_healthy().await,
            "CLI mode only needs TCP"
        );
    }

    /// Ids of flows without a uuid are stored in history; changing how they are
    /// derived would re-insert every stored flow under a new id.
    #[test]
    fn content_id_format_is_pinned() {
        let ep = |ns: &str, pod: &str, ip: &str| FlowEndpoint {
            namespace: ns.into(),
            pod: pod.into(),
            ip: ip.into(),
        };
        let id = content_id(
            "2023-11-14T22:13:20.123456Z",
            &ep("shop", "web-1", "10.0.0.1"),
            &ep("db", "pg-0", "10.0.0.2"),
            "DROPPED",
            "TCP",
            5432,
        );
        assert_eq!(id, "h-a51f07c2d2cba2de");
    }
}
