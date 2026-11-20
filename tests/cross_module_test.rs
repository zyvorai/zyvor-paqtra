// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// Integration tests for cross-module interactions.
///
/// Validates that multiple modules can be instantiated with the same
/// mock infrastructure and that their data structures interoperate
/// correctly. Tests interactions between RootCause, Healer, AutoPolicy,
/// Simulator, and Replay modules.
use paqtra::ebpf::{DropReasonType, MapReader, MockMapReader, PolicyVerdict};
use paqtra::kubernetes::K8sClient;
use paqtra::modules::autopolicy::{AutoPolicy, AutoPolicyConfig, LearningState};
use paqtra::modules::healer::{HealerConfig, SelfHealer};
use paqtra::modules::replay::{ReplayConfig, ReplayEngine};
use paqtra::modules::rootcause::{
    DropReason as RootCauseDropReason, RootCauseConfig, RootCauseEngine,
};
use paqtra::modules::simulator::{RiskLevel, Simulator, SimulatorConfig};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn mock_k8s_client() -> K8sClient {
    K8sClient::mock()
}

// ---------------------------------------------------------------------------
// Tests: Multiple modules share the same MockMapReader type
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_modules_share_mock_reader() {
    // All modules should be constructible with MockMapReader.
    // This verifies the MapReader trait bound is satisfied uniformly.
    let k8s = mock_k8s_client();

    let healer = SelfHealer::new(HealerConfig::default(), MockMapReader, k8s.clone());
    let autopolicy = AutoPolicy::new(AutoPolicyConfig::default(), MockMapReader, k8s.clone());
    let rootcause = RootCauseEngine::new(RootCauseConfig::default(), MockMapReader, k8s.clone());
    let simulator = Simulator::new(SimulatorConfig::default(), MockMapReader, k8s.clone());
    let replay = ReplayEngine::new(ReplayConfig::default(), MockMapReader, k8s);

    // Verify each module starts in a clean state
    assert!(healer.problems().is_empty());
    assert_eq!(*autopolicy.state(), LearningState::NotStarted);
    assert_eq!(rootcause.get_stats().total_drops, 0);
    assert_eq!(simulator.stats().flow_history_size, 0);
    assert_eq!(replay.stats().total_recordings, 0);
}

// ---------------------------------------------------------------------------
// Tests: RootCause and Healer analyze from the same mock data source
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_rootcause_and_healer_both_process_mock_data() {
    let k8s = mock_k8s_client();
    let mut healer = SelfHealer::new(HealerConfig::default(), MockMapReader, k8s.clone());
    let rootcause = RootCauseEngine::new(RootCauseConfig::default(), MockMapReader, k8s);

    // Both modules read from MockMapReader, which returns realistic mock drops.
    let _healer_stats = healer.run().await.unwrap();
    // MockMapReader has PolicyDenied drops, so healer should detect problems

    // RootCause starts with zero stats until analyze_drops is called
    let stats = rootcause.get_stats();
    assert_eq!(stats.total_drops, 0);
}

#[tokio::test]
async fn test_rootcause_and_healer_consistent_run_on_mock() {
    let k8s = mock_k8s_client();

    // Run healer
    let mut healer = SelfHealer::new(HealerConfig::default(), MockMapReader, k8s.clone());
    let healer_stats = healer.run().await.unwrap();

    // Run rootcause analysis
    let mut rootcause = RootCauseEngine::new(RootCauseConfig::default(), MockMapReader, k8s);
    let _analyses = rootcause.analyze_drops().await.unwrap();

    // Both should process mock data consistently.
    // MockMapReader has PolicyDenied drops, so healer detects problems.
    assert!(
        healer_stats.problems_detected > 0,
        "Healer should detect problems from mock drops"
    );
    // RootCause processes mock drops without error. Mock timestamps are in the past,
    // so they fall outside the analysis window and don't appear in history.
}

// ---------------------------------------------------------------------------
// Tests: DropReason mapping consistency between eBPF and RootCause
// ---------------------------------------------------------------------------

