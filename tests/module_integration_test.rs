// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Integration tests for module interactions.
///
/// Tests that the major modules work correctly independently and with
/// mock data: AutoPolicy, RootCause, Simulator, Replay, Chaos,
/// Canary, MultiCluster, and PacketExplainer.
use paqtra::ebpf::MockMapReader;
use paqtra::kubernetes::K8sClient;
use paqtra::modules::autopolicy::{AutoPolicy, AutoPolicyConfig, LabelSet, Protocol};
use paqtra::modules::canary::{CanaryConfig, CanaryEngine, CanaryMetrics, TrafficSplit};
use paqtra::modules::chaos::{ChaosConfig, ChaosEngine, ChaosExperiment, ChaosSeverity};
use paqtra::modules::multicluster::{
    CloudProvider, ClusterHealth, ClusterResources, ClusterState, MultiClusterAutopilot,
    MultiClusterConfig,
};
use paqtra::modules::packet_explainer::PacketExplainer;
use paqtra::modules::replay::{ReplayConfig, ReplayEngine, ReplayFilter};
use paqtra::modules::rootcause::{DropReason, RootCauseConfig, RootCauseEngine};
use paqtra::modules::simulator::{Simulator, SimulatorConfig};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Helper: construct a mock K8sClient for tests.
// K8sClient::mock() requires a Tokio runtime because the kube::Client uses
// tower buffer services internally. Tests that call this must be async.
// ---------------------------------------------------------------------------

fn mock_k8s_client() -> K8sClient {
    K8sClient::mock()
}

// ---------------------------------------------------------------------------
// Tests: AutoPolicy learner can process mock connections
// ---------------------------------------------------------------------------

// test_autopolicy_initial_state_is_not_started: removed — covered by
// tests/autopolicy_integration_test.rs::test_autopolicy_initial_state_not_started

#[tokio::test]
async fn test_autopolicy_observations_initially_empty() {
    let config = AutoPolicyConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let ap = AutoPolicy::new(config, reader, k8s);
    assert!(
        ap.observations().is_empty(),
        "No observations should exist before learning"
    );
}

#[tokio::test]
async fn test_autopolicy_policies_initially_empty() {
    let config = AutoPolicyConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let ap = AutoPolicy::new(config, reader, k8s);
    assert!(
        ap.policies().is_empty(),
        "No policies should exist before generation"
    );
}

#[tokio::test]
async fn test_autopolicy_stats_initial_values() {
    let config = AutoPolicyConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let ap = AutoPolicy::new(config, reader, k8s);
    let stats = ap.stats();
    assert_eq!(stats.total_observations, 0);
    assert_eq!(stats.unique_patterns, 0);
    assert_eq!(stats.policies_generated, 0);
    assert_eq!(stats.avg_confidence, 0.0);
}

#[test]
fn test_autopolicy_config_defaults() {
    let config = AutoPolicyConfig::default();
    assert!(config.enabled);
    assert!(config.audit_mode, "Default should be audit mode for safety");
    assert!(!config.auto_apply, "Auto-apply should be off by default");
    assert!(
        !config.auto_generate,
        "Auto-generate should be off by default"
    );
    assert!(
        config.min_observations >= 1,
        "Must require at least 1 observation"
    );
}

#[test]
fn test_protocol_from_number() {
    assert_eq!(Protocol::from_number(6), Protocol::TCP);
    assert_eq!(Protocol::from_number(17), Protocol::UDP);
    assert_eq!(Protocol::from_number(1), Protocol::ICMP);
    assert_eq!(Protocol::from_number(99), Protocol::Other(99));
    assert_eq!(Protocol::from_number(0), Protocol::Other(0));
}

#[test]
fn test_protocol_to_string() {
    assert_eq!(Protocol::TCP.to_string(), "TCP");
    assert_eq!(Protocol::UDP.to_string(), "UDP");
    assert_eq!(Protocol::ICMP.to_string(), "ICMP");
    assert_eq!(Protocol::Other(42).to_string(), "42");
}

