// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Simulation Engine
///
/// Core policy simulation engine that evaluates flows against modified policies
use super::*;
use anyhow::Result;
use std::collections::HashMap;

pub struct SimulationEngine {
    /// Current policy decisions
    policies: Vec<PolicyDecision>,

    /// Simulated policy changes
    simulated_policies: Vec<PolicyDecision>,

    /// Tracks which PolicyDecision entries were added by each named policy,
    /// enabling removal by policy name.
    policy_sources: HashMap<String, Vec<PolicyDecision>>,

    /// Execution trace for debugging
    trace: Vec<String>,
}

impl SimulationEngine {
    pub fn new(current_policies: Vec<PolicyDecision>) -> Self {
        let simulated_policies = current_policies.clone();
        Self {
            policies: current_policies,
            simulated_policies,
            policy_sources: HashMap::new(),
            trace: Vec::new(),
        }
    }

    /// Apply a simulation scenario to policies
    pub fn apply_scenario(&mut self, scenario: &SimulationScenario) -> Result<()> {
        self.trace
            .push(format!("Applying scenario: {:?}", scenario));

        match scenario {
            SimulationScenario::AddPolicy {
                policy_yaml,
                namespace,
            } => {
                self.add_policy(policy_yaml, namespace)?;
            }

            SimulationScenario::RemovePolicy {
                policy_name,
                namespace,
            } => {
                self.remove_policy(policy_name, namespace)?;
            }

            SimulationScenario::ModifyPolicy {
                policy_name,
                namespace,
                new_yaml,
            } => {
                self.remove_policy(policy_name, namespace)?;
                self.add_policy(new_yaml, namespace)?;
            }

            SimulationScenario::BlockTraffic {
                from_labels,
                to_labels,
                port,
                protocol,
            } => {
                self.block_traffic(from_labels, to_labels, port.as_ref(), protocol.as_ref())?;
            }

            SimulationScenario::AllowTraffic {
                from_labels,
                to_labels,
                port,
                protocol,
            } => {
                self.allow_traffic(from_labels, to_labels, *port, protocol)?;
            }

            SimulationScenario::BlockExternalIP { ip } => {
                self.block_external_ip(ip)?;
            }

            SimulationScenario::DefaultDeny { namespace } => {
                self.apply_default_deny(namespace)?;
            }
        }

        self.trace.push(format!(
            "Scenario applied. Total policies: {}",
            self.simulated_policies.len()
        ));

        Ok(())
    }

