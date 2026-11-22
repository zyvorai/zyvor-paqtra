// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

use crate::ebpf::{CiliumMapReader, EnrichedMapReader, MockMapReader};
use crate::endpoints::{Endpoint, EndpointManager};
use crate::hubble::{Flow, HubbleClient};
use crate::integration::{EnrichedConnection, IntegratedDataProvider};
use crate::kubernetes::K8sClient;
use crate::modules::autopolicy::{AutoPolicy, AutoPolicyConfig};
use crate::modules::canary::{CanaryConfig, CanaryEngine};
use crate::modules::chaos::{ChaosConfig, ChaosEngine};
use crate::modules::healer::{HealerConfig, SelfHealer};
use crate::modules::multicluster::{MultiClusterAutopilot, MultiClusterConfig};
use crate::modules::replay::{ReplayConfig, ReplayEngine};
use crate::modules::rootcause::{RootCauseConfig, RootCauseEngine};
use crate::modules::simulator::{Simulator, SimulatorConfig};
use anyhow::Result;

use super::autopolicy_view::AutoPolicyView;
use super::canary_view::CanaryView;
use super::chaos_view::ChaosView;
use super::multicluster_view::MultiClusterView;
use super::replay_view::ReplayView;
use super::rootcause_view::RootCauseView;
use super::simulator_view::SimulatorView;

/// Module container that can use either enriched or mock data
pub(crate) enum ModuleContainer {
    Enriched {
        healer: SelfHealer<EnrichedMapReader>,
        autopolicy: AutoPolicy<EnrichedMapReader>,
        rootcause: RootCauseEngine<EnrichedMapReader>,
        simulator: Simulator<EnrichedMapReader>,
        replay: ReplayEngine<EnrichedMapReader>,
    },
    Mock {
        healer: SelfHealer<MockMapReader>,
        autopolicy: AutoPolicy<MockMapReader>,
        rootcause: RootCauseEngine<MockMapReader>,
        simulator: Simulator<MockMapReader>,
        replay: ReplayEngine<MockMapReader>,
    },
}

impl ModuleContainer {
    /// Get healer stats (works for both enriched and mock)
    pub(crate) fn healer_stats(&self) -> crate::modules::healer::HealerStats {
        match self {
            ModuleContainer::Enriched { healer, .. } => healer.stats(),
            ModuleContainer::Mock { healer, .. } => healer.stats(),
        }
    }

    /// Check if using enriched data
    pub(crate) fn is_enriched(&self) -> bool {
        matches!(self, ModuleContainer::Enriched { .. })
    }
}

pub struct TuiApp {
    pub(crate) hubble_client: HubbleClient,
    pub(crate) endpoint_manager: EndpointManager,
    #[allow(dead_code)]
    pub(crate) k8s_client: K8sClient,
    pub(crate) flows: Vec<Flow>,
    pub(crate) endpoints: Vec<Endpoint>,
    pub(crate) selected_tab: usize,
    pub(crate) context: String,

    // Integration layer
    pub(crate) integrated_provider: Option<IntegratedDataProvider>,
    pub(crate) enriched_connections: Vec<EnrichedConnection>,

    // Intelligence modules (either enriched or mock)
    pub(crate) modules: ModuleContainer,

    // Standalone engines (don't depend on MapReader)
    pub(crate) chaos_engine: ChaosEngine,
    pub(crate) canary_engine: CanaryEngine,
    pub(crate) multicluster_engine: MultiClusterAutopilot,

    // Module state
    pub(crate) last_healer_run: std::time::Instant,
    pub(crate) last_autopolicy_update: std::time::Instant,

    // Status messages
    pub(crate) status_message: Option<String>,
    pub(crate) status_message_time: std::time::Instant,

    // View state
    pub(crate) simulator_view: SimulatorView,
    pub(crate) replay_view: ReplayView,
    pub(crate) chaos_view: ChaosView,
    pub(crate) canary_view: CanaryView,
    pub(crate) multicluster_view: MultiClusterView,
    pub(crate) autopolicy_view: AutoPolicyView,
    pub(crate) rootcause_view: RootCauseView,

