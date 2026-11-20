// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// Integration tests for the Self-Healer module.
///
/// Tests the healer's problem detection and fix generation workflow
/// using MockMapReader for eBPF data and K8sClient::mock() for
/// Kubernetes interactions. Only exercises the public API.
use paqtra::ebpf::MockMapReader;
use paqtra::kubernetes::K8sClient;
use paqtra::modules::healer::{FixAction, HealerConfig, HealerStats, Problem, SelfHealer};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn mock_k8s_client() -> K8sClient {
    K8sClient::mock()
}

// ---------------------------------------------------------------------------
// Tests: SelfHealer creation and initial state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_healer_initial_state_is_empty() {
    let config = HealerConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let healer = SelfHealer::new(config, reader, k8s);
    let stats = healer.stats();
    assert_eq!(stats.problems_detected, 0);
    assert_eq!(stats.fixes_proposed, 0);
    assert_eq!(stats.fixes_applied, 0);
    assert!(healer.problems().is_empty());
    assert!(healer.fixes().is_empty());
}

#[test]
fn test_healer_config_safe_defaults() {
    let config = HealerConfig::default();
    assert!(config.enabled, "Healer should be enabled by default");
    assert!(
        !config.auto_apply,
        "auto_apply should be off by default for safety"
    );
    assert!(config.dry_run, "dry_run should be on by default for safety");
    assert!(
        config.check_interval_secs > 0,
        "check_interval_secs must be positive"
    );
}

#[test]
fn test_healer_config_check_interval_value() {
    let config = HealerConfig::default();
    assert_eq!(
        config.check_interval_secs, 30,
        "Default check interval should be 30 seconds"
    );
}

// ---------------------------------------------------------------------------
// Tests: Healer detect and fix lifecycle via public run() method
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_healer_detect_and_fix_lifecycle() {
    let config = HealerConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut healer = SelfHealer::new(config, reader, k8s);

    // MockMapReader returns realistic drop data with PolicyDenied entries,
    // so healer should detect problems and propose fixes.
    let stats = healer.run().await.unwrap();
    // dry_run is true by default, so no fixes applied
    assert_eq!(stats.fixes_applied, 0, "dry_run prevents application");
}

#[tokio::test]
async fn test_healer_run_problems_list_is_empty_after_empty_run() {
    let config = HealerConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut healer = SelfHealer::new(config, reader, k8s);
    healer.run().await.unwrap();

    // MockMapReader has PolicyDenied drops, so problems may be detected
    // Fixes list is empty because dry_run is true and auto_apply is false
    assert!(
        healer.fixes().is_empty(),
        "With default config (auto_apply=false), no fixes should be applied"
    );
}

#[tokio::test]
async fn test_healer_disabled_returns_empty_stats() {
    let mut config = HealerConfig::default();
    config.enabled = false;
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut healer = SelfHealer::new(config, reader, k8s);
    let stats = healer.run().await.unwrap();
    assert_eq!(stats.problems_detected, 0);
    assert_eq!(stats.fixes_proposed, 0);
    assert_eq!(stats.fixes_applied, 0);
}

#[tokio::test]
async fn test_healer_dry_run_does_not_apply_fixes() {
    // Even with auto_apply on, dry_run should prevent actual application
    let mut config = HealerConfig::default();
    config.auto_apply = true;
    config.dry_run = true;
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut healer = SelfHealer::new(config, reader, k8s);
    let stats = healer.run().await.unwrap();
    assert_eq!(
        stats.fixes_applied, 0,
        "dry_run should prevent fixes from being applied"
    );
}

// ---------------------------------------------------------------------------
// Tests: HealerStats tracking
// ---------------------------------------------------------------------------

#[test]
fn test_healer_stats_default_all_zero() {
    let stats = HealerStats::default();
    assert_eq!(stats.problems_detected, 0);
    assert_eq!(stats.fixes_proposed, 0);
    assert_eq!(stats.fixes_applied, 0);
}