    /// Add a new policy (parsed from YAML)
    fn add_policy(&mut self, policy_yaml: &str, namespace: &str) -> Result<()> {
        self.trace
            .push(format!("Adding policy in namespace: {}", namespace));

        // Parse YAML to extract basic policy rules
        if let Ok(yaml_value) = serde_yaml::from_str::<serde_yaml::Value>(policy_yaml) {
            // Extract policy name for tracking
            let policy_name = yaml_value
                .get("metadata")
                .and_then(|m| m.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("unnamed");
            let source_key = format!("{}/{}", namespace, policy_name);

            let mut added = Vec::new();

            // Extract ingress/egress rules from CiliumNetworkPolicy
            if let Some(spec) = yaml_value.get("spec") {
                // Process ingress rules
                if let Some(ingress) = spec.get("ingress") {
                    if let Some(rules) = ingress.as_sequence() {
                        for rule in rules {
                            if let Some(ports) = rule.get("toPorts") {
                                if let Some(port_list) = ports.as_sequence() {
                                    for port_entry in port_list {
                                        if let Some(ports_inner) = port_entry.get("ports") {
                                            if let Some(port_seq) = ports_inner.as_sequence() {
                                                for p in port_seq {
                                                    let port = p
                                                        .get("port")
                                                        .and_then(|v| v.as_str())
                                                        .and_then(|s| s.parse::<u16>().ok())
                                                        .unwrap_or(0);
                                                    let protocol = p
                                                        .get("protocol")
                                                        .and_then(|v| v.as_str())
                                                        .map(
                                                            |s| if s == "UDP" { 17u8 } else { 6u8 },
                                                        )
                                                        .unwrap_or(6);
                                                    let decision = PolicyDecision {
                                                        src_identity: 0,
                                                        dst_identity: 0,
                                                        port,
                                                        protocol,
                                                        verdict: PolicyVerdict::Allow,
                                                    };
                                                    self.simulated_policies.push(decision.clone());
                                                    added.push(decision);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if !added.is_empty() {
                self.policy_sources.insert(source_key, added);
            }
        }

        Ok(())
    }

    /// Remove a policy by name, undoing the entries it added.
    fn remove_policy(&mut self, policy_name: &str, namespace: &str) -> Result<()> {
        let source_key = format!("{}/{}", namespace, policy_name);
        self.trace.push(format!("Removing policy: {}", source_key));

        let before = self.simulated_policies.len();

        if let Some(to_remove) = self.policy_sources.remove(&source_key) {
            // Remove matching entries from simulated_policies
            for target in &to_remove {
                if let Some(idx) = self.simulated_policies.iter().position(|p| {
                    p.src_identity == target.src_identity
                        && p.dst_identity == target.dst_identity
                        && p.port == target.port
                        && p.protocol == target.protocol
                        && p.verdict == target.verdict
                }) {
                    self.simulated_policies.remove(idx);
                }
            }
        }

        self.trace.push(format!(
            "Policies: {} → {}",
            before,
            self.simulated_policies.len()
        ));

        Ok(())
    }

    /// Block specific traffic
    fn block_traffic(
        &mut self,
        from_labels: &HashMap<String, String>,
        to_labels: &HashMap<String, String>,
        port: Option<&u16>,
        protocol: Option<&String>,
    ) -> Result<()> {
        self.trace.push(format!(
            "Blocking traffic: {:?} → {:?} port {:?} proto {:?}",
            from_labels, to_labels, port, protocol
        ));

        // Derive identity from labels using consistent hashing.
        // In production, the Simulator parent struct resolves identities via IPCache
        // before creating the engine; this hash serves as a deterministic fallback.
        let src_identity = Self::labels_to_identity(from_labels);
        let dst_identity = Self::labels_to_identity(to_labels);

        // Add deny rules
        if let Some(p) = port {
            let proto_opt = protocol.cloned();
            let proto = Self::protocol_to_number(&proto_opt);

            self.simulated_policies.push(PolicyDecision {
                src_identity,
                dst_identity,
                port: *p,
                protocol: proto,
                verdict: PolicyVerdict::Deny,
            });

            self.trace.push(format!(
                "Added DENY rule: {} → {} port {} proto {}",
                src_identity, dst_identity, p, proto
            ));
        } else {
            // Block all ports
            for p in [80, 443, 8080, 3000, 5432, 6379, 9200] {
                self.simulated_policies.push(PolicyDecision {
                    src_identity,
                    dst_identity,
                    port: p,
                    protocol: 6, // TCP
                    verdict: PolicyVerdict::Deny,
                });
            }
        }

        Ok(())
    }

    /// Allow specific traffic
    fn allow_traffic(
        &mut self,
        from_labels: &HashMap<String, String>,
        to_labels: &HashMap<String, String>,
        port: u16,
        protocol: &str,
    ) -> Result<()> {
        self.trace.push(format!(
            "Allowing traffic: {:?} → {:?} port {} proto {}",
            from_labels, to_labels, port, protocol
        ));

        let src_identity = Self::labels_to_identity(from_labels);
        let dst_identity = Self::labels_to_identity(to_labels);
        let proto = Self::protocol_to_number(&Some(protocol.to_string()));

        self.simulated_policies.push(PolicyDecision {
            src_identity,
            dst_identity,
            port,
            protocol: proto,
            verdict: PolicyVerdict::Allow,
        });

        Ok(())
    }

    /// Block external IP by adding deny rules for common ports
    fn block_external_ip(&mut self, ip: &IpAddr) -> Result<()> {
        self.trace.push(format!("Blocking external IP: {}", ip));

        // Use identity 0 as wildcard source (any internal endpoint)
        // and a deterministic identity for the external IP
        let ip_str = ip.to_string();
        let dst_identity = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            ip_str.hash(&mut hasher);
            (hasher.finish() % 10000) as u32
        };

        // Add deny rules for common ports to block traffic to this external IP
        let common_ports: &[(u16, u8)] =
            &[(80, 6), (443, 6), (8080, 6), (8443, 6), (53, 17), (53, 6)];

        for &(port, protocol) in common_ports {
            self.simulated_policies.push(PolicyDecision {
                src_identity: 0, // wildcard: any source
                dst_identity,
                port,
                protocol,
                verdict: PolicyVerdict::Deny,
            });
        }

        // Also add a catch-all deny for any traffic matching existing policies to this IP
        let overrides: Vec<PolicyDecision> = self
            .policies
            .iter()
            .filter(|p| p.dst_identity == dst_identity && p.verdict == PolicyVerdict::Allow)
            .map(|p| PolicyDecision {
                src_identity: p.src_identity,
                dst_identity,
                port: p.port,
                protocol: p.protocol,
                verdict: PolicyVerdict::Deny,
            })
            .collect();
        self.simulated_policies.extend(overrides);

        self.trace.push(format!(
            "Added deny rules for external IP {} (identity {})",
            ip, dst_identity
        ));

        Ok(())
    }

    /// Apply default-deny to namespace
    ///
    /// NOTE: PolicyDecision does not currently carry a namespace field, so we
    /// cannot filter removals by namespace alone. As a partial mitigation we
    /// only remove wildcard-allow rules whose src_identity matches an identity
    /// that was added via a policy scoped to this namespace (tracked in
    /// `policy_sources`). If no namespace-scoped entries are found we fall back
    /// to removing *all* wildcard-allow rules and log a warning.
    fn apply_default_deny(&mut self, namespace: &str) -> Result<()> {
        self.trace
            .push(format!("Applying default-deny to namespace: {}", namespace));

        // Collect identities that belong to the target namespace based on
        // policy_sources keyed as "namespace/name".
        let ns_identities: std::collections::HashSet<u32> = self
            .policy_sources
            .iter()
            .filter(|(key, _)| key.starts_with(&format!("{}/", namespace)))
            .flat_map(|(_, decisions)| decisions.iter().map(|d| d.src_identity))
            .collect();

        if ns_identities.is_empty() {
            // No namespace-specific identity information available – remove all
            // wildcard allow rules (previous behaviour) and document the gap.
            self.trace.push(format!(
                "No namespace-scoped identities found for '{}'; removing all wildcard allow rules",
                namespace
            ));
            self.simulated_policies
                .retain(|p| !(p.dst_identity == 0 && p.verdict == PolicyVerdict::Allow));
        } else {
            self.simulated_policies.retain(|p| {
                !(p.dst_identity == 0
                    && p.verdict == PolicyVerdict::Allow
                    && ns_identities.contains(&p.src_identity))
            });
        }

        self.trace.push(format!(
            "Removed wildcard allow rules for namespace '{}'",
            namespace
        ));

        Ok(())
    }

    /// Evaluate a flow against simulated policies
    pub fn evaluate_flow(&self, flow: &HistoricalFlow) -> PolicyVerdict {
        // Find matching policy decision
        for decision in &self.simulated_policies {
            if self.matches_flow(decision, flow) {
                return decision.verdict;
            }
        }

        // Default deny
        PolicyVerdict::Deny
    }

    /// Check if policy decision matches flow
    fn matches_flow(&self, decision: &PolicyDecision, flow: &HistoricalFlow) -> bool {
        // Exact match
        if decision.src_identity == flow.src_identity
            && decision.dst_identity == flow.dst_identity
            && decision.port == flow.port
            && decision.protocol == flow.protocol
        {
            return true;
        }

        // Wildcard destination match – also check port and protocol when specified
        if decision.src_identity == flow.src_identity
            && decision.dst_identity == 0
            && (decision.port == 0 || decision.port == flow.port)
            && (decision.protocol == 0 || decision.protocol == flow.protocol)
        {
            return true;
        }

        false
    }

    /// Get execution trace
    pub fn get_trace(&self) -> Vec<String> {
        self.trace.clone()
    }

    /// Convert labels to a deterministic identity via consistent hashing.
    /// The Simulator parent struct resolves real identities from IPCache when
    /// building HistoricalFlow records; this hash-based approach provides a
    /// stable fallback for label-based simulation scenarios.
    fn labels_to_identity(labels: &HashMap<String, String>) -> u32 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        // Sort for consistent hashing
        let mut sorted_labels: Vec<_> = labels.iter().collect();
        sorted_labels.sort_by_key(|k| k.0);

        for (k, v) in sorted_labels {
            k.hash(&mut hasher);
            v.hash(&mut hasher);
        }

        (hasher.finish() % 1_000_000) as u32
    }

    /// Convert protocol string to number
    fn protocol_to_number(protocol: &Option<String>) -> u8 {
        match protocol.as_ref().map(|s| s.as_str()) {
            Some("TCP") | Some("tcp") => 6,
            Some("UDP") | Some("udp") => 17,
            Some("ICMP") | Some("icmp") => 1,
            _ => 6, // Default to TCP
        }
    }

    /// Get policy count
    pub fn policy_count(&self) -> usize {
        self.simulated_policies.len()
    }

    /// Compare with original policies
    pub fn get_changes(&self) -> PolicyChanges {
        let added = self
            .simulated_policies
            .len()
            .saturating_sub(self.policies.len());
        let removed = self
            .policies
            .len()
            .saturating_sub(self.simulated_policies.len());

        // Count modifications: policies that share the same (src_identity, dst_identity)
        // tuple but have different (port, protocol, verdict).
        let mut modified = 0;
        for original in &self.policies {
            for simulated in &self.simulated_policies {
                if original.src_identity == simulated.src_identity
                    && original.dst_identity == simulated.dst_identity
                    && (original.port != simulated.port
                        || original.protocol != simulated.protocol
                        || original.verdict != simulated.verdict)
                {
                    modified += 1;
                    break; // Count each original policy at most once
                }
            }
        }

        PolicyChanges {
            added,
            removed,
            modified,
            total_before: self.policies.len(),
            total_after: self.simulated_policies.len(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PolicyChanges {
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
    pub total_before: usize,
    pub total_after: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_engine_creation() {
        let policies = vec![PolicyDecision {
            src_identity: 100,
            dst_identity: 200,
            port: 80,
            protocol: 6,
            verdict: PolicyVerdict::Allow,
        }];

        let engine = SimulationEngine::new(policies);
        assert_eq!(engine.policy_count(), 1);
    }

    #[test]
    fn test_block_traffic() {
        let policies = vec![];
        let mut engine = SimulationEngine::new(policies);

        let from_labels = HashMap::from([("app".to_string(), "web".to_string())]);
        let to_labels = HashMap::from([("app".to_string(), "db".to_string())]);

        engine
            .block_traffic(
                &from_labels,
                &to_labels,
                Some(&5432),
                Some(&"TCP".to_string()),
            )
            .unwrap();

        assert!(engine.policy_count() > 0);
    }

    #[test]
    fn test_evaluate_flow() {
        let policies = vec![PolicyDecision {
            src_identity: 100,
            dst_identity: 200,
            port: 80,
            protocol: 6,
            verdict: PolicyVerdict::Allow,
        }];

        let engine = SimulationEngine::new(policies);

        let flow = HistoricalFlow {
            src_identity: 100,
            dst_identity: 200,
            src_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            port: 80,
            protocol: 6,
            timestamp: 123456,
            verdict: PolicyVerdict::Allow,
        };

        let verdict = engine.evaluate_flow(&flow);
        assert_eq!(verdict, PolicyVerdict::Allow);
    }

    #[test]
    fn test_protocol_conversion() {
        assert_eq!(
            SimulationEngine::protocol_to_number(&Some("TCP".to_string())),
            6
        );
        assert_eq!(
            SimulationEngine::protocol_to_number(&Some("UDP".to_string())),
            17
        );
        assert_eq!(
            SimulationEngine::protocol_to_number(&Some("ICMP".to_string())),
            1
        );
    }

    #[test]
    fn test_protocol_conversion_lowercase() {
        assert_eq!(
            SimulationEngine::protocol_to_number(&Some("tcp".to_string())),
            6
        );
        assert_eq!(
            SimulationEngine::protocol_to_number(&Some("udp".to_string())),
            17
        );
        assert_eq!(
            SimulationEngine::protocol_to_number(&Some("icmp".to_string())),
            1
        );
    }

    #[test]
    fn test_protocol_conversion_default() {
        assert_eq!(SimulationEngine::protocol_to_number(&None), 6); // Default TCP
        assert_eq!(
            SimulationEngine::protocol_to_number(&Some("SCTP".to_string())),
            6
        ); // Unknown defaults to TCP
    }

    #[test]
    fn test_evaluate_flow_default_deny() {
        // Empty policies = default deny
        let engine = SimulationEngine::new(vec![]);

        let flow = HistoricalFlow {
            src_identity: 100,
            dst_identity: 200,
            src_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            port: 80,
            protocol: 6,
            timestamp: 123456,
            verdict: PolicyVerdict::Allow,
        };

        let verdict = engine.evaluate_flow(&flow);
        assert_eq!(verdict, PolicyVerdict::Deny); // Default deny
    }

    #[test]
    fn test_block_traffic_all_ports() {
        let mut engine = SimulationEngine::new(vec![]);

        let from_labels = HashMap::from([("app".to_string(), "web".to_string())]);
        let to_labels = HashMap::from([("app".to_string(), "db".to_string())]);

        // Block without specifying port - should block common ports
        engine
            .block_traffic(&from_labels, &to_labels, None, None)
            .unwrap();

        // Should have added deny rules for multiple common ports
        assert!(engine.policy_count() >= 7); // 80, 443, 8080, 3000, 5432, 6379, 9200
    }

    #[test]
    fn test_allow_traffic() {
        let mut engine = SimulationEngine::new(vec![]);

        let from_labels = HashMap::from([("app".to_string(), "web".to_string())]);
        let to_labels = HashMap::from([("app".to_string(), "api".to_string())]);

        engine
            .allow_traffic(&from_labels, &to_labels, 8080, "TCP")
            .unwrap();

        assert_eq!(engine.policy_count(), 1);
    }

    #[test]
    fn test_default_deny_scenario() {
        let policies = vec![PolicyDecision {
            src_identity: 0,
            dst_identity: 0, // Wildcard
            port: 80,
            protocol: 6,
            verdict: PolicyVerdict::Allow,
        }];

        let mut engine = SimulationEngine::new(policies);
        assert_eq!(engine.policy_count(), 1);

        engine.apply_default_deny("default").unwrap();

        // Wildcard allow rule should be removed
        assert_eq!(engine.policy_count(), 0);
    }

    #[test]
    fn test_add_policy_yaml() {
        let mut engine = SimulationEngine::new(vec![]);

        let yaml = r#"
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: allow-frontend
spec:
  endpointSelector:
    matchLabels:
      app: frontend
  ingress:
    - toPorts:
        - ports:
            - port: "80"
              protocol: TCP
"#;

        engine.add_policy(yaml, "default").unwrap();
        assert!(engine.policy_count() > 0);
    }

    #[test]
    fn test_add_policy_invalid_yaml() {
        let mut engine = SimulationEngine::new(vec![]);

        // Invalid YAML should not crash
        engine
            .add_policy("not: valid: yaml: {{", "default")
            .unwrap();
        assert_eq!(engine.policy_count(), 0);
    }

    #[test]
    fn test_get_changes() {
        let policies = vec![PolicyDecision {
            src_identity: 100,
            dst_identity: 200,
            port: 80,
            protocol: 6,
            verdict: PolicyVerdict::Allow,
        }];

        let mut engine = SimulationEngine::new(policies);

        let from_labels = HashMap::from([("app".to_string(), "web".to_string())]);
        let to_labels = HashMap::from([("app".to_string(), "db".to_string())]);
        engine
            .block_traffic(
                &from_labels,
                &to_labels,
                Some(&5432),
                Some(&"TCP".to_string()),
            )
            .unwrap();

        let changes = engine.get_changes();
        assert_eq!(changes.total_before, 1);
        assert_eq!(changes.total_after, 2);
        assert_eq!(changes.added, 1);
    }

    #[test]
    fn test_get_trace() {
        let mut engine = SimulationEngine::new(vec![]);

        let from_labels = HashMap::from([("app".to_string(), "web".to_string())]);
        let to_labels = HashMap::from([("app".to_string(), "db".to_string())]);
        engine
            .block_traffic(
                &from_labels,
                &to_labels,
                Some(&80),
                Some(&"TCP".to_string()),
            )
            .unwrap();

        let trace = engine.get_trace();
        assert!(!trace.is_empty());
        assert!(trace.iter().any(|t| t.contains("Blocking traffic")));
    }

    #[test]
    fn test_labels_to_identity_consistent() {
        let labels1 = HashMap::from([("app".to_string(), "web".to_string())]);
        let labels2 = HashMap::from([("app".to_string(), "web".to_string())]);
        let labels3 = HashMap::from([("app".to_string(), "api".to_string())]);

        // Same labels should produce same identity
        assert_eq!(
            SimulationEngine::labels_to_identity(&labels1),
            SimulationEngine::labels_to_identity(&labels2)
        );

        // Different labels should produce different identity
        assert_ne!(
            SimulationEngine::labels_to_identity(&labels1),
            SimulationEngine::labels_to_identity(&labels3)
        );
    }
}
