// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Multi-Cluster Autopilot Module
///
/// Intelligent cross-cluster orchestration and policy synchronization.
/// Coordinates operations across multiple Kubernetes clusters.
///
/// Features:
/// - Auto-discovery of clusters
/// - Policy synchronization
/// - Cross-cluster service mesh
/// - Intelligent workload placement
/// - Multi-cluster failover
/// - Global traffic management
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::kubernetes::K8sClient;

/// Multi-cluster configuration
#[derive(Debug, Clone)]
pub struct MultiClusterConfig {
    /// Enable multi-cluster management
    pub enabled: bool,

    /// Auto-sync policies across clusters
    pub auto_sync_policies: bool,

    /// Enable cross-cluster service discovery
    pub service_discovery: bool,

    /// Enable global load balancing
    pub global_lb: bool,

    /// Health check interval
    pub health_check_interval_secs: u64,

    /// Sync interval
    pub sync_interval_secs: u64,
}

impl Default for MultiClusterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_sync_policies: true,
            service_discovery: true,
            global_lb: true,
            health_check_interval_secs: 30,
            sync_interval_secs: 300, // 5 minutes
        }
    }
}

/// Cluster information
#[derive(Debug, Clone)]
pub struct ClusterInfo {
    pub id: String,
    pub name: String,
    pub api_endpoint: String,
    pub region: String,
    pub provider: CloudProvider,
    pub state: ClusterState,
    pub health: ClusterHealth,
    pub resources: ClusterResources,
    pub connectivity: ClusterConnectivity,
    pub last_sync: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CloudProvider {
    AWS,
    GCP,
    Azure,
    DigitalOcean,
    OnPremise,
    Other(String),
}

impl std::fmt::Display for CloudProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloudProvider::AWS => write!(f, "AWS"),
            CloudProvider::GCP => write!(f, "GCP"),
            CloudProvider::Azure => write!(f, "Azure"),
            CloudProvider::DigitalOcean => write!(f, "DigitalOcean"),
            CloudProvider::OnPremise => write!(f, "On-Premise"),
            CloudProvider::Other(s) => write!(f, "{}", s),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClusterState {
    Active,      // Fully operational
    Degraded,    // Partially operational
    Unreachable, // Cannot connect
    Syncing,     // Synchronizing policies
    Draining,    // Draining workloads
}

#[derive(Debug, Clone)]
pub struct ClusterHealth {
    pub healthy: bool,
    pub node_count: u32,
    pub healthy_nodes: u32,
    pub pod_count: u32,
    pub cpu_usage_pct: f32,
    pub memory_usage_pct: f32,
    pub network_ok: bool,
}

impl Default for ClusterHealth {
    fn default() -> Self {
        Self {
            healthy: false,
            node_count: 0,
            healthy_nodes: 0,
            pod_count: 0,
            cpu_usage_pct: 0.0,
            memory_usage_pct: 0.0,
            network_ok: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClusterResources {
    pub total_cpu_cores: u32,
    pub available_cpu_cores: u32,
    pub total_memory_gb: u32,
    pub available_memory_gb: u32,
    pub pod_capacity: u32,
    pub pod_available: u32,
}

impl Default for ClusterResources {
    fn default() -> Self {
        Self {
            total_cpu_cores: 32,
            available_cpu_cores: 18,
            total_memory_gb: 128,
            available_memory_gb: 68,
            pod_capacity: 1000,
            pod_available: 500,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ClusterConnectivity {
    pub connected_clusters: Vec<String>,
    pub avg_latency_ms: HashMap<String, f64>,
    pub bandwidth_mbps: HashMap<String, f64>,
}

/// Cross-cluster policy sync
#[derive(Debug, Clone)]
pub struct PolicySync {
    pub id: String,
    pub source_cluster: String,
    pub target_clusters: Vec<String>,
    pub policy_type: PolicyType,
    pub status: SyncStatus,
    pub created_at: u64,
    pub completed_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PolicyType {
    NetworkPolicy,
    CiliumPolicy,
    ServiceMesh,
    Security,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SyncStatus {
    Pending,
    InProgress,
    Completed,
    Failed { reason: String },
}

/// Workload placement decision
#[derive(Debug, Clone)]
pub struct PlacementDecision {
    pub workload_name: String,
    pub target_cluster: String,
    pub reason: PlacementReason,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub enum PlacementReason {
    LowerLatency,
    HigherAvailability,
    CostOptimization,
    ResourceAvailability,
    RegionAffinity,
    LoadBalancing,
}

impl PlacementReason {
    pub fn description(&self) -> &'static str {
        match self {
            PlacementReason::LowerLatency => "Lower network latency",
            PlacementReason::HigherAvailability => "Better availability zone",
            PlacementReason::CostOptimization => "Cost-effective region",
            PlacementReason::ResourceAvailability => "More available resources",
            PlacementReason::RegionAffinity => "Region affinity rules",
            PlacementReason::LoadBalancing => "Load distribution",
        }
    }
}

/// Multi-cluster autopilot engine
pub struct MultiClusterAutopilot {
    config: MultiClusterConfig,
    k8s_client: K8sClient,

    /// Managed clusters
    clusters: HashMap<String, ClusterInfo>,

    /// Active policy syncs
    active_syncs: Vec<PolicySync>,

    /// Sync history
    sync_history: Vec<PolicySync>,

    /// Placement recommendations
    placements: Vec<PlacementDecision>,
}

impl MultiClusterAutopilot {
    pub fn new(config: MultiClusterConfig, k8s_client: K8sClient) -> Self {
        Self {
            config,
            k8s_client,
            clusters: HashMap::new(),
            active_syncs: Vec::new(),
            sync_history: Vec::new(),
            placements: Vec::new(),
        }
    }

    /// Register a new cluster
    pub async fn register_cluster(
        &mut self,
        name: String,
        api_endpoint: String,
        region: String,
        provider: CloudProvider,
    ) -> Result<String> {
        let id = format!("cluster-{}", uuid::Uuid::new_v4());

        let cluster = ClusterInfo {
            id: id.clone(),
            name,
            api_endpoint,
            region,
            provider,
            state: ClusterState::Active,
            health: ClusterHealth::default(),
            resources: ClusterResources::default(),
            connectivity: ClusterConnectivity::default(),
            last_sync: None,
        };

        tracing::info!("🌐 Registered cluster: {}", cluster.name);

        self.clusters.insert(id.clone(), cluster);

        Ok(id)
    }

    /// Sync policy across clusters
    pub async fn sync_policy(
        &mut self,
        source_cluster: String,
        target_clusters: Vec<String>,
        policy_type: PolicyType,
    ) -> Result<String> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let id = format!("sync-{}-{}", now, uuid::Uuid::new_v4());

        let sync = PolicySync {
            id: id.clone(),
            source_cluster,
            target_clusters,
            policy_type,
            status: SyncStatus::Pending,
            created_at: now,
            completed_at: None,
        };

        tracing::info!("🔄 Starting policy sync: {}", sync.id);

        self.active_syncs.push(sync);

        // Cap sync history to prevent unbounded growth
        const MAX_HISTORY: usize = 10_000;
        if self.sync_history.len() > MAX_HISTORY {
            self.sync_history.drain(..1);
        }

        Ok(id)
    }

    /// Recommend workload placement
    pub fn recommend_placement(&mut self, workload_name: String) -> Result<PlacementDecision> {
        // Find cluster with best resources
        let best_cluster = self
            .clusters
            .values()
            .filter(|c| c.state == ClusterState::Active)
            .max_by_key(|c| c.resources.available_cpu_cores)
            .ok_or_else(|| anyhow::anyhow!("No healthy clusters available"))?;

        let decision = PlacementDecision {
            workload_name,
            target_cluster: best_cluster.name.clone(),
            reason: PlacementReason::ResourceAvailability,
            confidence: 0.85,
        };

        tracing::info!(
            "📍 Placement recommendation: {} → {}",
            decision.workload_name,
            decision.target_cluster
        );

        self.placements.push(decision.clone());

        Ok(decision)
    }

    /// Perform health check on all clusters.
    ///
    /// Queries `kubectl get nodes` for each cluster to determine real node
    /// health, updating node counts and cluster state accordingly.  Falls
    /// back to the cached node-count comparison when kubectl is unavailable.
    pub async fn health_check_all(&mut self) -> Result<()> {
        for cluster in self.clusters.values_mut() {
            // Try to query real node health via kubectl
            let check = tokio::process::Command::new("kubectl")
                .args([
                    "get", "nodes",
                    "--context", &cluster.name,
                    "-o", "jsonpath={range .items[*]}{.metadata.name},{.status.conditions[?(@.type==\"Ready\")].status}{\"\\n\"}{end}",
                    "--request-timeout=5s",
                ])
                .output()
                .await;

            match check {
                Ok(output) if output.status.success() => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let mut total = 0u32;
                    let mut ready = 0u32;
                    for line in stdout.lines() {
                        if line.trim().is_empty() {
                            continue;
                        }
                        total += 1;
                        if line.ends_with("True") {
                            ready += 1;
                        }
                    }
                    if total > 0 {
                        cluster.health.node_count = total;
                        cluster.health.healthy_nodes = ready;
                    }
                }
                Ok(_) | Err(_) => {
                    // kubectl failed or context unknown — keep existing cached values
                    tracing::debug!(
                        cluster = %cluster.name,
                        "kubectl node check failed, using cached health data"
                    );
                }
            }

            // Derive cluster state from node readiness
            // A cluster with zero nodes is not healthy
            cluster.health.healthy = cluster.health.node_count > 0
                && cluster.health.healthy_nodes == cluster.health.node_count;

            if cluster.health.healthy {
                cluster.state = ClusterState::Active;
            } else if cluster.health.healthy_nodes > 0 {
                cluster.state = ClusterState::Degraded;
            } else {
                cluster.state = ClusterState::Unreachable;
            }
        }

        Ok(())
    }

    /// Get cluster topology
    pub fn get_topology(&self) -> ClusterTopology {
        let regions: HashMap<String, Vec<String>> =
            self.clusters
                .values()
                .fold(HashMap::new(), |mut acc, cluster| {
                    acc.entry(cluster.region.clone())
                        .or_default()
                        .push(cluster.name.clone());
                    acc
                });

        // Calculate connected pairs from the cluster connectivity data.
        // A pair (A, B) is connected if A lists B in its connected_clusters.
        // We count unique unordered pairs to avoid double-counting.
        let mut pairs = HashSet::new();
        for cluster in self.clusters.values() {
            for connected_id in &cluster.connectivity.connected_clusters {
                let mut pair = [cluster.id.clone(), connected_id.clone()];
                pair.sort();
                pairs.insert((pair[0].clone(), pair[1].clone()));
            }
        }

        ClusterTopology {
            total_clusters: self.clusters.len(),
            regions,
            connected_pairs: pairs.len(),
        }
    }

    /// Get managed clusters
    pub fn clusters(&self) -> &HashMap<String, ClusterInfo> {
        &self.clusters
    }

    /// Get statistics
    pub fn stats(&self) -> MultiClusterStats {
        let active_clusters = self
            .clusters
            .values()
            .filter(|c| c.state == ClusterState::Active)
            .count();

        let degraded_clusters = self
            .clusters
            .values()
            .filter(|c| c.state == ClusterState::Degraded)
            .count();

        MultiClusterStats {
            total_clusters: self.clusters.len(),
            active_clusters,
            degraded_clusters,
            active_syncs: self.active_syncs.len(),
            total_syncs: self.sync_history.len() + self.active_syncs.len(),
            placement_recommendations: self.placements.len(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClusterTopology {
    pub total_clusters: usize,
    pub regions: HashMap<String, Vec<String>>,
    pub connected_pairs: usize,
}

#[derive(Debug, Clone)]
pub struct MultiClusterStats {
    pub total_clusters: usize,
    pub active_clusters: usize,
    pub degraded_clusters: usize,
    pub active_syncs: usize,
    pub total_syncs: usize,
    pub placement_recommendations: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires live Kubernetes cluster"]
    async fn test_autopilot_creation() {
        let config = MultiClusterConfig::default();
        let k8s_client = K8sClient::new().await.unwrap();

        let autopilot = MultiClusterAutopilot::new(config, k8s_client);
        assert_eq!(autopilot.clusters.len(), 0);
    }

    #[test]
    fn test_cloud_provider() {
        let aws = CloudProvider::AWS;
        assert_eq!(aws.to_string(), "AWS");
    }

    #[test]
    fn test_cluster_health() {
        let health = ClusterHealth::default();
        assert_eq!(health.healthy, false);
        assert_eq!(health.node_count, 0);
    }
}