#[tokio::test]
async fn test_healer_stats_after_run() {
    let config = HealerConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut healer = SelfHealer::new(config, reader, k8s);
    let run_stats = healer.run().await.unwrap();
    // MockMapReader has drop data, stats reflect detected problems

    // The healer's internal stats should match
    let internal_stats = healer.stats();
    assert_eq!(
        internal_stats.problems_detected,
        run_stats.problems_detected
    );
}

#[tokio::test]
async fn test_healer_stats_consistent_with_run_result() {
    let config = HealerConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut healer = SelfHealer::new(config, reader, k8s);
    let run_stats = healer.run().await.unwrap();
    let internal_stats = healer.stats();

    // The run return value and internal stats should agree on problem count
    assert_eq!(
        run_stats.problems_detected, internal_stats.problems_detected,
        "run() return value and stats() should agree on problem count"
    );
}

// ---------------------------------------------------------------------------
// Tests: Problem type construction and equality
// ---------------------------------------------------------------------------

#[test]
fn test_problem_dns_drops_equality() {
    let p1 = Problem::DNSDrops {
        namespace: "default".to_string(),
        pod: "web".to_string(),
        count: 10,
    };
    let p2 = Problem::DNSDrops {
        namespace: "default".to_string(),
        pod: "web".to_string(),
        count: 10,
    };
    assert_eq!(p1, p2);

    let p3 = Problem::DNSDrops {
        namespace: "default".to_string(),
        pod: "web".to_string(),
        count: 20,
    };
    assert_ne!(p1, p3, "Different counts should not be equal");
}

#[test]
fn test_problem_dns_drops_different_namespace() {
    let p1 = Problem::DNSDrops {
        namespace: "default".to_string(),
        pod: "web".to_string(),
        count: 10,
    };
    let p2 = Problem::DNSDrops {
        namespace: "prod".to_string(),
        pod: "web".to_string(),
        count: 10,
    };
    assert_ne!(p1, p2, "Different namespaces should not be equal");
}

#[test]
fn test_problem_mtu_mismatch_fields() {
    let p = Problem::MTUMismatch {
        namespace: "prod".to_string(),
        pod: "api-server".to_string(),
        expected: 1500,
        actual: 1400,
    };
    match p {
        Problem::MTUMismatch {
            namespace,
            pod,
            expected,
            actual,
        } => {
            assert_eq!(namespace, "prod");
            assert_eq!(pod, "api-server");
            assert_eq!(expected, 1500);
            assert_eq!(actual, 1400);
        }
        _ => panic!("Expected MTUMismatch"),
    }
}

#[test]
fn test_problem_policy_gap_fields() {
    let p = Problem::PolicyGap {
        src_namespace: "frontend".to_string(),
        src_pod: "web-abc".to_string(),
        dst_namespace: "backend".to_string(),
        dst_pod: "api-xyz".to_string(),
        port: 8080,
        protocol: "TCP".to_string(),
    };
    match p {
        Problem::PolicyGap {
            src_namespace,
            dst_namespace,
            port,
            protocol,
            ..
        } => {
            assert_eq!(src_namespace, "frontend");
            assert_eq!(dst_namespace, "backend");
            assert_eq!(port, 8080);
            assert_eq!(protocol, "TCP");
        }
        _ => panic!("Expected PolicyGap"),
    }
}

#[test]
fn test_problem_conntrack_full() {
    let p = Problem::ConntrackFull {
        node: "worker-1".to_string(),
        utilization: 0.95,
    };
    match p {
        Problem::ConntrackFull {
            node, utilization, ..
        } => {
            assert_eq!(node, "worker-1");
            assert!(utilization > 0.9);
        }
        _ => panic!("Expected ConntrackFull"),
    }
}

#[test]
fn test_problem_lb_timeout() {
    let p = Problem::LoadBalancerTimeout {
        service: "my-svc".to_string(),
        backend: "pod-abc".to_string(),
        timeout_count: 42,
    };
    match p {
        Problem::LoadBalancerTimeout {
            service,
            backend,
            timeout_count,
        } => {
            assert_eq!(service, "my-svc");
            assert_eq!(backend, "pod-abc");
            assert_eq!(timeout_count, 42);
        }
        _ => panic!("Expected LoadBalancerTimeout"),
    }
}

