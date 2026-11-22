// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// What-If Simulator Module
///
/// Simulates policy changes and predicts their impact before applying.
/// Enables risk-free policy testing by replaying historical traffic
/// through a simulated policy engine.
///
/// Features:
/// - Policy impact prediction
/// - Service dependency analysis
/// - Risk assessment scoring
/// - Rollback recommendations
/// - Safe testing environment
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use crate::ebpf::{ConntrackEntry, MapReader, PolicyDecision, PolicyVerdict};
use crate::kubernetes::K8sClient;

pub mod analyzer;
pub mod engine;
pub mod scorer;

/// Simulator configuration
#[derive(Debug, Clone)]
pub struct SimulatorConfig {
    /// Enable simulator
    pub enabled: bool,

    /// Number of historical flows to replay
    pub replay_flow_count: usize,

    /// Time window for historical data (seconds)
    pub history_window_secs: u64,

    /// Enable dependency tracking
    pub track_dependencies: bool,

    /// Enable risk scoring
    pub risk_scoring: bool,

    /// Minimum confidence for predictions
    pub min_confidence: f32,
}

impl Default for SimulatorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            replay_flow_count: 1000,
            history_window_secs: 3600, // 1 hour
            track_dependencies: true,
            risk_scoring: true,
            min_confidence: 0.7,
        }
    }
}

/// Simulation scenario - what to test
#[derive(Debug, Clone)]
pub enum SimulationScenario {
    /// Test adding a new policy
    AddPolicy {
        policy_yaml: String,
        namespace: String,
    },

    /// Test removing an existing policy
    RemovePolicy {
        policy_name: String,
        namespace: String,
    },

    /// Test modifying a policy
    ModifyPolicy {
        policy_name: String,
        namespace: String,
        new_yaml: String,
    },

    /// Test blocking specific traffic
    BlockTraffic {
        from_labels: HashMap<String, String>,
        to_labels: HashMap<String, String>,
        port: Option<u16>,
        protocol: Option<String>,
    },

    /// Test allowing specific traffic
    AllowTraffic {
        from_labels: HashMap<String, String>,
        to_labels: HashMap<String, String>,
        port: u16,
        protocol: String,
    },

    /// Test blocking external IP
    BlockExternalIP { ip: IpAddr },

    /// Test default-deny for namespace
    DefaultDeny { namespace: String },
}

/// Simulation result
#[derive(Debug, Clone)]
pub struct SimulationResult {
    /// The scenario that was tested
    pub scenario: SimulationScenario,

    /// Impact analysis
    pub impact: ImpactAnalysis,

    /// Risk assessment
    pub risk: RiskAssessment,

    /// Affected resources
    pub affected: AffectedResources,

    /// Recommendations
    pub recommendations: Vec<String>,

    /// Confidence in prediction (0.0 to 1.0)
    pub confidence: f32,

    /// Detailed breakdown
    pub details: SimulationDetails,
}

/// Impact analysis
#[derive(Debug, Clone)]
pub struct ImpactAnalysis {
    /// Total flows analyzed
    pub total_flows: usize,

    /// Flows that would be blocked
    pub blocked_flows: usize,

    /// Flows that would be allowed
    pub allowed_flows: usize,

    /// Flows with changed behavior
    pub changed_flows: usize,

    /// Services that would be impacted
    pub impacted_services: Vec<String>,

    /// Critical services affected
    pub critical_services: Vec<String>,
}

/// Risk assessment
#[derive(Debug, Clone)]
pub struct RiskAssessment {
    /// Overall risk level
    pub level: RiskLevel,

    /// Risk score (0-10)
    pub score: u8,

    /// Risk factors
    pub factors: Vec<RiskFactor>,

    /// Is this safe to apply?
    pub safe_to_apply: bool,