#[test]
fn test_drop_reason_mapping_consistency() {
    // Verify that all major DropReasonType variants produce valid
    // RootCauseDropReason conversions without panicking.
    let reason_types = vec![
        DropReasonType::PolicyDenied,
        DropReasonType::InvalidPacket,
        DropReasonType::NoRoute,
        DropReasonType::UnknownL4Protocol,
        DropReasonType::FragmentationNeeded,
        DropReasonType::CTMapFull,
        DropReasonType::NATMapFull,
        DropReasonType::InvalidSourceIP,
        DropReasonType::InvalidDestIP,
        DropReasonType::ServiceBackendNotFound,
        DropReasonType::ConnectionTrackingInvalid,
        DropReasonType::AuthRequired,
        DropReasonType::Other(42),
    ];

    for rt in &reason_types {
        let converted = RootCauseDropReason::from_ebpf(rt);
        let display = converted.to_string();
        assert!(
            !display.is_empty(),
            "DropReason from {:?} should have a non-empty display string",
            rt
        );
    }
}

#[test]
fn test_policy_denied_maps_consistently() {
    // PolicyDenied in eBPF should map to PolicyDenied in RootCause
    let ebpf_reason = DropReasonType::PolicyDenied;
    let rootcause_reason = RootCauseDropReason::from_ebpf(&ebpf_reason);
    assert_eq!(rootcause_reason, RootCauseDropReason::PolicyDenied);
}

#[test]
fn test_auth_required_maps_to_policy_denied() {
    // AuthRequired in eBPF maps to PolicyDenied in RootCause
    let rootcause_reason = RootCauseDropReason::from_ebpf(&DropReasonType::AuthRequired);
    assert_eq!(rootcause_reason, RootCauseDropReason::PolicyDenied);
}

#[test]
fn test_fragmentation_maps_to_frag_needed() {
    let rootcause_reason = RootCauseDropReason::from_ebpf(&DropReasonType::FragmentationNeeded);
    assert_eq!(rootcause_reason, RootCauseDropReason::FragNeeded);
}

#[test]
fn test_service_backend_not_found_maps_to_no_backend() {
    let rootcause_reason = RootCauseDropReason::from_ebpf(&DropReasonType::ServiceBackendNotFound);
    assert_eq!(rootcause_reason, RootCauseDropReason::NoBackend);
}

#[test]
fn test_ct_map_full_maps_to_ct_state_mismatch() {
    let rootcause_reason = RootCauseDropReason::from_ebpf(&DropReasonType::CTMapFull);
    assert_eq!(rootcause_reason, RootCauseDropReason::CTStateMismatch);
}

// ---------------------------------------------------------------------------
// Tests: Modules can run concurrently on mock infrastructure
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_healer_and_autopolicy_run_independently() {
    let k8s = mock_k8s_client();

    let mut healer = SelfHealer::new(HealerConfig::default(), MockMapReader, k8s.clone());
    let mut autopolicy = AutoPolicy::new(AutoPolicyConfig::default(), MockMapReader, k8s);

    // Run healer
    let _healer_stats = healer.run().await.unwrap();

    // Start autopolicy learning
    autopolicy.start_learning().await.unwrap();
    assert!(matches!(autopolicy.state(), LearningState::Learning { .. }));
}

#[tokio::test]
async fn test_simulator_and_replay_run_independently() {
    let k8s = mock_k8s_client();

    let mut simulator = Simulator::new(SimulatorConfig::default(), MockMapReader, k8s.clone());
    let mut replay = ReplayEngine::new(ReplayConfig::default(), MockMapReader, k8s);

    // Load simulator history (MockMapReader now has conntrack entries)
    let _loaded = simulator.load_history().await.unwrap();

    // Start replay recording
    let rec_id = replay
        .start_recording("cross-test".to_string())
        .await
        .unwrap();
    assert!(!rec_id.is_empty());

    // Both modules operate independently
    assert!(replay.stats().recording_in_progress);
}