#[test]
fn test_label_set_operations() {
    let mut labels = HashMap::new();
    labels.insert("app".to_string(), "web".to_string());
    labels.insert("tier".to_string(), "frontend".to_string());

    let ls = LabelSet::new(labels.clone());
    assert!(!ls.is_empty());
    assert_eq!(ls.get("app"), Some(&"web".to_string()));
    assert_eq!(ls.get("tier"), Some(&"frontend".to_string()));
    assert_eq!(ls.get("nonexistent"), None);

    let roundtripped = ls.to_hashmap();
    assert_eq!(roundtripped, labels);
}

#[test]
fn test_label_set_empty() {
    let ls = LabelSet::new(HashMap::new());
    assert!(ls.is_empty());
    assert_eq!(ls.get("anything"), None);
}

#[test]
fn test_label_set_equality() {
    let mut labels1 = HashMap::new();
    labels1.insert("a".to_string(), "1".to_string());
    labels1.insert("b".to_string(), "2".to_string());

    // Same labels, different insertion order
    let mut labels2 = HashMap::new();
    labels2.insert("b".to_string(), "2".to_string());
    labels2.insert("a".to_string(), "1".to_string());

    let ls1 = LabelSet::new(labels1);
    let ls2 = LabelSet::new(labels2);
    assert_eq!(
        ls1, ls2,
        "LabelSets with same entries should be equal regardless of insertion order"
    );
}

// ---------------------------------------------------------------------------
// Tests: RootCause analyzer handles empty drop list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_rootcause_initial_drop_history_empty() {
    let config = RootCauseConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let engine = RootCauseEngine::new(config, reader, k8s);
    let stats = engine.get_stats();
    assert_eq!(stats.total_drops, 0, "Initial drop history must be empty");
    assert!(stats.by_reason.is_empty());
    assert!(stats.by_namespace.is_empty());
    assert!(stats.top_patterns.is_empty());
}

#[test]
fn test_rootcause_config_defaults() {
    let config = RootCauseConfig::default();
    assert!(config.enabled);
    assert!(config.auto_correlate);
    assert!(config.pattern_analysis);
    assert!(
        config.analysis_window_secs > 0,
        "Analysis window must be positive"
    );
    assert!(
        config.min_drop_count >= 1,
        "Min drop count must be at least 1"
    );
}

#[test]
fn test_drop_reason_from_ebpf_covers_all_known_types() {
    // Verify that from_ebpf works for all well-known DropReasonType variants
    use paqtra::ebpf::DropReasonType;

    let known_types = vec![
        DropReasonType::PolicyDenied,
        DropReasonType::InvalidPacket,
        DropReasonType::NoRoute,
        DropReasonType::UnknownL4Protocol,
        DropReasonType::FragmentationNeeded,
        DropReasonType::CTMapFull,
        DropReasonType::NATMapFull,
        DropReasonType::InvalidSourceIP,
        DropReasonType::InvalidDestIP,
        DropReasonType::UnsupportedL3Protocol,
        DropReasonType::MissedTailCall,
        DropReasonType::ErrorWritingToPacket,
        DropReasonType::UnknownL4ICMPType,
        DropReasonType::UnknownICMPv6Type,
        DropReasonType::UnknownICMPv6Code,
        DropReasonType::ServiceBackendNotFound,
        DropReasonType::NoTunnelEndpoint,
        DropReasonType::HostUnreachable,
        DropReasonType::StaleOrUnroutable,
        DropReasonType::ConnectionTrackingInvalid,
        DropReasonType::AuthRequired,
        DropReasonType::NATNotNeeded,
        DropReasonType::IsClusterIP,
        DropReasonType::Other(42),
    ];

    for dt in known_types {
        // This must not panic
        let _dr = DropReason::from_ebpf(&dt);
    }
}