    /// Reasons why unsafe
    pub unsafe_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            RiskLevel::Low => "Low",
            RiskLevel::Medium => "Medium",
            RiskLevel::High => "High",
            RiskLevel::Critical => "Critical",
        }
    }

    pub fn from_score(score: u8) -> Self {
        match score {
            0..=2 => RiskLevel::Low,
            3..=5 => RiskLevel::Medium,
            6..=8 => RiskLevel::High,
            _ => RiskLevel::Critical,
        }
    }
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Risk factor
#[derive(Debug, Clone)]
pub struct RiskFactor {
    pub category: RiskCategory,
    pub description: String,
    pub severity: u8, // 1-10
}

#[derive(Debug, Clone)]
pub enum RiskCategory {
    ServiceAvailability,
    DataPath,
    Security,
    Compliance,
    Performance,
}

/// Affected resources
#[derive(Debug, Clone)]
pub struct AffectedResources {
    /// Affected namespaces
    pub namespaces: Vec<String>,

    /// Affected pods (by identity)
    pub pod_identities: Vec<u32>,

    /// Affected services
    pub services: Vec<ServiceImpact>,

    /// Affected endpoints
    pub endpoints: Vec<EndpointImpact>,

    /// Dependencies broken
    pub broken_dependencies: Vec<Dependency>,
}

/// Service impact
#[derive(Debug, Clone)]
pub struct ServiceImpact {
    pub name: String,
    pub namespace: String,
    pub impact_type: ImpactType,
    pub affected_ports: Vec<u16>,
    pub client_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImpactType {
    FullyBlocked,      // All traffic blocked
    PartiallyBlocked,  // Some ports/clients blocked
    Unaffected,        // No impact
    PerformanceImpact, // Still works but slower
}

/// Endpoint impact
#[derive(Debug, Clone)]
pub struct EndpointImpact {
    pub identity: u32,
    pub namespace: String,
    pub labels: HashMap<String, String>,
    pub blocked_egress: Vec<EgressBlocked>,
    pub blocked_ingress: Vec<IngressBlocked>,
}

#[derive(Debug, Clone)]
pub struct EgressBlocked {
    pub to_identity: u32,
    pub port: u16,
    pub protocol: u8,
    pub flow_count: usize,
}

#[derive(Debug, Clone)]
pub struct IngressBlocked {
    pub from_identity: u32,
    pub port: u16,
    pub protocol: u8,
    pub flow_count: usize,
}

/// Dependency between services
#[derive(Debug, Clone)]
pub struct Dependency {
    pub from_service: String,
    pub to_service: String,
    pub port: u16,
    pub protocol: String,
    pub criticality: DependencyCriticality,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DependencyCriticality {
    Critical,  // Service won't work without it
    Important, // Degraded functionality
    Optional,  // Nice to have
}

/// Simulation details
#[derive(Debug, Clone)]
pub struct SimulationDetails {
    /// Flow-by-flow breakdown
    pub flow_results: Vec<FlowSimulationResult>,

    /// Policy evaluation trace
    pub policy_trace: Vec<String>,

    /// Timing information
    pub simulation_time_ms: u64,
}

/// Result for a single flow
#[derive(Debug, Clone)]
pub struct FlowSimulationResult {
    pub src_identity: u32,
    pub dst_identity: u32,
    pub port: u16,
    pub protocol: u8,
    pub before: PolicyVerdict,
    pub after: PolicyVerdict,
    pub changed: bool,
}

/// What-If Simulator Engine
pub struct Simulator<M: MapReader> {
    config: SimulatorConfig,
    ebpf_reader: M,
    k8s_client: K8sClient,

    /// Historical flows for replay
    flow_history: Vec<HistoricalFlow>,

    /// Current policy state
    current_policies: Vec<PolicyDecision>,
}

/// Historical flow record
#[derive(Debug, Clone)]
pub struct HistoricalFlow {
    pub src_identity: u32,
    pub dst_identity: u32,
    pub src_ip: IpAddr,
    pub dst_ip: IpAddr,
    pub port: u16,
    pub protocol: u8,
    pub timestamp: u64,
    pub verdict: PolicyVerdict,
}

impl<M: MapReader> Simulator<M> {
    pub fn new(config: SimulatorConfig, ebpf_reader: M, k8s_client: K8sClient) -> Self {
        Self {
            config,
            ebpf_reader,
            k8s_client,
            flow_history: Vec::new(),
            current_policies: Vec::new(),
        }
    }

