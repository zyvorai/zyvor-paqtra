// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use anyhow::{Context, Result};
use k8s_openapi::api::core::v1::ServiceAccount;
use k8s_openapi::api::rbac::v1::{ClusterRoleBinding, RoleRef, Subject};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use std::collections::BTreeMap;
use tokio::process::Command;

use crate::kubernetes::K8sClient;

pub struct CiliumManager {
    k8s_client: K8sClient,
}

#[derive(Debug, Clone)]
pub struct CiliumStatus {
    pub installed: bool,
    pub version: Option<String>,
    #[allow(dead_code)]
    pub hubble_enabled: bool,
}

impl CiliumManager {
    pub fn new(k8s_client: K8sClient) -> Self {
        Self { k8s_client }
    }

    pub async fn is_installed(&self) -> Result<bool> {
        let pods = self
            .k8s_client
            .get_pods_by_label("kube-system", "k8s-app=cilium")
            .await?;

        Ok(!pods.is_empty())
    }

    pub async fn get_status(&self) -> Result<CiliumStatus> {
        let installed = self.is_installed().await?;

        if !installed {
            return Ok(CiliumStatus {
                installed: false,
                version: None,
                hubble_enabled: false,
            });
        }

        // Get version from cilium CLI if available
        let version = self.get_cilium_version().await.ok();

        // Check if Hubble is enabled by looking for hubble-relay
        let hubble_pods = self
            .k8s_client
            .get_pods_by_label("kube-system", "k8s-app=hubble-relay")
            .await
            .unwrap_or_default();

        Ok(CiliumStatus {
            installed: true,
            version,
            hubble_enabled: !hubble_pods.is_empty(),
        })
    }

    async fn get_cilium_version(&self) -> Result<String> {
        let output = Command::new("cilium")
            .args(["version", "--client"])
            .output()
            .await
            .context("Failed to get Cilium version")?;

        if output.status.success() {
            let version_str = String::from_utf8_lossy(&output.stdout);
            // Extract version from output
            for line in version_str.lines() {
                if line.contains("cilium-cli") {
                    if let Some(version) = line.split_whitespace().last() {
                        return Ok(version.to_string());
                    }
                }
            }
        }

        Ok("unknown".to_string())
    }

    pub async fn install(&self, auto_approve: bool) -> Result<()> {
        println!("🔍 Cilium not detected in the cluster.");

        // Check if cilium CLI is available
        if !self.is_cilium_cli_available().await {
            println!("📥 Cilium CLI not found. Installing automatically...");

            if !auto_approve {
                println!("\n❓ Would you like to install Cilium CLI? (Y/n)");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;

                let response = input.trim();
                if !response.is_empty() && !response.eq_ignore_ascii_case("y") {
                    anyhow::bail!("Cilium CLI installation cancelled.");
                }
            }

            self.install_cilium_cli().await?;
            println!("✅ Cilium CLI installed successfully!");
        }

        if !auto_approve {
            println!("\n❓ Would you like to install Cilium now? (y/N)");
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;

            if !input.trim().eq_ignore_ascii_case("y") {
                anyhow::bail!("Cilium installation cancelled. Please install Cilium manually:\n  cilium install");
            }
        }

        println!("📦 Installing Cilium...");

        let output = Command::new("cilium")
            .args(["install"])
            .status()
            .await
            .context("Failed to execute cilium install")?;

        if !output.success() {
            anyhow::bail!("Failed to install Cilium. Please check the error messages above.");
        }

        println!("⏳ Waiting for Cilium to be ready...");

        let status_output = Command::new("cilium")
            .args(["status", "--wait"])
            .status()
            .await
            .context("Failed to wait for Cilium status")?;

        if !status_output.success() {
            anyhow::bail!("Cilium installation completed but status check failed.");
        }

        println!("✅ Cilium installed successfully!");
        Ok(())
    }

    pub async fn upgrade(&self) -> Result<()> {
        println!("🔄 Upgrading Cilium...");

        let output = Command::new("cilium")
            .args(["upgrade"])
            .status()
            .await
            .context("Failed to execute cilium upgrade")?;

        if !output.success() {
            anyhow::bail!("Failed to upgrade Cilium");
        }

        println!("✅ Cilium upgraded successfully!");
        Ok(())
    }