#[tokio::test]
async fn test_rootcause_and_autopolicy_independent() {
    let k8s = mock_k8s_client();

    let mut rootcause =
        RootCauseEngine::new(RootCauseConfig::default(), MockMapReader, k8s.clone());
    let mut autopolicy = AutoPolicy::new(AutoPolicyConfig::default(), MockMapReader, k8s);

    // Analyze drops via rootcause (MockMapReader has drop data)
    let _analyses = rootcause.analyze_drops().await.unwrap();

    // Start autopolicy learning
    autopolicy.start_learning().await.unwrap();
    assert!(matches!(autopolicy.state(), LearningState::Learning { .. }));

    // Update autopolicy (MockMapReader has conntrack data)
    let _stats = autopolicy.update().await.unwrap();
}

// ---------------------------------------------------------------------------
// Tests: Risk level from simulator applies to policy decisions
// ---------------------------------------------------------------------------

#[test]
fn test_risk_level_informs_policy_verdict_decisions() {
    // Low risk changes are safe to apply
    let low = RiskLevel::from_score(1);
    assert_eq!(low, RiskLevel::Low);

    // Critical risk changes should be blocked
    let critical = RiskLevel::from_score(10);
    assert_eq!(critical, RiskLevel::Critical);

    // These are the thresholds that would be used in cross-module decisions
    assert!(RiskLevel::Low < RiskLevel::Medium);
    assert!(RiskLevel::High < RiskLevel::Critical);
}

#[test]
fn test_risk_level_boundary_values() {
    // Exact boundaries
    assert_eq!(RiskLevel::from_score(2), RiskLevel::Low);
    assert_eq!(RiskLevel::from_score(3), RiskLevel::Medium);
    assert_eq!(RiskLevel::from_score(5), RiskLevel::Medium);
    assert_eq!(RiskLevel::from_score(6), RiskLevel::High);
    assert_eq!(RiskLevel::from_score(8), RiskLevel::High);
    assert_eq!(RiskLevel::from_score(9), RiskLevel::Critical);
}

// ---------------------------------------------------------------------------
// Tests: MockMapReader returns consistent data across modules
// ---------------------------------------------------------------------------

#[test]
fn test_mock_map_reader_consistent_across_reads() {
    let reader = MockMapReader;

    // Policy map should return the same data every time
    let policies1 = reader.read_policy_map().unwrap();
    let policies2 = reader.read_policy_map().unwrap();
    assert_eq!(policies1.len(), policies2.len());
    assert_eq!(policies1[0].src_identity, policies2[0].src_identity);
    assert_eq!(policies1[0].verdict, policies2[0].verdict);

    // Drop map should return consistent data
    let drops1 = reader.read_drop_map().unwrap();
    let drops2 = reader.read_drop_map().unwrap();
    assert_eq!(drops1.len(), drops2.len());
}

#[test]
fn test_mock_reader_all_maps_readable() {
    let reader = MockMapReader;

    assert!(reader.read_policy_map().is_ok());
    assert!(reader.read_conntrack_map().is_ok());
    assert!(reader.read_lb_map().is_ok());
    assert!(reader.read_ipcache_map().is_ok());
    assert!(reader.read_drop_map().is_ok());
}

#[test]
fn test_mock_reader_policy_map_has_allow_verdict() {
    let reader = MockMapReader;
    let decisions = reader.read_policy_map().unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].verdict, PolicyVerdict::Allow);
}

// ---------------------------------------------------------------------------
// Tests: K8sClient::mock() is reusable across modules
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_k8s_mock_cloneable_across_modules() {
    let k8s = mock_k8s_client();

    // Clone the client for each module (mimicking real usage)
    let _healer = SelfHealer::new(HealerConfig::default(), MockMapReader, k8s.clone());
    let _autopolicy = AutoPolicy::new(AutoPolicyConfig::default(), MockMapReader, k8s.clone());
    let _rootcause = RootCauseEngine::new(RootCauseConfig::default(), MockMapReader, k8s.clone());
    let _simulator = Simulator::new(SimulatorConfig::default(), MockMapReader, k8s.clone());
    let _replay = ReplayEngine::new(ReplayConfig::default(), MockMapReader, k8s);

    // If we get here, all modules accepted the cloned K8sClient
}

