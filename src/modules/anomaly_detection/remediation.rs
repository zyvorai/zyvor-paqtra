// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Auto-Remediation Engine - Suggests and applies fixes
use anyhow::Result;

use super::{Anomaly, AnomalyType, RemediationAction, RemediationType, Severity};
use crate::modules::yaml_escape;

/// Suggests and optionally applies automated remediation
pub struct RemediationEngine {
    auto_apply: bool,
}

impl RemediationEngine {
    pub fn new(auto_apply: bool) -> Self {
        Self { auto_apply }
    }

    /// Suggest remediation for an anomaly
    pub fn suggest_remediation(&self, mut anomaly: Anomaly) -> Result<Anomaly> {
        let remediation = match anomaly.anomaly_type {
            AnomalyType::TrafficSpike => self.handle_traffic_spike(&anomaly),
            AnomalyType::ErrorRateSpike => self.handle_error_spike(&anomaly),
            AnomalyType::LatencyIncrease => self.handle_latency(&anomaly),
            AnomalyType::PortScan => self.handle_port_scan(&anomaly),
            AnomalyType::DNSTunneling => self.handle_dns_tunneling(&anomaly),
            AnomalyType::DataExfiltration => self.handle_data_exfiltration(&anomaly),
            AnomalyType::UnusualConnectionPattern => self.handle_unusual_pattern(&anomaly),
            _ => self.default_remediation(&anomaly),
        };

        // Audit log all remediation suggestions
        tracing::info!(
            anomaly_id = %anomaly.id,
            anomaly_type = ?anomaly.anomaly_type,
            severity = ?anomaly.severity,
            action_type = ?remediation.action_type,
            auto_applicable = remediation.auto_applicable,
            confidence = remediation.confidence,
            "Remediation suggested: {}",
            remediation.description
        );

        if remediation.auto_applicable {
            tracing::warn!(
                anomaly_id = %anomaly.id,
                action_type = ?remediation.action_type,
                "AUTO-REMEDIATION: Action marked for automatic application - {}",
                remediation.description
            );
        }

        anomaly.remediation = Some(remediation);
        Ok(anomaly)
    }

    fn handle_traffic_spike(&self, anomaly: &Anomaly) -> RemediationAction {
        let confidence = if anomaly.severity >= Severity::High {
            0.9
        } else {
            0.7
        };

        RemediationAction {
            action_type: RemediationType::RateLimit,
            description: format!(
                "Apply rate limiting to {}/{} ({}% of baseline)",
                anomaly.context.namespace,
                anomaly.context.service,
                (anomaly.observed_value / anomaly.baseline_value * 100.0) as i32
            ),
            confidence,
            auto_applicable: self.auto_apply && confidence > 0.8,
            policy_yaml: Some(self.generate_rate_limit_policy(anomaly)),
        }
    }

    fn handle_error_spike(&self, anomaly: &Anomaly) -> RemediationAction {
        RemediationAction {
            action_type: RemediationType::RollbackDeployment,
            description: format!(
                "Consider rolling back recent deployment in {}/{}",
                anomaly.context.namespace, anomaly.context.service
            ),
            confidence: 0.75,
            auto_applicable: false, // Never auto-rollback
            policy_yaml: None,
        }
    }

    fn handle_latency(&self, anomaly: &Anomaly) -> RemediationAction {
        RemediationAction {
            action_type: RemediationType::ScaleUp,
            description: format!(
                "Scale up {} replicas (latency {:.0}ms vs baseline {:.0}ms)",
                anomaly.context.service, anomaly.observed_value, anomaly.baseline_value
            ),
            confidence: 0.8,
            auto_applicable: self.auto_apply,
            policy_yaml: None,
        }
    }

    fn handle_port_scan(&self, anomaly: &Anomaly) -> RemediationAction {
        RemediationAction {
            action_type: RemediationType::BlockDestination,
            description: format!(
                "Block traffic from {} (suspected port scan)",
                anomaly.context.pod
            ),
            confidence: 0.95,
            auto_applicable: self.auto_apply && anomaly.severity >= Severity::High,
            policy_yaml: Some(self.generate_deny_policy(anomaly)),
        }
    }