    /// Load historical flows for simulation
    pub async fn load_history(&mut self) -> Result<usize> {
        // Read connection tracking to get recent flows
        let conntrack = self.ebpf_reader.read_conntrack_map()?;

        // Pre-read IPCache once for all identity resolutions
        let ipcache = self.ebpf_reader.read_ipcache_map().unwrap_or_default();

        // Convert to historical flows
        self.flow_history = conntrack
            .iter()
            .take(self.config.replay_flow_count)
            .map(|ct| self.conntrack_to_flow(ct, &ipcache))
            .collect::<Result<Vec<_>>>()?;

        Ok(self.flow_history.len())
    }

    /// Resolve an IP to its identity via a pre-read IPCache
    fn resolve_identity(&self, ip: &str, ipcache: &[crate::ebpf::IPCacheEntry]) -> u32 {
        if let Some(entry) = ipcache.iter().find(|e| e.ip == ip) {
            return entry.identity;
        }
        0
    }

    /// Convert conntrack entry to historical flow
    fn conntrack_to_flow(
        &self,
        ct: &ConntrackEntry,
        ipcache: &[crate::ebpf::IPCacheEntry],
    ) -> Result<HistoricalFlow> {
        let src_ip: IpAddr = ct
            .src_ip
            .parse()
            .unwrap_or_else(|_| IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)));
        let dst_ip: IpAddr = ct
            .dst_ip
            .parse()
            .unwrap_or_else(|_| IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)));

        Ok(HistoricalFlow {
            src_identity: self.resolve_identity(&ct.src_ip, ipcache),
            dst_identity: self.resolve_identity(&ct.dst_ip, ipcache),
            src_ip,
            dst_ip,
            port: ct.dst_port,
            protocol: ct.protocol,
            timestamp: ct.last_seen,
            verdict: PolicyVerdict::Allow, // Assume allowed if in conntrack
        })
    }

    /// Simulate a scenario
    pub async fn simulate(&mut self, scenario: SimulationScenario) -> Result<SimulationResult> {
        use analyzer::ImpactAnalyzer;
        use engine::SimulationEngine;
        use scorer::RiskScorer;

        let start = std::time::Instant::now();

        // Load current policies
        self.current_policies = self.ebpf_reader.read_policy_map()?;

        // Ensure we have flow history
        if self.flow_history.is_empty() {
            self.load_history().await?;
        }

        // Create simulation engine with modified policies
        let mut engine = SimulationEngine::new(self.current_policies.clone());
        engine.apply_scenario(&scenario)?;

        // Replay all flows through simulator
        let mut flow_results = Vec::new();
        for flow in &self.flow_history {
            let before = self.evaluate_current_policy(flow);
            let after = engine.evaluate_flow(flow);

            flow_results.push(FlowSimulationResult {
                src_identity: flow.src_identity,
                dst_identity: flow.dst_identity,
                port: flow.port,
                protocol: flow.protocol,
                before,
                after,
                changed: before != after,
            });
        }

        // Analyze impact
        let analyzer = ImpactAnalyzer::new(&self.k8s_client, &self.ebpf_reader);
        let impact = analyzer.analyze(&flow_results).await?;
        let affected = analyzer.find_affected_resources(&flow_results).await?;

        // Score risk
        let scorer = RiskScorer::new(self.config.min_confidence);
        let risk = scorer.assess_risk(&impact, &affected, &scenario);

        // Generate recommendations
        let recommendations = self.generate_recommendations(&impact, &risk, &scenario);

        // Calculate confidence
        let confidence = self.calculate_confidence(&flow_results);

        let simulation_time_ms = start.elapsed().as_millis() as u64;

        tracing::info!(
            total_flows = impact.total_flows,
            changed_flows = impact.changed_flows,
            blocked_flows = impact.blocked_flows,
            risk_score = risk.score,
            simulation_time_ms,
            "Simulation completed"
        );

        Ok(SimulationResult {
            scenario,
            impact,
            risk,
            affected,
            recommendations,
            confidence,
            details: SimulationDetails {
                flow_results,
                policy_trace: engine.get_trace(),
                simulation_time_ms,
            },
        })
    }

    /// Evaluate flow against current policies
    fn evaluate_current_policy(&self, flow: &HistoricalFlow) -> PolicyVerdict {
        // Find matching policy decision
        for decision in &self.current_policies {
            if decision.src_identity == flow.src_identity
                && decision.dst_identity == flow.dst_identity
                && decision.port == flow.port
                && decision.protocol == flow.protocol
            {
                return decision.verdict;
            }
        }

        // Default deny if no policy found
        PolicyVerdict::Deny
    }

    /// Generate recommendations based on impact
    fn generate_recommendations(
        &self,
        impact: &ImpactAnalysis,
        risk: &RiskAssessment,
        scenario: &SimulationScenario,
    ) -> Vec<String> {
        let mut recommendations = Vec::new();

        if risk.score >= 8 {
            recommendations
                .push("⚠️  HIGH RISK: Do not apply this change in production".to_string());
        }

        if !impact.critical_services.is_empty() {
            recommendations.push(format!(
                "Critical services affected: {}. Consider gradual rollout.",
                impact.critical_services.join(", ")
            ));
        }

        if impact.blocked_flows > impact.total_flows / 2 {
            recommendations.push(
                "More than 50% of flows would be blocked. Review policy carefully.".to_string(),
            );
        }

        match scenario {
            SimulationScenario::DefaultDeny { .. } => {
                recommendations.push(
                    "Ensure all required egress is explicitly allowed before applying default-deny"
                        .to_string(),
                );
            }
            SimulationScenario::BlockExternalIP { .. } => {
                recommendations
                    .push("Verify DNS and external dependencies before blocking IPs".to_string());
            }
            _ => {}
        }

        if recommendations.is_empty() {
            recommendations.push("✅ Change appears safe to apply".to_string());
        }

        recommendations
    }

    /// Calculate confidence in simulation
    fn calculate_confidence(&self, flow_results: &[FlowSimulationResult]) -> f32 {
        if flow_results.is_empty() {
            return 0.0;
        }

        // Confidence based on:
        // 1. Amount of historical data
        let data_score =
            (flow_results.len() as f32 / self.config.replay_flow_count as f32).min(1.0);

        // 2. Coverage of identities
        let unique_src: HashSet<_> = flow_results.iter().map(|f| f.src_identity).collect();
        let unique_dst: HashSet<_> = flow_results.iter().map(|f| f.dst_identity).collect();
        let coverage_score = ((unique_src.len() + unique_dst.len()) as f32 / 20.0).min(1.0);

        // Combined
        (data_score * 0.6 + coverage_score * 0.4).min(1.0)
    }

    /// Get simulation statistics
    pub fn stats(&self) -> SimulatorStats {
        SimulatorStats {
            flow_history_size: self.flow_history.len(),
            current_policies: self.current_policies.len(),
            history_window_secs: self.config.history_window_secs,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SimulatorStats {
    pub flow_history_size: usize,
    pub current_policies: usize,
    pub history_window_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ebpf::MockMapReader;

    #[ignore = "needs a Kubernetes cluster"]
    #[tokio::test]
    async fn test_simulator_creation() {
        let config = SimulatorConfig::default();
        let reader = MockMapReader;
        let k8s_client = K8sClient::new().await.unwrap();

        let simulator = Simulator::new(config, reader, k8s_client);
        assert_eq!(simulator.flow_history.len(), 0);
    }

    #[test]
    fn test_risk_level_from_score() {
        assert_eq!(RiskLevel::from_score(0), RiskLevel::Low);
        assert_eq!(RiskLevel::from_score(3), RiskLevel::Medium);
        assert_eq!(RiskLevel::from_score(7), RiskLevel::High);
        assert_eq!(RiskLevel::from_score(10), RiskLevel::Critical);
    }

    #[test]
    fn test_impact_type() {
        let impact = ImpactType::FullyBlocked;
        assert_eq!(impact, ImpactType::FullyBlocked);
    }
}