#[test]
fn test_drop_reason_to_string_nonempty() {
    let reasons = vec![
        DropReason::PolicyDenied,
        DropReason::InvalidSourceIP,
        DropReason::InvalidPacket,
        DropReason::CTStateMismatch,
        DropReason::PortNotAllowed,
        DropReason::UnknownL3Protocol,
        DropReason::UnknownL4Protocol,
        DropReason::UnsupportedL3Protocol,
        DropReason::NoMapping,
        DropReason::UnknownDestination,
        DropReason::LBError,
        DropReason::ServiceNotFound,
        DropReason::NoBackend,
        DropReason::FragNeeded,
        DropReason::TTLExceeded,
        DropReason::Other(99),
    ];

    for reason in reasons {
        let s = reason.to_string();
        assert!(
            !s.is_empty(),
            "DropReason::{:?}.to_string() must not be empty",
            reason
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: Simulator engine handles empty policy
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_simulator_initial_state() {
    let config = SimulatorConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let sim = Simulator::new(config, reader, k8s);
    let stats = sim.stats();
    assert_eq!(stats.flow_history_size, 0, "No flows loaded yet");
    assert_eq!(stats.current_policies, 0, "No policies loaded yet");
}

// test_simulator_config_defaults: removed — covered by
// tests/simulator_integration_test.rs::test_simulator_config_defaults

// test_risk_level_from_score: removed — covered by more granular tests in
// tests/simulator_integration_test.rs (test_risk_level_from_score_low/medium/high/critical)

// test_risk_level_ordering: removed — covered by
// tests/simulator_integration_test.rs::test_risk_level_ordering (with extra assertion)

// test_risk_level_to_string: removed — covered by
// tests/simulator_integration_test.rs::test_risk_level_to_string

// test_impact_type_equality: removed — covered by
// tests/simulator_integration_test.rs::test_impact_type_equality and test_impact_type_inequality

// ---------------------------------------------------------------------------
// Tests: Replay recorder can start/stop recording
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_replay_engine_initial_state() {
    let config = ReplayConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let engine = ReplayEngine::new(config, reader, k8s);
    let stats = engine.stats();
    assert_eq!(stats.total_recordings, 0);
    assert!(!stats.recording_in_progress);
    assert_eq!(stats.total_flows_recorded, 0);
}

// test_replay_config_defaults: removed — covered by more detailed version in
// tests/replay_integration_test.rs::test_replay_config_defaults

#[test]
fn test_replay_filter_default_is_unfiltered() {
    let filter = ReplayFilter::default();
    assert!(filter.namespaces.is_none());
    assert!(filter.src_labels.is_none());
    assert!(filter.dst_labels.is_none());
    assert!(filter.ports.is_none());
    assert!(filter.protocols.is_none());
    assert!(filter.verdicts.is_none());
    assert!(filter.limit.is_none());
}

// ---------------------------------------------------------------------------
// Tests: Chaos engine respects circuit breaker
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_chaos_engine_initial_state() {
    let config = ChaosConfig::default();
    let k8s = mock_k8s_client();

    let engine = ChaosEngine::new(config, k8s);
    let stats = engine.stats();
    assert_eq!(stats.active_experiments, 0);
    assert_eq!(stats.total_experiments, 0);
    assert!(!stats.circuit_breaker_active);
}

#[tokio::test]
async fn test_chaos_circuit_breaker_toggle() {
    let config = ChaosConfig::default();
    let k8s = mock_k8s_client();

    let mut engine = ChaosEngine::new(config, k8s);
    assert!(!engine.stats().circuit_breaker_active);

    engine.trigger_circuit_breaker("test reason");
    assert!(
        engine.stats().circuit_breaker_active,
        "Circuit breaker should be active after triggering"
    );

    engine.reset_circuit_breaker();
    assert!(
        !engine.stats().circuit_breaker_active,
        "Circuit breaker should be inactive after reset"
    );
}

#[test]
fn test_chaos_experiment_severity_levels() {
    // High severity: drop rate > 30%
    let high_drop = ChaosExperiment::PacketDrop { drop_rate: 0.5 };
    assert_eq!(high_drop.severity(), ChaosSeverity::High);

    // Medium severity: low drop rate
    let low_drop = ChaosExperiment::PacketDrop { drop_rate: 0.1 };
    assert_eq!(low_drop.severity(), ChaosSeverity::Medium);

    // High severity: latency > 1000ms
    let high_latency = ChaosExperiment::Latency {
        delay_ms: 2000,
        jitter_ms: 100,
    };
    assert_eq!(high_latency.severity(), ChaosSeverity::High);

    // Medium severity: low latency
    let low_latency = ChaosExperiment::Latency {
        delay_ms: 100,
        jitter_ms: 10,
    };
    assert_eq!(low_latency.severity(), ChaosSeverity::Medium);

    // Critical severity: DNS failure > 50%
    let dns_critical = ChaosExperiment::DNSFailure { failure_rate: 0.8 };
    assert_eq!(dns_critical.severity(), ChaosSeverity::Critical);
}

#[test]
fn test_chaos_experiment_names_nonempty() {
    let experiments = vec![
        ChaosExperiment::PacketDrop { drop_rate: 0.1 },
        ChaosExperiment::Latency {
            delay_ms: 100,
            jitter_ms: 10,
        },
        ChaosExperiment::Bandwidth { limit_mbps: 100 },
        ChaosExperiment::ConnectionKill { kill_rate: 0.1 },
        ChaosExperiment::DNSFailure { failure_rate: 0.1 },
        ChaosExperiment::PacketCorruption {
            corruption_rate: 0.01,
        },
        ChaosExperiment::PacketDuplication {
            duplication_rate: 0.05,
        },
    ];

    for exp in experiments {
        assert!(
            !exp.name().is_empty(),
            "Experiment name must not be empty for {:?}",
            exp
        );
        assert!(
            !exp.description().is_empty(),
            "Experiment description must not be empty for {:?}",
            exp
        );
    }
}

#[test]
fn test_chaos_config_safety_limits() {
    let config = ChaosConfig::default();
    assert!(
        config.max_drop_rate <= 1.0 && config.max_drop_rate > 0.0,
        "Max drop rate must be between 0 and 1"
    );
    assert!(config.max_latency_ms > 0, "Max latency must be positive");
    assert!(
        config.require_confirmation,
        "Default should require confirmation for safety"
    );
}

// ---------------------------------------------------------------------------
// Tests: Canary manager handles 0-request edge case
// ---------------------------------------------------------------------------

#[test]
fn test_canary_metrics_zero_requests() {
    let metrics = CanaryMetrics::default();

    // With zero requests, success rate should be 1.0 (no failures)
    assert_eq!(
        metrics.canary_success_rate(),
        1.0,
        "Success rate with 0 requests should be 1.0"
    );
    assert_eq!(
        metrics.canary_error_rate(),
        0.0,
        "Error rate with 0 requests should be 0.0"
    );
    assert_eq!(
        metrics.stable_success_rate(),
        1.0,
        "Stable success rate with 0 requests should be 1.0"
    );
}

#[test]
fn test_canary_metrics_with_data() {
    let mut metrics = CanaryMetrics::default();
    metrics.canary_requests = 200;
    metrics.canary_successes = 190;
    metrics.canary_errors = 10;

    assert!(
        (metrics.canary_success_rate() - 0.95).abs() < 0.001,
        "Expected 95% success rate"
    );
    assert!(
        (metrics.canary_error_rate() - 0.05).abs() < 0.001,
        "Expected 5% error rate"
    );
}

#[test]
fn test_traffic_split_construction() {
    let stable = TrafficSplit::new_stable();
    assert_eq!(stable.stable_pct, 100);
    assert_eq!(stable.canary_pct, 0);
    assert!(!stable.is_fully_promoted());

    let split = TrafficSplit::new_split(30);
    assert_eq!(split.stable_pct, 70);
    assert_eq!(split.canary_pct, 30);
    assert!(!split.is_fully_promoted());

    let full = TrafficSplit::new_split(100);
    assert_eq!(full.stable_pct, 0);
    assert_eq!(full.canary_pct, 100);
    assert!(full.is_fully_promoted());
}

#[test]
fn test_traffic_split_zero_canary() {
    let split = TrafficSplit::new_split(0);
    assert_eq!(split.stable_pct, 100);
    assert_eq!(split.canary_pct, 0);
    assert!(!split.is_fully_promoted());
}

#[tokio::test]
async fn test_canary_engine_initial_state() {
    let config = CanaryConfig::default();
    let k8s = mock_k8s_client();

    let engine = CanaryEngine::new(config, k8s);
    assert!(engine.active_canaries().is_empty());
    assert!(engine.history().is_empty());
    let stats = engine.stats();
    assert_eq!(stats.active_canaries, 0);
    assert_eq!(stats.total_deployments, 0);
    assert_eq!(stats.successful_promotions, 0);
    assert_eq!(stats.rollbacks, 0);
}

#[test]
fn test_canary_config_defaults() {
    let config = CanaryConfig::default();
    assert!(config.enabled);
    assert!(
        config.initial_traffic_pct > 0,
        "Must start with some canary traffic"
    );
    assert!(config.initial_traffic_pct <= 100);
    assert!(config.traffic_step_pct > 0);
    assert!(
        config.auto_promote_threshold > config.auto_rollback_threshold,
        "Promote threshold must be higher than rollback threshold"
    );
}

// ---------------------------------------------------------------------------
// Tests: MultiCluster manager handles empty cluster list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_multicluster_initial_state() {
    let config = MultiClusterConfig::default();
    let k8s = mock_k8s_client();

    let mc = MultiClusterAutopilot::new(config, k8s);
    assert!(
        mc.clusters().is_empty(),
        "No clusters should be registered initially"
    );
    let stats = mc.stats();
    assert_eq!(stats.total_clusters, 0);
    assert_eq!(stats.active_clusters, 0);
    assert_eq!(stats.degraded_clusters, 0);
    assert_eq!(stats.active_syncs, 0);
}

#[tokio::test]
async fn test_multicluster_topology_empty() {
    let config = MultiClusterConfig::default();
    let k8s = mock_k8s_client();

    let mc = MultiClusterAutopilot::new(config, k8s);
    let topology = mc.get_topology();
    assert_eq!(topology.total_clusters, 0);
    assert!(topology.regions.is_empty());
}

#[tokio::test]
async fn test_multicluster_placement_fails_with_no_clusters() {
    let config = MultiClusterConfig::default();
    let k8s = mock_k8s_client();

    let mut mc = MultiClusterAutopilot::new(config, k8s);
    let result = mc.recommend_placement("my-workload".to_string());
    assert!(
        result.is_err(),
        "Placement should fail when no clusters are available"
    );
}

#[test]
fn test_cloud_provider_to_string() {
    assert_eq!(CloudProvider::AWS.to_string(), "AWS");
    assert_eq!(CloudProvider::GCP.to_string(), "GCP");
    assert_eq!(CloudProvider::Azure.to_string(), "Azure");
    assert_eq!(CloudProvider::DigitalOcean.to_string(), "DigitalOcean");
    assert_eq!(CloudProvider::OnPremise.to_string(), "On-Premise");
    assert_eq!(
        CloudProvider::Other("Custom".to_string()).to_string(),
        "Custom"
    );
}

#[test]
fn test_cluster_state_equality() {
    assert_eq!(ClusterState::Active, ClusterState::Active);
    assert_eq!(ClusterState::Degraded, ClusterState::Degraded);
    assert_ne!(ClusterState::Active, ClusterState::Degraded);
    assert_ne!(ClusterState::Active, ClusterState::Unreachable);
}

#[test]
fn test_cluster_health_default() {
    let health = ClusterHealth::default();
    assert!(!health.healthy);
    assert!(!health.network_ok);
    assert_eq!(health.node_count, 0);
    assert_eq!(health.healthy_nodes, 0);
    assert_eq!(health.pod_count, 0);
}

#[test]
fn test_cluster_resources_default() {
    let resources = ClusterResources::default();
    assert!(resources.available_cpu_cores <= resources.total_cpu_cores);
    assert!(resources.available_memory_gb <= resources.total_memory_gb);
    assert!(resources.pod_available <= resources.pod_capacity);
}

#[test]
fn test_multicluster_config_defaults() {
    let config = MultiClusterConfig::default();
    assert!(config.enabled);
    assert!(config.auto_sync_policies);
    assert!(config.service_discovery);
    assert!(config.global_lb);
    assert!(config.health_check_interval_secs > 0);
    assert!(config.sync_interval_secs > 0);
}

// ---------------------------------------------------------------------------
// Tests: PacketExplainer cache size calculation
// ---------------------------------------------------------------------------

#[test]
fn test_packet_explainer_empty_cache() {
    let explainer = PacketExplainer::new();
    let stats = explainer.stats();
    assert_eq!(stats.total_explanations, 0);
    assert_eq!(stats.cache_size_kb, 0);
}

#[test]
fn test_packet_explainer_cache_grows() {
    let mut explainer = PacketExplainer::new();

    explainer
        .explain_packet("ns1", "pod1", "ns2", "pod2", 80, "TCP", "FORWARDED")
        .unwrap();
    assert_eq!(explainer.stats().total_explanations, 1);

    explainer
        .explain_packet("ns1", "pod1", "ns3", "pod3", 443, "TCP", "DROPPED")
        .unwrap();
    assert_eq!(explainer.stats().total_explanations, 2);
}

#[test]
fn test_packet_explainer_cache_deduplication() {
    let mut explainer = PacketExplainer::new();

    // Same parameters should be cached
    for _ in 0..10 {
        explainer
            .explain_packet("ns", "p", "ns2", "p2", 80, "TCP", "FORWARDED")
            .unwrap();
    }
    assert_eq!(
        explainer.stats().total_explanations,
        1,
        "Repeated same params should produce only 1 cache entry"
    );
}

#[test]
fn test_packet_explainer_dropped_packet_analysis() {
    let mut explainer = PacketExplainer::new();
    let explanation = explainer
        .explain_packet("frontend", "web", "backend", "api", 80, "TCP", "DROPPED")
        .unwrap();

    assert_eq!(explanation.verdict, "DROPPED");
    assert!(
        explanation.what_happened.contains("BLOCKED"),
        "Dropped packets should be described as blocked"
    );
    assert!(!explanation.troubleshooting_tips.is_empty());
    assert_eq!(
        explanation.severity, "Critical",
        "Port 80 drops are critical"
    );
}

#[test]
fn test_packet_explainer_forwarded_packet_analysis() {
    let mut explainer = PacketExplainer::new();
    let explanation = explainer
        .explain_packet("ns", "pod", "ns2", "pod2", 443, "TCP", "FORWARDED")
        .unwrap();

    assert_eq!(explanation.verdict, "FORWARDED");
    assert!(explanation.what_happened.contains("ALLOWED"));
    assert_eq!(explanation.severity, "Low");
}

#[test]
fn test_packet_explainer_cross_namespace_context() {
    let mut explainer = PacketExplainer::new();
    let explanation = explainer
        .explain_packet("frontend", "web", "backend", "api", 8080, "TCP", "DROPPED")
        .unwrap();

    assert!(
        explanation.policy_context.contains("Cross-namespace"),
        "Cross-namespace traffic should be identified"
    );
}

#[test]
fn test_packet_explainer_same_namespace_context() {
    let mut explainer = PacketExplainer::new();
    let explanation = explainer
        .explain_packet("app", "client", "app", "server", 8080, "TCP", "FORWARDED")
        .unwrap();

    assert!(
        explanation.policy_context.contains("same namespace"),
        "Same-namespace traffic should be identified"
    );
}

#[test]
fn test_packet_explainer_dns_analysis() {
    let mut explainer = PacketExplainer::new();
    let explanation = explainer
        .explain_packet("app", "pod", "kube-system", "coredns", 53, "UDP", "DROPPED")
        .unwrap();

    assert!(
        explanation.why_happened.contains("DNS"),
        "DNS drops on port 53 should mention DNS in analysis"
    );
}

#[test]
fn test_packet_explainer_security_analysis_ssh() {
    let mut explainer = PacketExplainer::new();
    let explanation = explainer
        .explain_packet("admin", "jump", "prod", "server", 22, "TCP", "FORWARDED")
        .unwrap();

    assert!(
        explanation.security_analysis.contains("SSH"),
        "Port 22 should trigger SSH security analysis"
    );
}
