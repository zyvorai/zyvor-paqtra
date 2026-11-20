// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// Integration tests for the AutoPolicy learning and policy generation module.
///
/// Tests the autopolicy engine's learning lifecycle, state transitions,
/// and policy generation using MockMapReader for eBPF data and
/// K8sClient::mock() for Kubernetes interactions.
use paqtra::ebpf::MockMapReader;
use paqtra::kubernetes::K8sClient;
use paqtra::modules::autopolicy::{
    AutoPolicy, AutoPolicyConfig, LabelSet, LearningState, LearningStats, Protocol, TrafficPattern,
};
use std::collections::HashMap;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn mock_k8s_client() -> K8sClient {
    K8sClient::mock()
}

// ---------------------------------------------------------------------------
// Tests: AutoPolicy creation and initial state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_autopolicy_initial_state_not_started() {
    let config = AutoPolicyConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let ap = AutoPolicy::new(config, reader, k8s);
    assert_eq!(*ap.state(), LearningState::NotStarted);
}

#[tokio::test]
async fn test_autopolicy_observations_initially_empty() {
    let config = AutoPolicyConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let ap = AutoPolicy::new(config, reader, k8s);
    assert!(
        ap.observations().is_empty(),
        "No observations should exist before learning starts"
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
async fn test_autopolicy_stats_initially_zero() {
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

// ---------------------------------------------------------------------------
// Tests: Learning lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_autopolicy_start_learning_transitions_state() {
    let config = AutoPolicyConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut ap = AutoPolicy::new(config, reader, k8s);
    ap.start_learning().await.unwrap();

    assert!(
        matches!(ap.state(), LearningState::Learning { .. }),
        "State should transition to Learning after start_learning"
    );
}

#[tokio::test]
async fn test_autopolicy_start_learning_records_timestamp() {
    let config = AutoPolicyConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut ap = AutoPolicy::new(config, reader, k8s);
    ap.start_learning().await.unwrap();

    match ap.state() {
        LearningState::Learning {
            started_at,
            progress,
        } => {
            assert!(*started_at > 0, "started_at should be a valid timestamp");
            assert!(
                (*progress - 0.0).abs() < f32::EPSILON,
                "Initial progress should be 0.0"
            );
        }
        other => panic!("Expected Learning state, got {:?}", other),
    }
}

#[tokio::test]
async fn test_autopolicy_disabled_rejects_learning() {
    let mut config = AutoPolicyConfig::default();
    config.enabled = false;
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut ap = AutoPolicy::new(config, reader, k8s);
    let result = ap.start_learning().await;
    assert!(
        result.is_err(),
        "start_learning should fail when autopolicy is disabled"
    );
}

#[tokio::test]
async fn test_autopolicy_update_returns_stats() {
    let config = AutoPolicyConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut ap = AutoPolicy::new(config, reader, k8s);
    ap.start_learning().await.unwrap();

    // MockMapReader returns mock conntrack entries
    let _stats = ap.update().await.unwrap();
}

#[tokio::test]
async fn test_autopolicy_disabled_update_returns_empty() {
    let mut config = AutoPolicyConfig::default();
    config.enabled = false;
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut ap = AutoPolicy::new(config, reader, k8s);
    let stats = ap.update().await.unwrap();
    assert_eq!(stats.connections_observed, 0);
    assert_eq!(stats.patterns_learned, 0);
    assert_eq!(stats.unique_patterns, 0);
}

// ---------------------------------------------------------------------------
// Tests: Policy generation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_autopolicy_generate_policies_with_no_observations() {
    let config = AutoPolicyConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut ap = AutoPolicy::new(config, reader, k8s);
    let policies = ap.generate_policies().unwrap();
    assert!(
        policies.is_empty(),
        "With no observations, no policies should be generated"
    );
}

#[tokio::test]
async fn test_autopolicy_state_transitions_during_generation() {
    let config = AutoPolicyConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut ap = AutoPolicy::new(config, reader, k8s);

    // generate_policies sets state to Generating, then Generated
    let policies = ap.generate_policies().unwrap();
    match ap.state() {
        LearningState::Generated { count } => {
            assert_eq!(*count, policies.len());
        }
        other => panic!("Expected Generated state, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Tests: AutoPolicyConfig
// ---------------------------------------------------------------------------

#[test]
fn test_autopolicy_config_defaults_are_safe() {
    let config = AutoPolicyConfig::default();
    assert!(config.enabled, "Should be enabled by default");
    assert!(
        config.audit_mode,
        "Audit mode should be on by default for safety"
    );
    assert!(!config.auto_apply, "auto_apply should be off by default");
    assert!(
        !config.auto_generate,
        "auto_generate should be off by default"
    );
    assert!(
        config.min_observations >= 1,
        "Must require at least 1 observation"
    );
    assert!(
        config.learning_duration > Duration::ZERO,
        "Learning duration must be positive"
    );
    assert!(
        config.update_interval_secs > 0,
        "Update interval must be positive"
    );
}

#[test]
fn test_autopolicy_config_learning_duration_default_7_days() {
    let config = AutoPolicyConfig::default();
    let seven_days = Duration::from_secs(7 * 24 * 60 * 60);
    assert_eq!(config.learning_duration, seven_days);
}

// ---------------------------------------------------------------------------
// Tests: LearningState variants
// ---------------------------------------------------------------------------

#[test]
fn test_learning_state_not_started_equality() {
    assert_eq!(LearningState::NotStarted, LearningState::NotStarted);
}

#[test]
fn test_learning_state_learning_holds_values() {
    let state = LearningState::Learning {
        started_at: 1000,
        progress: 0.5,
    };
    if let LearningState::Learning {
        started_at,
        progress,
    } = state
    {
        assert_eq!(started_at, 1000);
        assert!((progress - 0.5).abs() < f32::EPSILON);
    } else {
        panic!("Expected Learning variant");
    }
}

#[test]
fn test_learning_state_generated_holds_count() {
    let state = LearningState::Generated { count: 5 };
    if let LearningState::Generated { count } = state {
        assert_eq!(count, 5);
    } else {
        panic!("Expected Generated variant");
    }
}

#[test]
fn test_learning_state_applied_holds_count() {
    let state = LearningState::Applied { count: 3 };
    if let LearningState::Applied { count } = state {
        assert_eq!(count, 3);
    } else {
        panic!("Expected Applied variant");
    }
}

#[test]
fn test_learning_state_inequality_across_variants() {
    assert_ne!(LearningState::NotStarted, LearningState::Generating);
    assert_ne!(
        LearningState::NotStarted,
        LearningState::Generated { count: 0 }
    );
}

// ---------------------------------------------------------------------------
// Tests: Protocol and LabelSet
// ---------------------------------------------------------------------------

#[test]
fn test_protocol_from_number_well_known() {
    assert_eq!(Protocol::from_number(6), Protocol::TCP);
    assert_eq!(Protocol::from_number(17), Protocol::UDP);
    assert_eq!(Protocol::from_number(1), Protocol::ICMP);
}

#[test]
fn test_protocol_from_number_unknown() {
    assert_eq!(Protocol::from_number(0), Protocol::Other(0));
    assert_eq!(Protocol::from_number(99), Protocol::Other(99));
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
    let mut map = HashMap::new();
    map.insert("app".to_string(), "web".to_string());
    map.insert("tier".to_string(), "frontend".to_string());

    let ls = LabelSet::new(map.clone());
    assert!(!ls.is_empty());
    assert_eq!(ls.get("app"), Some(&"web".to_string()));
    assert_eq!(ls.get("tier"), Some(&"frontend".to_string()));
    assert_eq!(ls.get("nonexistent"), None);
}

#[test]
fn test_label_set_empty() {
    let ls = LabelSet::new(HashMap::new());
    assert!(ls.is_empty());
    assert_eq!(ls.get("anything"), None);
}

#[test]
fn test_label_set_equality_regardless_of_insertion_order() {
    let mut m1 = HashMap::new();
    m1.insert("z".to_string(), "1".to_string());
    m1.insert("a".to_string(), "2".to_string());

    let mut m2 = HashMap::new();
    m2.insert("a".to_string(), "2".to_string());
    m2.insert("z".to_string(), "1".to_string());

    let ls1 = LabelSet::new(m1);
    let ls2 = LabelSet::new(m2);
    assert_eq!(
        ls1, ls2,
        "LabelSets should be equal regardless of insertion order"
    );
}

// ---------------------------------------------------------------------------
// Tests: TrafficPattern
// ---------------------------------------------------------------------------

#[test]
fn test_traffic_pattern_equality() {
    let mut labels = HashMap::new();
    labels.insert("app".to_string(), "web".to_string());

    let p1 = TrafficPattern {
        src_namespace: "default".to_string(),
        src_labels: LabelSet::new(labels.clone()),
        dst_namespace: "backend".to_string(),
        dst_labels: LabelSet::new(labels.clone()),
        port: 80,
        protocol: Protocol::TCP,
    };
    let p2 = TrafficPattern {
        src_namespace: "default".to_string(),
        src_labels: LabelSet::new(labels.clone()),
        dst_namespace: "backend".to_string(),
        dst_labels: LabelSet::new(labels.clone()),
        port: 80,
        protocol: Protocol::TCP,
    };
    assert_eq!(p1, p2);
}

#[test]
fn test_traffic_pattern_different_port_not_equal() {
    let labels = HashMap::new();
    let p1 = TrafficPattern {
        src_namespace: "default".to_string(),
        src_labels: LabelSet::new(labels.clone()),
        dst_namespace: "default".to_string(),
        dst_labels: LabelSet::new(labels.clone()),
        port: 80,
        protocol: Protocol::TCP,
    };
    let p2 = TrafficPattern {
        src_namespace: "default".to_string(),
        src_labels: LabelSet::new(labels.clone()),
        dst_namespace: "default".to_string(),
        dst_labels: LabelSet::new(labels),
        port: 443,
        protocol: Protocol::TCP,
    };
    assert_ne!(p1, p2, "Different ports should produce different patterns");
}

// ---------------------------------------------------------------------------
// Tests: LearningStats
// ---------------------------------------------------------------------------

#[test]
fn test_learning_stats_default() {
    let stats = LearningStats::default();
    assert_eq!(stats.connections_observed, 0);
    assert_eq!(stats.patterns_learned, 0);
    assert_eq!(stats.unique_patterns, 0);
}