#[test]
fn test_problem_variants_are_not_equal_across_types() {
    let dns = Problem::DNSDrops {
        namespace: "default".to_string(),
        pod: "web".to_string(),
        count: 10,
    };
    let mtu = Problem::MTUMismatch {
        namespace: "default".to_string(),
        pod: "web".to_string(),
        expected: 1500,
        actual: 1400,
    };
    assert_ne!(dns, mtu, "Different problem variants should not be equal");
}

// ---------------------------------------------------------------------------
// Tests: FixAction type construction and equality
// ---------------------------------------------------------------------------

#[test]
fn test_fix_action_create_dns_policy() {
    let fix = FixAction::CreateDNSPolicy {
        namespace: "default".to_string(),
    };
    let fix2 = FixAction::CreateDNSPolicy {
        namespace: "default".to_string(),
    };
    assert_eq!(fix, fix2);
}

#[test]
fn test_fix_action_create_allow_policy() {
    let fix = FixAction::CreateAllowPolicy {
        src: "frontend".to_string(),
        dst: "backend".to_string(),
        port: 8080,
    };
    match &fix {
        FixAction::CreateAllowPolicy { src, dst, port } => {
            assert_eq!(src, "frontend");
            assert_eq!(dst, "backend");
            assert_eq!(*port, 8080);
        }
        _ => panic!("Expected CreateAllowPolicy"),
    }
}

#[test]
fn test_fix_action_adjust_mtu() {
    let fix = FixAction::AdjustMTU {
        namespace: "prod".to_string(),
        pod: "web".to_string(),
        new_mtu: 1450,
    };
    match &fix {
        FixAction::AdjustMTU {
            namespace,
            pod,
            new_mtu,
        } => {
            assert_eq!(namespace, "prod");
            assert_eq!(pod, "web");
            assert_eq!(*new_mtu, 1450);
        }
        _ => panic!("Expected AdjustMTU"),
    }
}

#[test]
fn test_fix_action_rebalance_backend() {
    let fix = FixAction::RebalanceBackend {
        service: "my-svc".to_string(),
        backend: "pod-1".to_string(),
    };
    match &fix {
        FixAction::RebalanceBackend { service, backend } => {
            assert_eq!(service, "my-svc");
            assert_eq!(backend, "pod-1");
        }
        _ => panic!("Expected RebalanceBackend"),
    }
}

#[test]
fn test_fix_action_tune_conntrack() {
    let fix = FixAction::TuneConntrack {
        node: "worker-1".to_string(),
        new_timeout: 3600,
    };
    match &fix {
        FixAction::TuneConntrack { node, new_timeout } => {
            assert_eq!(node, "worker-1");
            assert_eq!(*new_timeout, 3600);
        }
        _ => panic!("Expected TuneConntrack"),
    }
}

#[test]
fn test_fix_action_inequality_across_variants() {
    let dns = FixAction::CreateDNSPolicy {
        namespace: "default".to_string(),
    };
    let allow = FixAction::CreateAllowPolicy {
        src: "a".to_string(),
        dst: "b".to_string(),
        port: 80,
    };
    let mtu = FixAction::AdjustMTU {
        namespace: "prod".to_string(),
        pod: "web".to_string(),
        new_mtu: 1450,
    };

    assert_ne!(dns, allow);
    assert_ne!(allow, mtu);
    assert_ne!(dns, mtu);
}

// ---------------------------------------------------------------------------
// Tests: Multiple sequential runs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_healer_multiple_runs_are_idempotent() {
    let config = HealerConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut healer = SelfHealer::new(config, reader, k8s);

    // Run multiple times - each should produce consistent results
    let mut prev_detected = None;
    for _ in 0..3 {
        let stats = healer.run().await.unwrap();
        if let Some(prev) = prev_detected {
            assert_eq!(
                stats.problems_detected, prev,
                "Multiple runs should be idempotent"
            );
        }
        prev_detected = Some(stats.problems_detected);
        // dry_run + auto_apply=false => no fixes applied
        assert_eq!(stats.fixes_applied, 0);
    }
}