    async fn is_cilium_cli_available(&self) -> bool {
        Command::new("cilium")
            .arg("version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    async fn install_cilium_cli(&self) -> Result<()> {
        // Detect OS and architecture
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;

        let (download_url, checksum_url, binary_name) = match (os, arch) {
            ("linux", "x86_64") => (
                "https://github.com/cilium/cilium-cli/releases/latest/download/cilium-linux-amd64.tar.gz",
                "https://github.com/cilium/cilium-cli/releases/latest/download/cilium-linux-amd64.tar.gz.sha256sum",
                "cilium-linux-amd64.tar.gz"
            ),
            ("linux", "aarch64") => (
                "https://github.com/cilium/cilium-cli/releases/latest/download/cilium-linux-arm64.tar.gz",
                "https://github.com/cilium/cilium-cli/releases/latest/download/cilium-linux-arm64.tar.gz.sha256sum",
                "cilium-linux-arm64.tar.gz"
            ),
            ("macos", _) | ("darwin", _) => {
                println!("For macOS, please install using Homebrew:");
                println!("   brew install cilium-cli");
                anyhow::bail!("Please install cilium-cli using Homebrew on macOS");
            },
            _ => {
                anyhow::bail!("Unsupported OS/architecture: {}/{}", os, arch);
            }
        };

        println!("Downloading Cilium CLI from GitHub...");

        // Download binary and checksum to /tmp
        let output = Command::new("curl")
            .args(["-fL", "--remote-name-all", download_url])
            .current_dir("/tmp")
            .status()
            .await
            .context("Failed to download Cilium CLI")?;

        if !output.success() {
            anyhow::bail!("Failed to download Cilium CLI");
        }

        let checksum_output = Command::new("curl")
            .args([
                "-fL",
                "-o",
                &format!("{}.sha256sum", binary_name),
                checksum_url,
            ])
            .current_dir("/tmp")
            .status()
            .await
            .context("Failed to download checksum file")?;

        if !checksum_output.success() {
            // Clean up the downloaded file
            let _ = std::fs::remove_file(format!("/tmp/{}", binary_name));
            anyhow::bail!(
                "Failed to download checksum file. Cannot verify download integrity. \
                 Aborting installation for security."
            );
        } else {
            // Verify checksum
            println!("Verifying download integrity...");
            let verify_output = Command::new("sha256sum")
                .args(["--check", &format!("{}.sha256sum", binary_name)])
                .current_dir("/tmp")
                .output()
                .await
                .context("Failed to verify checksum")?;

            if !verify_output.status.success() {
                // Clean up the downloaded file
                let _ = std::fs::remove_file(format!("/tmp/{}", binary_name));
                anyhow::bail!(
                    "Checksum verification failed! The downloaded file may be corrupted or tampered with. \
                     Aborting installation for security."
                );
            }
            println!("Checksum verified successfully.");
        }

        println!("Extracting Cilium CLI...");

        // Extract
        let extract_output = Command::new("tar")
            .args(["-xzf", binary_name])
            .current_dir("/tmp")
            .status()
            .await
            .context("Failed to extract Cilium CLI")?;

        if !extract_output.success() {
            anyhow::bail!("Failed to extract Cilium CLI");
        }

        println!("📁 Installing Cilium CLI to /usr/local/bin...");

        // Try to move to /usr/local/bin with sudo
        let install_output = Command::new("sudo")
            .args(["mv", "/tmp/cilium", "/usr/local/bin/"])
            .status()
            .await
            .context("Failed to install Cilium CLI to /usr/local/bin")?;

        if !install_output.success() {
            // Fallback: try to install to ~/.local/bin
            println!("⚠️  Failed to install to /usr/local/bin, trying ~/.local/bin...");

            let home = std::env::var("HOME")?;
            let local_bin = format!("{}/.local/bin", home);

            // Create ~/.local/bin if it doesn't exist
            std::fs::create_dir_all(&local_bin)?;

            let fallback_output = Command::new("mv")
                .args(["/tmp/cilium", &format!("{}/cilium", local_bin)])
                .status()
                .await
                .context("Failed to install Cilium CLI to ~/.local/bin")?;

            if !fallback_output.success() {
                anyhow::bail!("Failed to install Cilium CLI");
            }

            println!("ℹ️  Cilium CLI installed to {}", local_bin);
            println!("ℹ️  Make sure {} is in your PATH", local_bin);
        }

        // Clean up
        let _ = std::fs::remove_file(format!("/tmp/{}", binary_name));

        // Verify installation
        if !self.is_cilium_cli_available().await {
            anyhow::bail!(
                "Cilium CLI installed but not found in PATH. You may need to restart your shell."
            );
        }

        Ok(())
    }

    pub async fn enable_features(&self) -> Result<()> {
        let mut data = BTreeMap::new();
        data.insert("enable-hubble".to_string(), "true".to_string());
        data.insert("hubble-metrics-enabled".to_string(), "true".to_string());
        data.insert("hubble-listen-address".to_string(), ":4244".to_string());
        data.insert("hubble-relay-enabled".to_string(), "true".to_string());
        data.insert("monitor-aggregation".to_string(), "medium".to_string());
        data.insert("enable-l7-proxy".to_string(), "true".to_string());

        // Use patch instead of replace to avoid overwriting existing Cilium config keys
        let patch = serde_json::json!({
            "data": data
        });

        self.k8s_client
            .patch_configmap("kube-system", "cilium-config", &patch.to_string())
            .await?;

        Ok(())
    }

    /// Check if hubble-relay pods are running in the cluster.
    pub async fn is_hubble_relay_running(&self) -> bool {
        self.k8s_client
            .get_pods_by_label("kube-system", "k8s-app=hubble-relay")
            .await
            .map(|pods| !pods.is_empty())
            .unwrap_or(false)
    }

    /// Enable Hubble via the cilium CLI, which deploys hubble-relay.
    pub async fn enable_hubble(&self) -> Result<()> {
        if !self.is_cilium_cli_available().await {
            anyhow::bail!(
                "Cilium CLI is required to enable Hubble. Please install cilium-cli first."
            );
        }

        println!("📡 Enabling Hubble relay...");

        let output = Command::new("cilium")
            .args(["hubble", "enable"])
            .status()
            .await
            .context("Failed to execute 'cilium hubble enable'")?;

        if !output.success() {
            anyhow::bail!("Failed to enable Hubble. Please run 'cilium hubble enable' manually.");
        }

        // Wait for hubble-relay pods to become ready
        println!("⏳ Waiting for Hubble relay to be ready...");

        let max_wait = 60; // seconds
        for i in 0..max_wait {
            if self.is_hubble_relay_running().await {
                // Check if the pod is actually ready (not just existing)
                let ready = Command::new("kubectl")
                    .args([
                        "wait",
                        "--for=condition=ready",
                        "pod",
                        "-l",
                        "k8s-app=hubble-relay",
                        "-n",
                        "kube-system",
                        "--timeout=5s",
                    ])
                    .output()
                    .await
                    .map(|o| o.status.success())
                    .unwrap_or(false);

                if ready {
                    println!("✔ Hubble relay is ready");
                    return Ok(());
                }
            }

            if i % 10 == 0 && i > 0 {
                println!("  Still waiting for Hubble relay... ({}/{}s)", i, max_wait);
            }

            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        anyhow::bail!(
            "Hubble relay did not become ready within {}s. \
             Check 'kubectl get pods -n kube-system -l k8s-app=hubble-relay' for details.",
            max_wait
        );
    }

    pub async fn create_tui_service_account(&self) -> Result<()> {
        // Create ServiceAccount
        let sa = ServiceAccount {
            metadata: ObjectMeta {
                name: Some("paqtra".to_string()),
                namespace: Some("kube-system".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        self.k8s_client
            .create_or_update_service_account("kube-system", sa)
            .await?;

        // Create ClusterRoleBinding with least-privilege: view-only access
        // The TUI only needs to read pods, services, configmaps, and Cilium resources
        let crb = ClusterRoleBinding {
            metadata: ObjectMeta {
                name: Some("paqtra-binding".to_string()),
                ..Default::default()
            },
            subjects: Some(vec![Subject {
                kind: "ServiceAccount".to_string(),
                name: "paqtra".to_string(),
                namespace: Some("kube-system".to_string()),
                ..Default::default()
            }]),
            role_ref: RoleRef {
                api_group: "rbac.authorization.k8s.io".to_string(),
                kind: "ClusterRole".to_string(),
                name: "view".to_string(),
            },
        };

        self.k8s_client
            .create_or_update_cluster_role_binding(crb)
            .await?;

        Ok(())
    }
}