    // AutoPolicy view state
    pub(crate) selected_policy_index: usize,
    pub(crate) policy_detail_mode: bool,
    pub(crate) policy_apply_confirmation: bool,
    pub(crate) policy_rollback_confirmation: bool,
    pub(crate) policy_batch_apply_confirmation: bool,
    pub(crate) policy_batch_rollback_confirmation: bool,
    pub(crate) applied_policies: std::collections::HashSet<String>,

    // RootCause view state
    pub(crate) selected_fix_index: usize,
    pub(crate) fix_apply_confirmation: bool,

    // Packet Explainer state
    pub(crate) selected_flow_index: usize,
    pub(crate) show_packet_explanation: bool,
    pub(crate) packet_explainer:
        tokio::sync::Mutex<crate::modules::packet_explainer::PacketExplainer>,

    // Pending async actions from sync confirmation handlers
    pub(crate) canary_pending_action: Option<super::canary_view::ConfirmationType>,

    // UX enhancements
    pub(crate) show_help: bool,
}

impl TuiApp {
    pub async fn new(context: String, hubble_port: u16, k8s_client: K8sClient) -> Result<Self> {
        let hubble_client = HubbleClient::new(hubble_port, true).await?;
        let endpoint_manager = EndpointManager::new(k8s_client.clone());

        // Initialize integrated data provider
        let integrated_provider = match IntegratedDataProvider::new(k8s_client.clone()).await {
            Ok(provider) => Some(provider),
            Err(e) => {
                tracing::warn!(
                    "Failed to initialize IntegratedDataProvider: {}. Using mock data.",
                    e
                );
                None
            }
        };

        // Initialize intelligence modules based on data availability
        let modules = if let Some(ref provider) = integrated_provider {
            // Try to create EnrichedMapReader
            match CiliumMapReader::new() {
                Ok(cilium_reader) => {
                    let enriched_reader =
                        EnrichedMapReader::new(cilium_reader, provider.identity_resolver().clone());

                    tracing::info!("Initializing intelligence modules with enriched data");

                    ModuleContainer::Enriched {
                        healer: SelfHealer::new(
                            HealerConfig::default(),
                            enriched_reader.clone(),
                            k8s_client.clone(),
                        ),
                        autopolicy: AutoPolicy::new(
                            AutoPolicyConfig::default(),
                            enriched_reader.clone(),
                            k8s_client.clone(),
                        ),
                        rootcause: RootCauseEngine::new(
                            RootCauseConfig::default(),
                            enriched_reader.clone(),
                            k8s_client.clone(),
                        ),
                        simulator: Simulator::new(
                            SimulatorConfig::default(),
                            enriched_reader.clone(),
                            k8s_client.clone(),
                        ),
                        replay: ReplayEngine::new(
                            ReplayConfig::default(),
                            enriched_reader,
                            k8s_client.clone(),
                        ),
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to create CiliumMapReader: {}. Using mock data.", e);
                    let mock_reader = MockMapReader;
                    ModuleContainer::Mock {
                        healer: SelfHealer::new(
                            HealerConfig::default(),
                            mock_reader,
                            k8s_client.clone(),
                        ),
                        autopolicy: AutoPolicy::new(
                            AutoPolicyConfig::default(),
                            mock_reader,
                            k8s_client.clone(),
                        ),
                        rootcause: RootCauseEngine::new(
                            RootCauseConfig::default(),
                            mock_reader,
                            k8s_client.clone(),
                        ),
                        simulator: Simulator::new(
                            SimulatorConfig::default(),
                            mock_reader,
                            k8s_client.clone(),
                        ),
                        replay: ReplayEngine::new(
                            ReplayConfig::default(),
                            mock_reader,
                            k8s_client.clone(),
                        ),
                    }
                }
            }
        } else {
            // Fallback to mock data
            tracing::warn!("Using mock data for intelligence modules");
            let mock_reader = MockMapReader;

            ModuleContainer::Mock {
                healer: SelfHealer::new(HealerConfig::default(), mock_reader, k8s_client.clone()),
                autopolicy: AutoPolicy::new(
                    AutoPolicyConfig::default(),
                    mock_reader,
                    k8s_client.clone(),
                ),
                rootcause: RootCauseEngine::new(
                    RootCauseConfig::default(),
                    mock_reader,
                    k8s_client.clone(),
                ),
                simulator: Simulator::new(
                    SimulatorConfig::default(),
                    mock_reader,
                    k8s_client.clone(),
                ),
                replay: ReplayEngine::new(ReplayConfig::default(), mock_reader, k8s_client.clone()),
            }
        };

        // Initialize standalone engines
        let chaos_engine = ChaosEngine::new(ChaosConfig::default(), k8s_client.clone());
        let canary_engine = CanaryEngine::new(CanaryConfig::default(), k8s_client.clone());
        let multicluster_engine =
            MultiClusterAutopilot::new(MultiClusterConfig::default(), k8s_client.clone());

        Ok(Self {
            hubble_client,
            endpoint_manager,
            k8s_client,
            flows: Vec::new(),
            endpoints: Vec::new(),
            selected_tab: 0,
            context,
            integrated_provider,
            enriched_connections: Vec::new(),
            modules,
            chaos_engine,
            canary_engine,
            multicluster_engine,
            last_healer_run: std::time::Instant::now(),
            last_autopolicy_update: std::time::Instant::now(),
            status_message: None,
            status_message_time: std::time::Instant::now(),
            simulator_view: SimulatorView::new(),
            replay_view: ReplayView::new(),
            chaos_view: ChaosView::new(),
            canary_view: CanaryView::new(),
            multicluster_view: MultiClusterView::new(),
            autopolicy_view: AutoPolicyView::new(),
            rootcause_view: RootCauseView::new(),
            selected_policy_index: 0,
            policy_detail_mode: false,
            policy_apply_confirmation: false,
            policy_rollback_confirmation: false,
            policy_batch_apply_confirmation: false,
            policy_batch_rollback_confirmation: false,
            applied_policies: std::collections::HashSet::new(),
            selected_fix_index: 0,
            fix_apply_confirmation: false,
            selected_flow_index: 0,
            show_packet_explanation: false,
            packet_explainer: tokio::sync::Mutex::new(
                crate::modules::packet_explainer::PacketExplainer::new(),
            ),
            canary_pending_action: None,
            show_help: false,
        })
    }

    pub(crate) fn set_status_message(&mut self, message: &str) {
        self.status_message = Some(message.to_string());
        self.status_message_time = std::time::Instant::now();
    }

    pub(crate) fn save_policies(
        &self,
        policies: &[crate::modules::autopolicy::GeneratedPolicy],
    ) -> Result<usize> {
        use std::fs;
        use std::io::Write;

        // Create policies directory if it doesn't exist
        fs::create_dir_all("./policies")?;

        let mut count = 0;
        for policy in policies {
            let filename = format!("./policies/{}.yaml", policy.name);
            let mut file = fs::File::create(&filename)?;
            file.write_all(policy.yaml.as_bytes())?;
            count += 1;
            tracing::debug!("Saved policy to {}", filename);
        }

        Ok(count)
    }

    pub(crate) async fn apply_policy_kubectl(
        &self,
        _policy_name: &str,
        policy_yaml: &str,
    ) -> Result<()> {
        use std::io::Write;
        use tokio::process::Command;

        // Write policy to a secure temporary file
        let mut temp = tempfile::NamedTempFile::with_suffix(".yaml")?;
        temp.write_all(policy_yaml.as_bytes())?;
        temp.flush()?;

        // Apply using kubectl
        let output = Command::new("kubectl")
            .arg("apply")
            .arg("-f")
            .arg(temp.path())
            .output()
            .await?;

        // temp file is automatically cleaned up on drop

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("kubectl apply failed: {}", stderr))
        }
    }

    pub(crate) async fn rollback_policy_kubectl(
        &self,
        policy_name: &str,
        namespace: &str,
    ) -> Result<()> {
        use tokio::process::Command;

        // Delete the CiliumNetworkPolicy using kubectl
        let output = Command::new("kubectl")
            .args([
                "delete",
                "ciliumnetworkpolicy",
                policy_name,
                "-n",
                namespace,
            ])
            .output()
            .await?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Check if error is "not found" - treat as success (already deleted)
            if stderr.contains("NotFound") || stderr.contains("not found") {
                Ok(())
            } else {
                Err(anyhow::anyhow!("kubectl delete failed: {}", stderr))
            }
        }
    }

    /// Get the policy name and YAML for a root-cause fix.
    /// The number of arms here must match `handlers::FIX_COUNT`.
    pub(crate) fn get_fix_policy(&self, fix_index: usize) -> (String, String) {
        // Compile-time assertion: if FIX_COUNT changes, this will fail to compile
        const _: () = assert!(
            super::handlers::FIX_COUNT == 4,
            "FIX_COUNT and get_fix_policy arms must match"
        );
        match fix_index {
            0 => {
                // Allow port 8080 policy
                let policy_name = "rootcause-fix-allow-8080".to_string();
                let policy_yaml = r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: rootcause-fix-allow-8080
  namespace: default
spec:
  endpointSelector:
    matchLabels:
      app: frontend
  egress:
  - toEndpoints:
    - matchLabels:
        app: backend
    toPorts:
    - ports:
      - port: "8080"
        protocol: TCP"#
                    .to_string();
                (policy_name, policy_yaml)
            }
            1 => {
                // DNS egress policy
                let policy_name = "rootcause-fix-dns-egress".to_string();
                let policy_yaml = r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: rootcause-fix-dns-egress
  namespace: default
spec:
  endpointSelector: {}
  egress:
  - toEndpoints:
    - matchLabels:
        k8s:io.kubernetes.pod.namespace: kube-system
        k8s-app: kube-dns
    toPorts:
    - ports:
      - port: "53"
        protocol: UDP
      rules:
        dns:
        - matchPattern: "*"
  - toFQDNs:
    - matchPattern: "*"
    toPorts:
    - ports:
      - port: "53"
        protocol: UDP"#
                    .to_string();
                (policy_name, policy_yaml)
            }
            2 => {
                // MTU adjustment (ConfigMap update - simulated as policy for demo)
                let policy_name = "rootcause-fix-mtu-note".to_string();
                let policy_yaml = r#"# MTU Adjustment
#
# This would normally update the ConfigMap:
# kubectl -n kube-system patch configmap/cilium-config \
#   --type merge -p '{"data":{"tunnel-protocol":"disabled"}}'
#
# For demonstration, this creates a note policy:
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: rootcause-fix-mtu-note
  namespace: default
  annotations:
    description: "MTU issue detected - consider adjusting CNI MTU settings"
spec:
  endpointSelector: {}
  egress:
  - {}  # Allow all egress — MTU issues affect all destinations"#
                    .to_string();
                (policy_name, policy_yaml)
            }
            3 => {
                // DB access policy
                let policy_name = "rootcause-fix-db-access".to_string();
                let policy_yaml = r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: rootcause-fix-db-access
  namespace: default
spec:
  endpointSelector:
    matchLabels:
      app: api
  egress:
  - toEndpoints:
    - matchLabels:
        app: db
    toPorts:
    - ports:
      - port: "5432"
        protocol: TCP"#
                    .to_string();
                (policy_name, policy_yaml)
            }
            _ => ("unknown-fix".to_string(), "# Unknown fix".to_string()),
        }
    }
}
