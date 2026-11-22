// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

use anyhow::{Context, Result};
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::Mutex;

#[cfg(feature = "grpc")]
pub mod grpc;

#[cfg(feature = "grpc")]
pub enum HubbleClient {
    Grpc(grpc::GrpcHubbleClient),
    Cli(CliHubbleClient),
}

#[cfg(not(feature = "grpc"))]
pub type HubbleClient = CliHubbleClient;

#[cfg(feature = "grpc")]
impl HubbleClient {
    pub async fn new(port: u16, use_grpc: bool) -> Result<Self> {
        if use_grpc {
            match grpc::GrpcHubbleClient::connect(format!("http://localhost:{}", port)).await {
                Ok(client) => Ok(HubbleClient::Grpc(client)),
                Err(e) => {
                    tracing::warn!("gRPC connection failed, falling back to CLI: {}", e);
                    Ok(HubbleClient::Cli(CliHubbleClient::new(port)))
                }
            }
        } else {
            Ok(HubbleClient::Cli(CliHubbleClient::new(port)))
        }
    }

    pub async fn get_flows(&mut self) -> Result<Vec<Flow>> {
        match self {
            HubbleClient::Grpc(client) => client.get_flows(100).await,
            HubbleClient::Cli(client) => client.get_flows().await,
        }
    }
}

/// Global handle to keep the port-forward process alive for the lifetime of the application.
/// Uses Mutex<Option<>> instead of OnceLock to allow reconnection if the process dies.
static PORT_FORWARD_HANDLE: std::sync::OnceLock<Mutex<Option<Arc<Mutex<tokio::process::Child>>>>> =
    std::sync::OnceLock::new();

/// Start Hubble port-forward and verify it is running.
/// Returns the port on success, or an error if port-forward fails to start.
pub async fn start_port_forward() -> Result<u16> {
    start_port_forward_on(4245).await
}

/// Start Hubble port-forward on a specific port and verify it is running.
pub async fn start_port_forward_on(port: u16) -> Result<u16> {
    // Check if an existing port-forward is already working on this port
    if tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .is_ok()
    {
        tracing::info!("Reusing existing Hubble port-forward on port {}", port);
        println!("Hubble port-forward already active on port {}", port);
        return Ok(port);
    }

    let child = Command::new("cilium")
        .args(["hubble", "port-forward"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("Failed to start cilium hubble port-forward. Is cilium CLI installed?")?;

    let pid = child.id().unwrap_or(0);
    tracing::info!("Started hubble port-forward (pid: {})", pid);

    // Wrap the child in Arc<Mutex> and store globally to prevent drop.
    // kill_on_drop(true) ensures the process is killed when the app exits.
    let child = Arc::new(Mutex::new(child));
    let handle_store = PORT_FORWARD_HANDLE.get_or_init(|| Mutex::new(None));
    // Kill the old process before replacing the handle to prevent a process leak
    {
        let mut guard = handle_store.lock().await;
        if let Some(old_child) = guard.take() {
            let mut old_guard = old_child.lock().await;
            if let Err(e) = old_guard.kill().await {
                tracing::warn!("Failed to kill old port-forward process: {}", e);
            }
        }
        *guard = Some(child.clone());
    }

    // Wait and verify the port-forward is actually listening.
    // Port-forward setup involves discovering the hubble-relay pod and
    // establishing a tunnel, which can take 15-30s on real clusters.
    let max_retries = 30;
    for attempt in 1..=max_retries {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Check if the child process has exited unexpectedly
        {
            let mut child_guard = child.lock().await;
            match child_guard.try_wait() {
                Ok(Some(status)) => {
                    anyhow::bail!(
                        "Hubble port-forward process exited unexpectedly ({}). \
                         Ensure Hubble relay is running (cilium hubble enable).",
                        status
                    );
                }
                Ok(None) => {} // Still running, try connecting
                Err(e) => {
                    tracing::warn!("Failed to check port-forward process status: {}", e);
                }
            }
        }

        // Try connecting to verify port-forward is ready
        match tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await {
            Ok(_) => {
                tracing::info!("Hubble port-forward ready on port {}", port);
                return Ok(port);
            }
            Err(_) if attempt < max_retries => {
                tracing::debug!(
                    "Port-forward not ready yet (attempt {}/{})",
                    attempt,
                    max_retries
                );
            }
            Err(e) => {
                tracing::error!(
                    "Port-forward failed to start after {} attempts: {}",
                    max_retries,
                    e
                );
                // Kill the process since it's not working
                let mut child_guard = child.lock().await;
                if let Err(kill_err) = child_guard.kill().await {
                    tracing::warn!("Failed to kill port-forward process: {}", kill_err);
                }
                anyhow::bail!(
                    "Hubble port-forward failed to start on port {}. \
                     Ensure Hubble is enabled and the Hubble relay is running.",
                    port
                );
            }
        }
    }

    Ok(port)
}

pub struct CliHubbleClient {
    _port: u16,
}

impl CliHubbleClient {
    async fn get_flows_impl() -> Result<Vec<Flow>> {
        let output = tokio::process::Command::new("cilium")
            .args(["hubble", "observe", "--last", "100", "-o", "json"])
            .output()
            .await
            .context("Failed to execute cilium hubble observe")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(
                "cilium hubble observe exited with {}: {}",
                output.status,
                stderr
            );
        }

        let flows_json = String::from_utf8_lossy(&output.stdout);
        let flows: Vec<Flow> = flows_json
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

        Ok(flows)
    }
}

#[cfg(feature = "grpc")]
impl CliHubbleClient {
    pub fn new(port: u16) -> Self {
        Self { _port: port }
    }

    pub async fn get_flows(&self) -> Result<Vec<Flow>> {
        Self::get_flows_impl().await
    }
}

#[cfg(not(feature = "grpc"))]
impl CliHubbleClient {
    /// Create a new CLI Hubble client.
    /// The `_use_grpc` parameter is accepted for API compatibility with the grpc feature.
    pub async fn new(port: u16, _use_grpc: bool) -> Result<Self> {
        Ok(CliHubbleClient { _port: port })
    }

    pub async fn get_flows(&mut self) -> Result<Vec<Flow>> {
        Self::get_flows_impl().await
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Flow {
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub verdict: String,
    #[serde(default)]
    pub source: FlowEndpoint,
    #[serde(default)]
    pub destination: FlowEndpoint,
    #[serde(default)]
    pub r#type: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct FlowEndpoint {
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub pod_name: String,
    #[serde(default)]
    pub ip: String,
}
