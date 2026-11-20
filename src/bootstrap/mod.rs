// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use anyhow::Result;

use crate::cilium::CiliumManager;
use crate::kubernetes::K8sClient;
use crate::policies::PolicyManager;

pub struct BootstrapManager {
    k8s_client: K8sClient,
}

impl BootstrapManager {
    pub async fn new() -> Result<Self> {
        let k8s_client = K8sClient::new().await?;
        Ok(Self { k8s_client })
    }

    pub fn get_k8s_client(&self) -> K8sClient {
        self.k8s_client.clone()
    }

    #[allow(dead_code)]
    pub async fn run_bootstrap(&self) -> Result<BootstrapResult> {
        self.run_bootstrap_with_options(false, false).await
    }

    pub async fn run_bootstrap_with_options(
        &self,
        auto_install: bool,
        auto_upgrade: bool,
    ) -> Result<BootstrapResult> {
        println!("🚀 Bootstrapping Paqtra...");

        // Step 1: Detect cluster
        let context = self.detect_cluster().await?;
        println!("✔ Cluster detected: {}", context);

        // Step 2: Detect and install/upgrade Cilium if needed
        let cilium_mgr = CiliumManager::new(self.k8s_client.clone());
        let status = cilium_mgr.get_status().await?;

        if !status.installed {
            if auto_install {
                cilium_mgr.install(true).await?;
                println!("✔ Cilium installed");
            } else {
                // Ask user interactively
                cilium_mgr.install(false).await?;
                println!("✔ Cilium installed");
            }
        } else {
            println!("✔ Cilium detected");

            if let Some(version) = &status.version {
                println!("  Version: {}", version);
            }

            if auto_upgrade {
                println!("🔄 Auto-upgrade enabled, checking for updates...");
                cilium_mgr.upgrade().await?;
            }
        }

        // Step 3: Enable required Cilium features (configmap)
        self.enable_cilium_features().await?;

        // Step 3b: Ensure Hubble relay is actually deployed
        if !cilium_mgr.is_hubble_relay_running().await {
            println!("⚠ Hubble relay not found, installing...");
            cilium_mgr.enable_hubble().await?;
        } else {
            println!("✔ Hubble enabled");
        }

        // Step 4: Auto-create Cilium policies
        self.apply_default_policies().await?;
        println!("✔ Default policies applied");
        println!("✔ DNS allowed");

        // Step 5: Setup TUI service account
        self.setup_service_account().await?;

        // Step 6: Port-forward Hubble (background)
        let hubble_port = self.setup_hubble_port_forward().await?;
        println!("✔ Hubble port-forward started");
        println!("✔ Connected to Hubble");

        println!("\n🎉 Launching TUI...\n");

        Ok(BootstrapResult {
            context,
            hubble_port,
        })
    }

    async fn detect_cluster(&self) -> Result<String> {
        let context = self.k8s_client.get_current_context().await?;
        Ok(context)
    }

    async fn enable_cilium_features(&self) -> Result<()> {
        let cilium_mgr = CiliumManager::new(self.k8s_client.clone());
        cilium_mgr.enable_features().await?;
        Ok(())
    }

    async fn apply_default_policies(&self) -> Result<()> {
        let policy_mgr = PolicyManager::new(self.k8s_client.clone());

        // Apply default policies to all namespaces
        let namespaces = self.k8s_client.list_namespaces().await?;

        for ns in &namespaces {
            // Skip kube-system to avoid interfering with system components
            if ns == "kube-system" {
                continue;
            }

            policy_mgr.apply_intra_namespace_policy(ns).await?;
            policy_mgr.apply_dns_policy(ns).await?;
        }

        // Apply Hubble observability policy
        policy_mgr.apply_hubble_policy().await?;

        Ok(())
    }

    async fn setup_service_account(&self) -> Result<()> {
        let cilium_mgr = CiliumManager::new(self.k8s_client.clone());
        cilium_mgr.create_tui_service_account().await?;
        Ok(())
    }

    async fn setup_hubble_port_forward(&self) -> Result<u16> {
        crate::hubble::start_port_forward().await
    }
}

pub struct BootstrapResult {
    pub context: String,
    pub hubble_port: u16,
}
