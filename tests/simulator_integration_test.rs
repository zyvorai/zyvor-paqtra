// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Integration tests for the What-If Simulator module.
///
/// Tests the simulator's scenario preparation, risk scoring, and
/// impact analysis using MockMapReader for eBPF data and
/// K8sClient::mock() for Kubernetes interactions.
use paqtra::ebpf::MockMapReader;
use paqtra::kubernetes::K8sClient;
use paqtra::modules::simulator::{
    DependencyCriticality, ImpactType, RiskLevel, SimulationScenario, Simulator, SimulatorConfig,
};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn mock_k8s_client() -> K8sClient {
    K8sClient::mock()
}

// ---------------------------------------------------------------------------
// Tests: Simulator creation and initial state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_simulator_initial_state_empty() {
    let config = SimulatorConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let sim = Simulator::new(config, reader, k8s);
    let stats = sim.stats();
    assert_eq!(stats.flow_history_size, 0, "No flows loaded yet");
    assert_eq!(stats.current_policies, 0, "No policies loaded yet");
}

#[tokio::test]
async fn test_simulator_stats_reflects_config() {
    let mut config = SimulatorConfig::default();
    config.history_window_secs = 7200;
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let sim = Simulator::new(config, reader, k8s);
    let stats = sim.stats();
    assert_eq!(
        stats.history_window_secs, 7200,
        "Stats should reflect configured history window"
    );
}

// ---------------------------------------------------------------------------
// Tests: SimulatorConfig defaults
// ---------------------------------------------------------------------------

#[test]
fn test_simulator_config_defaults() {
    let config = SimulatorConfig::default();
    assert!(config.enabled);
    assert!(config.replay_flow_count > 0, "Must replay at least 1 flow");
    assert!(config.track_dependencies);
    assert!(config.risk_scoring);
    assert!(
        config.min_confidence > 0.0 && config.min_confidence <= 1.0,
        "Confidence must be in (0, 1]"
    );
    assert!(
        config.history_window_secs > 0,
        "History window must be positive"
    );
}

// ---------------------------------------------------------------------------
// Tests: RiskLevel scoring
// ---------------------------------------------------------------------------

#[test]
fn test_risk_level_from_score_low() {
    assert_eq!(RiskLevel::from_score(0), RiskLevel::Low);
    assert_eq!(RiskLevel::from_score(1), RiskLevel::Low);
    assert_eq!(RiskLevel::from_score(2), RiskLevel::Low);
}

#[test]
fn test_risk_level_from_score_medium() {
    assert_eq!(RiskLevel::from_score(3), RiskLevel::Medium);
    assert_eq!(RiskLevel::from_score(4), RiskLevel::Medium);
    assert_eq!(RiskLevel::from_score(5), RiskLevel::Medium);
}

#[test]
fn test_risk_level_from_score_high() {
    assert_eq!(RiskLevel::from_score(6), RiskLevel::High);
    assert_eq!(RiskLevel::from_score(7), RiskLevel::High);
    assert_eq!(RiskLevel::from_score(8), RiskLevel::High);
}

#[test]
fn test_risk_level_from_score_critical() {
    assert_eq!(RiskLevel::from_score(9), RiskLevel::Critical);
    assert_eq!(RiskLevel::from_score(10), RiskLevel::Critical);
}

#[test]
fn test_risk_level_ordering() {
    assert!(RiskLevel::Low < RiskLevel::Medium);
    assert!(RiskLevel::Medium < RiskLevel::High);
    assert!(RiskLevel::High < RiskLevel::Critical);
    assert!(RiskLevel::Low < RiskLevel::Critical);
}

#[test]
fn test_risk_level_to_string() {
    assert_eq!(RiskLevel::Low.to_string(), "Low");
    assert_eq!(RiskLevel::Medium.to_string(), "Medium");
    assert_eq!(RiskLevel::High.to_string(), "High");
    assert_eq!(RiskLevel::Critical.to_string(), "Critical");
}

#[test]
fn test_risk_level_equality() {
    assert_eq!(RiskLevel::Low, RiskLevel::Low);
    assert_eq!(RiskLevel::Critical, RiskLevel::Critical);
    assert_ne!(RiskLevel::Low, RiskLevel::High);
}

// ---------------------------------------------------------------------------
// Tests: ImpactType
// ---------------------------------------------------------------------------

#[test]
fn test_impact_type_equality() {
    assert_eq!(ImpactType::FullyBlocked, ImpactType::FullyBlocked);
    assert_eq!(ImpactType::PartiallyBlocked, ImpactType::PartiallyBlocked);
    assert_eq!(ImpactType::Unaffected, ImpactType::Unaffected);
    assert_eq!(ImpactType::PerformanceImpact, ImpactType::PerformanceImpact);
}

#[test]
fn test_impact_type_inequality() {
    assert_ne!(ImpactType::FullyBlocked, ImpactType::Unaffected);
    assert_ne!(ImpactType::PartiallyBlocked, ImpactType::PerformanceImpact);
    assert_ne!(ImpactType::FullyBlocked, ImpactType::PartiallyBlocked);
}

// ---------------------------------------------------------------------------
// Tests: DependencyCriticality
// ---------------------------------------------------------------------------