// ---------------------------------------------------------------------------
// Tests: Full lifecycle via public APIs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_healer_then_rootcause_lifecycle() {
    let k8s = mock_k8s_client();

    // Step 1: Healer runs and processes mock data
    let mut healer = SelfHealer::new(HealerConfig::default(), MockMapReader, k8s.clone());
    let healer_stats = healer.run().await.unwrap();
    assert!(
        healer_stats.problems_detected > 0,
        "Healer should detect problems from mock drops"
    );

    // Step 2: RootCause analyzes the same mock data without error
    let mut rootcause = RootCauseEngine::new(RootCauseConfig::default(), MockMapReader, k8s);
    rootcause.analyze_drops().await.unwrap();
}

#[tokio::test]
async fn test_autopolicy_to_simulator_lifecycle() {
    let k8s = mock_k8s_client();

    // Use a config with min_observations=1 so mock data (3 connections) meets the threshold
    let mut config = AutoPolicyConfig::default();
    config.min_observations = 1;

    // Step 1: AutoPolicy starts learning
    let mut autopolicy = AutoPolicy::new(config, MockMapReader, k8s.clone());
    autopolicy.start_learning().await.unwrap();
    assert!(matches!(autopolicy.state(), LearningState::Learning { .. }));

    // Step 2: Update (MockMapReader has conntrack data)
    let learning_stats = autopolicy.update().await.unwrap();
    assert!(
        learning_stats.connections_observed > 0,
        "Should observe connections from MockMapReader"
    );

    // Step 3: Generate policies
    let policies = autopolicy.generate_policies().unwrap();
    assert!(
        !policies.is_empty(),
        "Should generate at least one policy from observed traffic"
    );

    // Step 4: Simulator loads history from mock conntrack data
    let mut simulator = Simulator::new(SimulatorConfig::default(), MockMapReader, k8s);
    let loaded = simulator.load_history().await.unwrap();
    assert!(
        loaded > 0,
        "Simulator should load conntrack entries from MockMapReader"
    );
}

#[tokio::test]
async fn test_replay_to_rootcause_lifecycle() {
    let k8s = mock_k8s_client();

    // Step 1: Start a recording
    let mut replay = ReplayEngine::new(ReplayConfig::default(), MockMapReader, k8s.clone());
    let rec_id = replay
        .start_recording("lifecycle-test".to_string())
        .await
        .unwrap();
    assert!(!rec_id.is_empty());

    // Step 2: Capture (MockMapReader has conntrack data)
    let captured = replay.capture().await.unwrap();
    assert!(captured > 0, "Should capture flows from MockMapReader");

    // Step 3: Stop recording
    let recording = replay.stop_recording().await.unwrap();
    assert_eq!(recording.name, "lifecycle-test");
    assert!(
        recording.flow_count > 0,
        "Recording should contain captured flows"
    );

    // Step 4: RootCause analyzes mock drops without error
    let mut rootcause = RootCauseEngine::new(RootCauseConfig::default(), MockMapReader, k8s);
    rootcause.analyze_drops().await.unwrap();
}

// ---------------------------------------------------------------------------
// Tests: All five modules created simultaneously
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_all_five_modules_coexist() {
    let k8s = mock_k8s_client();

    let mut healer = SelfHealer::new(HealerConfig::default(), MockMapReader, k8s.clone());
    let mut autopolicy = AutoPolicy::new(AutoPolicyConfig::default(), MockMapReader, k8s.clone());
    let mut rootcause =
        RootCauseEngine::new(RootCauseConfig::default(), MockMapReader, k8s.clone());
    let mut simulator = Simulator::new(SimulatorConfig::default(), MockMapReader, k8s.clone());
    let mut replay = ReplayEngine::new(ReplayConfig::default(), MockMapReader, k8s);

    // Run all modules
    let _healer_stats = healer.run().await.unwrap();
    autopolicy.start_learning().await.unwrap();
    let _learning_stats = autopolicy.update().await.unwrap();
    let _analyses = rootcause.analyze_drops().await.unwrap();
    let _loaded = simulator.load_history().await.unwrap();
    let rec_id = replay
        .start_recording("all-modules-test".to_string())
        .await
        .unwrap();

    // Verify all are in expected states
    assert!(matches!(autopolicy.state(), LearningState::Learning { .. }));
    assert!(!rec_id.is_empty());
    assert!(replay.stats().recording_in_progress);
}