    fn handle_dns_tunneling(&self, anomaly: &Anomaly) -> RemediationAction {
        RemediationAction {
            action_type: RemediationType::ApplyNetworkPolicy,
            description: format!(
                "Restrict DNS queries from {} (suspected tunneling)",
                anomaly.context.service
            ),
            confidence: 0.85,
            auto_applicable: false, // Requires human review
            policy_yaml: Some(self.generate_dns_restriction_policy(anomaly)),
        }
    }

    fn handle_data_exfiltration(&self, anomaly: &Anomaly) -> RemediationAction {
        RemediationAction {
            action_type: RemediationType::IsolatePod,
            description: format!(
                "URGENT: Isolate {}/{} (suspected data exfiltration)",
                anomaly.context.namespace, anomaly.context.pod
            ),
            confidence: 0.9,
            auto_applicable: self.auto_apply && anomaly.severity >= Severity::Critical,
            policy_yaml: Some(self.generate_isolation_policy(anomaly)),
        }
    }

    fn handle_unusual_pattern(&self, anomaly: &Anomaly) -> RemediationAction {
        RemediationAction {
            action_type: RemediationType::AlertOnly,
            description: format!(
                "Monitor {} for unusual connection pattern",
                anomaly.context.service
            ),
            confidence: 0.6,
            auto_applicable: false,
            policy_yaml: None,
        }
    }

    fn default_remediation(&self, _anomaly: &Anomaly) -> RemediationAction {
        RemediationAction {
            action_type: RemediationType::AlertOnly,
            description: "Manual investigation recommended".to_string(),
            confidence: 0.5,
            auto_applicable: false,
            policy_yaml: None,
        }
    }

    fn generate_rate_limit_policy(&self, anomaly: &Anomaly) -> String {
        format!(
            r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: rate-limit-{}
  namespace: {}
spec:
  endpointSelector:
    matchLabels:
      app: {}
  ingress:
  - fromEndpoints:
    - {{}}
    toPorts:
    - ports:
      - port: "{}"
        protocol: TCP
      rules:
        http:
        - method: ".*"
          rateLimit:
            requestsPerSecond: {}
"#,
            yaml_escape(&anomaly.context.service),
            yaml_escape(&anomaly.context.namespace),
            yaml_escape(&anomaly.context.service),
            anomaly.context.port,
            (anomaly.baseline_value * 1.5) as i32
        )
    }

    fn generate_deny_policy(&self, anomaly: &Anomaly) -> String {
        format!(
            r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: block-{}
  namespace: {}
spec:
  endpointSelector:
    matchLabels:
      app: {}
  ingress:
  - fromEndpoints:
    - matchLabels:
        io.kubernetes.pod.name: {}
    toPorts:
    - ports:
      - port: "{}"
        protocol: TCP
      rules:
        deny: true
"#,
            yaml_escape(&anomaly.id),
            yaml_escape(&anomaly.context.namespace),
            yaml_escape(&anomaly.context.service),
            yaml_escape(&anomaly.context.pod),
            anomaly.context.port
        )
    }

    fn generate_dns_restriction_policy(&self, anomaly: &Anomaly) -> String {
        format!(
            r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: dns-restrict-{}
  namespace: {}
spec:
  endpointSelector:
    matchLabels:
      app: {}
  egress:
  - toEndpoints:
    - matchLabels:
        io.kubernetes.pod.namespace: kube-system
        k8s-app: kube-dns
    toPorts:
    - ports:
      - port: "53"
        protocol: UDP
      rules:
        dns:
        - matchPattern: "*.cluster.local"
        - matchPattern: "*.svc"
"#,
            yaml_escape(&anomaly.context.service),
            yaml_escape(&anomaly.context.namespace),
            yaml_escape(&anomaly.context.service)
        )
    }