#[test]
fn test_dependency_criticality_equality() {
    assert_eq!(
        DependencyCriticality::Critical,
        DependencyCriticality::Critical
    );
    assert_eq!(
        DependencyCriticality::Important,
        DependencyCriticality::Important
    );
    assert_eq!(
        DependencyCriticality::Optional,
        DependencyCriticality::Optional
    );
    assert_ne!(
        DependencyCriticality::Critical,
        DependencyCriticality::Optional
    );
}

// ---------------------------------------------------------------------------
// Tests: SimulationScenario construction
// ---------------------------------------------------------------------------

#[test]
fn test_simulation_scenario_add_policy() {
    let scenario = SimulationScenario::AddPolicy {
        policy_yaml: "apiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy".to_string(),
        namespace: "default".to_string(),
    };
    match &scenario {
        SimulationScenario::AddPolicy {
            policy_yaml,
            namespace,
        } => {
            assert!(policy_yaml.contains("CiliumNetworkPolicy"));
            assert_eq!(namespace, "default");
        }
        _ => panic!("Expected AddPolicy"),
    }
}

#[test]
fn test_simulation_scenario_remove_policy() {
    let scenario = SimulationScenario::RemovePolicy {
        policy_name: "allow-dns".to_string(),
        namespace: "kube-system".to_string(),
    };
    match &scenario {
        SimulationScenario::RemovePolicy {
            policy_name,
            namespace,
        } => {
            assert_eq!(policy_name, "allow-dns");
            assert_eq!(namespace, "kube-system");
        }
        _ => panic!("Expected RemovePolicy"),
    }
}

#[test]
fn test_simulation_scenario_default_deny() {
    let scenario = SimulationScenario::DefaultDeny {
        namespace: "prod".to_string(),
    };
    match &scenario {
        SimulationScenario::DefaultDeny { namespace } => {
            assert_eq!(namespace, "prod");
        }
        _ => panic!("Expected DefaultDeny"),
    }
}

#[test]
fn test_simulation_scenario_block_traffic() {
    let mut from_labels = HashMap::new();
    from_labels.insert("app".to_string(), "frontend".to_string());
    let mut to_labels = HashMap::new();
    to_labels.insert("app".to_string(), "database".to_string());

    let scenario = SimulationScenario::BlockTraffic {
        from_labels: from_labels.clone(),
        to_labels: to_labels.clone(),
        port: Some(5432),
        protocol: Some("TCP".to_string()),
    };
    match &scenario {
        SimulationScenario::BlockTraffic {
            from_labels: fl,
            to_labels: tl,
            port,
            protocol,
        } => {
            assert_eq!(fl.get("app").unwrap(), "frontend");
            assert_eq!(tl.get("app").unwrap(), "database");
            assert_eq!(*port, Some(5432));
            assert_eq!(protocol.as_deref(), Some("TCP"));
        }
        _ => panic!("Expected BlockTraffic"),
    }
}

#[test]
fn test_simulation_scenario_allow_traffic() {
    let scenario = SimulationScenario::AllowTraffic {
        from_labels: HashMap::new(),
        to_labels: HashMap::new(),
        port: 80,
        protocol: "TCP".to_string(),
    };
    match &scenario {
        SimulationScenario::AllowTraffic { port, protocol, .. } => {
            assert_eq!(*port, 80);
            assert_eq!(protocol, "TCP");
        }
        _ => panic!("Expected AllowTraffic"),
    }
}

#[test]
fn test_simulation_scenario_block_external_ip() {
    let ip: std::net::IpAddr = "203.0.113.1".parse().unwrap();
    let scenario = SimulationScenario::BlockExternalIP { ip };
    match &scenario {
        SimulationScenario::BlockExternalIP { ip: blocked_ip } => {
            assert_eq!(blocked_ip.to_string(), "203.0.113.1");
        }
        _ => panic!("Expected BlockExternalIP"),
    }
}

// ---------------------------------------------------------------------------
// Tests: Simulator load_history with mock data
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_simulator_load_history_empty_conntrack() {
    let config = SimulatorConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut sim = Simulator::new(config, reader, k8s);
    let count = sim.load_history().await.unwrap();
    // MockMapReader has realistic conntrack entries

    let stats = sim.stats();
    assert_eq!(stats.flow_history_size, count);
}

// ---------------------------------------------------------------------------
// Tests: Simulator scenario execution
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_simulator_simulate_add_policy() {
    let config = SimulatorConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut sim = Simulator::new(config, reader, k8s);

    let scenario = SimulationScenario::AddPolicy {
        policy_yaml: r#"
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: test-policy
  namespace: default
spec:
  endpointSelector: {}
  ingress:
    - fromEndpoints:
        - {}
"#
        .to_string(),
        namespace: "default".to_string(),
    };

    // MockMapReader provides conntrack data, simulate should complete successfully
    let result = sim.simulate(scenario).await.unwrap();
    // Verify the simulation produced meaningful output
    assert!(
        !result.details.flow_results.is_empty(),
        "Simulation should produce flow results"
    );
}

#[tokio::test]
async fn test_simulator_simulate_default_deny() {
    let config = SimulatorConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut sim = Simulator::new(config, reader, k8s);

    let scenario = SimulationScenario::DefaultDeny {
        namespace: "prod".to_string(),
    };

    let result = sim.simulate(scenario).await.unwrap();

    // Recommendations should mention default-deny for this scenario type
    assert!(
        result
            .recommendations
            .iter()
            .any(|r| r.contains("default-deny") || r.contains("safe")),
        "Recommendations should provide guidance: {:?}",
        result.recommendations
    );
}