    fn generate_isolation_policy(&self, anomaly: &Anomaly) -> String {
        format!(
            r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: isolate-{}
  namespace: {}
spec:
  endpointSelector:
    matchLabels:
      io.kubernetes.pod.name: {}
  ingress:
  - {{}}
  egress:
  - toEndpoints:
    - matchLabels:
        io.kubernetes.pod.namespace: kube-system
        k8s-app: kube-dns
    toPorts:
    - ports:
      - port: "53"
        protocol: UDP
"#,
            yaml_escape(&anomaly.id),
            yaml_escape(&anomaly.context.namespace),
            yaml_escape(&anomaly.context.pod)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::{AnomalyContext, MetricType};
    use super::*;
    use chrono::Utc;

    fn make_anomaly(anomaly_type: AnomalyType, severity: Severity) -> Anomaly {
        Anomaly {
            id: "test-anomaly-1".to_string(),
            timestamp: Utc::now(),
            anomaly_type,
            severity,
            confidence: 0.9,
            metric: MetricType::RequestRate,
            baseline_value: 100.0,
            observed_value: 500.0,
            deviation: 4.0,
            context: AnomalyContext {
                namespace: "default".to_string(),
                pod: "web-abc123".to_string(),
                service: "web".to_string(),
                destination: None,
                protocol: "TCP".to_string(),
                port: 8080,
                contributing_factors: vec![],
                historical_occurrences: 0,
            },
            remediation: None,
        }
    }

    #[test]
    fn test_remediation_engine_creation() {
        let engine = RemediationEngine::new(true);
        assert!(engine.auto_apply);

        let engine2 = RemediationEngine::new(false);
        assert!(!engine2.auto_apply);
    }

    #[test]
    fn test_handle_traffic_spike_returns_rate_limit() {
        let engine = RemediationEngine::new(false);
        let anomaly = make_anomaly(AnomalyType::TrafficSpike, Severity::High);
        let action = engine.handle_traffic_spike(&anomaly);

        assert_eq!(action.action_type, RemediationType::RateLimit);
        assert!(action.policy_yaml.is_some());
        assert!(action.description.contains("rate limiting"));
    }

    #[test]
    fn test_handle_port_scan_returns_block_destination() {
        let engine = RemediationEngine::new(false);
        let anomaly = make_anomaly(AnomalyType::PortScan, Severity::High);
        let action = engine.handle_port_scan(&anomaly);

        assert_eq!(action.action_type, RemediationType::BlockDestination);
        assert_eq!(action.confidence, 0.95);
        assert!(action.description.contains("port scan"));
    }

    #[test]
    fn test_handle_dns_tunneling_returns_apply_network_policy() {
        let engine = RemediationEngine::new(true);
        let anomaly = make_anomaly(AnomalyType::DNSTunneling, Severity::High);
        let action = engine.handle_dns_tunneling(&anomaly);

        assert_eq!(action.action_type, RemediationType::ApplyNetworkPolicy);
        assert!(!action.auto_applicable); // DNS tunneling never auto-applies
        let yaml = action.policy_yaml.unwrap();
        assert!(yaml.contains("CiliumNetworkPolicy"));
        assert!(yaml.contains("dns-restrict"));
    }

    #[test]
    fn test_handle_data_exfiltration_returns_isolate_pod() {
        let engine = RemediationEngine::new(true);
        let anomaly = make_anomaly(AnomalyType::DataExfiltration, Severity::Critical);
        let action = engine.handle_data_exfiltration(&anomaly);

        assert_eq!(action.action_type, RemediationType::IsolatePod);
        assert_eq!(action.confidence, 0.9);
        assert!(action.auto_applicable); // auto_apply=true and severity=Critical
        assert!(action.description.contains("URGENT"));
    }

    #[test]
    fn test_confidence_values_within_range() {
        let engine = RemediationEngine::new(false);
        let anomaly = make_anomaly(AnomalyType::TrafficSpike, Severity::Low);

        let actions = vec![
            engine.handle_traffic_spike(&anomaly),
            engine.handle_error_spike(&anomaly),
            engine.handle_latency(&anomaly),
            engine.handle_port_scan(&anomaly),
            engine.handle_dns_tunneling(&anomaly),
            engine.handle_data_exfiltration(&anomaly),
            engine.handle_unusual_pattern(&anomaly),
            engine.default_remediation(&anomaly),
        ];

        for action in &actions {
            assert!(
                action.confidence >= 0.0 && action.confidence <= 1.0,
                "Confidence {} out of range for {:?}",
                action.confidence,
                action.action_type
            );
        }
    }

    #[test]
    fn test_suggest_remediation_populates_remediation_field() {
        let engine = RemediationEngine::new(false);
        let anomaly = make_anomaly(AnomalyType::PortScan, Severity::High);
        assert!(anomaly.remediation.is_none());

        let result = engine.suggest_remediation(anomaly).unwrap();
        assert!(result.remediation.is_some());
        let remediation = result.remediation.unwrap();
        assert_eq!(remediation.action_type, RemediationType::BlockDestination);
    }
}
