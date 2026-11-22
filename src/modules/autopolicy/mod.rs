// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// AutoPolicy Module - Zero-Trust Policy Learning
///
/// Automatically learns traffic patterns from eBPF and generates
/// minimal privilege network policies.
///
/// Features:
/// - Traffic pattern learning
/// - Communication graph building
/// - Minimal privilege policy generation
/// - Audit mode for safe testing
/// - Policy refinement over time
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::ebpf::{ConntrackEntry, EnrichedMapReader, MapReader};
use crate::kubernetes::K8sClient;
use crate::policies::PolicyManager;

pub mod types;
pub use types::*;

pub mod analyzer;
pub mod confidence;
pub mod generator;
pub mod learner;

/// AutoPolicy Engine
pub struct AutoPolicy<M: MapReader> {
    config: AutoPolicyConfig,
    ebpf_reader: M,
    k8s_client: K8sClient,
    policy_manager: PolicyManager,

    state: LearningState,
    observations: HashMap<TrafficPattern, TrafficObservation>,
    generated_policies: Vec<GeneratedPolicy>,

    learning_start: Option<u64>,
}

impl<M: MapReader> AutoPolicy<M> {
    pub fn new(config: AutoPolicyConfig, ebpf_reader: M, k8s_client: K8sClient) -> Self {
        let policy_manager = PolicyManager::new(k8s_client.clone());

        Self {
            config,
            ebpf_reader,
            k8s_client,
            policy_manager,
            state: LearningState::NotStarted,
            observations: HashMap::new(),
            generated_policies: Vec::new(),
            learning_start: None,
        }
    }

    /// Start learning phase
    pub async fn start_learning(&mut self) -> Result<()> {
        if !self.config.enabled {
            anyhow::bail!("AutoPolicy is not enabled");
        }

        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        self.learning_start = Some(now);
        self.state = LearningState::Learning {
            started_at: now,
            progress: 0.0,
        };

        println!("🔍 AutoPolicy learning started");
        println!(
            "   Duration: {} days",
            self.config.learning_duration.as_secs() / 86400
        );
        println!(
            "   Mode: {}",
            if self.config.audit_mode {
                "Audit"
            } else {
                "Enforce"
            }
        );

        Ok(())
    }

    /// Update learning with current traffic
    pub async fn update(&mut self) -> Result<LearningStats> {
        if !self.config.enabled {
            return Ok(LearningStats::default());
        }

        // Read current connections from eBPF
        let connections = self.ebpf_reader.read_conntrack_map()?;

        // Pre-read IPCache once for all IP resolutions in this update cycle
        let ipcache = self.ebpf_reader.read_ipcache_map().unwrap_or_default();

        let mut stats = LearningStats {
            connections_observed: connections.len(),
            ..Default::default()
        };

        // Process each connection
        for conn in &connections {
            if let Some(pattern) = self.connection_to_pattern(conn, &ipcache)? {
                self.record_observation(pattern);
                stats.patterns_learned += 1;
            }
        }

        // Update learning progress
        self.update_progress()?;

        // Check if learning is complete
        if self.is_learning_complete()? {
            println!("✅ Learning phase completed!");
            self.complete_learning()?;
        }

        stats.unique_patterns = self.observations.len();
        Ok(stats)
    }

    /// Resolve an IP address to namespace and labels via a pre-read IPCache
    fn resolve_ip(
        &self,
        ip: &str,
        ipcache: &[crate::ebpf::IPCacheEntry],
    ) -> (String, HashMap<String, String>) {
        if let Some(entry) = ipcache.iter().find(|e| e.ip == ip) {
            let namespace = if entry.namespace.is_empty() {
                "default".to_string()
            } else {
                entry.namespace.clone()
            };
            let labels: HashMap<String, String> = entry
                .labels
                .iter()
                .filter_map(|l| {
                    let parts: Vec<&str> = l.splitn(2, '=').collect();
                    if parts.len() == 2 {
                        Some((parts[0].to_string(), parts[1].to_string()))
                    } else {
                        None
                    }
                })
                .collect();
            return (namespace, labels);
        }
        ("unknown".to_string(), HashMap::new())
    }

    /// Convert connection to traffic pattern
    fn connection_to_pattern(
        &self,
        conn: &ConntrackEntry,
        ipcache: &[crate::ebpf::IPCacheEntry],
    ) -> Result<Option<TrafficPattern>> {
        let (src_namespace, src_labels) = self.resolve_ip(&conn.src_ip, ipcache);
        let (dst_namespace, dst_labels) = self.resolve_ip(&conn.dst_ip, ipcache);

        let pattern = TrafficPattern {
            src_namespace,
            src_labels: LabelSet::new(src_labels),
            dst_namespace,
            dst_labels: LabelSet::new(dst_labels),
            port: conn.dst_port,
            protocol: Protocol::from_number(conn.protocol),
        };

        Ok(Some(pattern))
    }

    /// Record traffic observation
    fn record_observation(&mut self, pattern: TrafficPattern) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some(obs) = self.observations.get_mut(&pattern) {
            obs.count += 1;
            obs.last_seen = now;
        } else {
            // Cap observations to prevent unbounded growth
            if self.observations.len() >= 50_000 {
                return;
            }
            self.observations.insert(
                pattern.clone(),
                TrafficObservation {
                    pattern,
                    count: 1,
                    first_seen: now,
                    last_seen: now,
                    bytes_transferred: 0,
                },
            );
        }
    }

    /// Update learning progress
    fn update_progress(&mut self) -> Result<()> {
        if let Some(start) = self.learning_start {
            let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

            let elapsed = now - start;
            let total = self.config.learning_duration.as_secs();
            let progress = if total == 0 {
                1.0
            } else {
                (elapsed as f32 / total as f32).min(1.0)
            };

            self.state = LearningState::Learning {
                started_at: start,
                progress,
            };
        }

        Ok(())
    }

    /// Check if learning phase is complete
    fn is_learning_complete(&self) -> Result<bool> {
        if let Some(start) = self.learning_start {
            let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

            let elapsed = now - start;
            Ok(elapsed >= self.config.learning_duration.as_secs())
        } else {
            Ok(false)
        }
    }

    /// Complete learning phase
    fn complete_learning(&mut self) -> Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        self.state = LearningState::Completed { learned_at: now };

        // Auto-generate if configured
        if self.config.auto_generate {
            self.generate_policies()?;
        }

        Ok(())
    }

    /// Generate policies from learned patterns
    pub fn generate_policies(&mut self) -> Result<Vec<GeneratedPolicy>> {
        self.state = LearningState::Generating;

        let mut policies = Vec::new();

        // Group patterns by source namespace/labels
        let mut by_source: HashMap<(String, LabelSet), Vec<&TrafficObservation>> = HashMap::new();

        for obs in self.observations.values() {
            // Only include patterns with sufficient observations
            if obs.count >= self.config.min_observations {
                let key = (
                    obs.pattern.src_namespace.clone(),
                    obs.pattern.src_labels.clone(),
                );
                by_source.entry(key).or_default().push(obs);
            }
        }

        // Generate one policy per source
        for ((namespace, labels), observations) in by_source {
            let policy = self.generate_policy_for_source(&namespace, &labels, observations)?;
            policies.push(policy);
        }

        self.generated_policies = policies.clone();
        self.state = LearningState::Generated {
            count: policies.len(),
        };

        println!("✅ Generated {} policies", policies.len());

        Ok(policies)
    }

    /// Generate policy for a specific source
    fn generate_policy_for_source(
        &self,
        namespace: &str,
        labels: &LabelSet,
        observations: Vec<&TrafficObservation>,
    ) -> Result<GeneratedPolicy> {
        // Build egress rules
        let mut egress_rules = Vec::new();

        // Group by destination
        let mut by_dest: HashMap<(String, LabelSet), Vec<(u16, Protocol)>> = HashMap::new();

        for obs in &observations {
            let key = (
                obs.pattern.dst_namespace.clone(),
                obs.pattern.dst_labels.clone(),
            );
            by_dest
                .entry(key)
                .or_default()
                .push((obs.pattern.port, obs.pattern.protocol));
        }

        // Create egress rules
        for ((dst_ns, dst_labels), ports) in by_dest {
            let rule = self.format_egress_rule(&dst_ns, &dst_labels, &ports);
            egress_rules.push(rule);
        }

        // Generate policy name
        let policy_name = if let Some(app) = labels.get("app") {
            crate::modules::sanitize_k8s_name(&format!("auto-policy-{}", app))
        } else {
            crate::modules::sanitize_k8s_name(&format!("auto-policy-{}", namespace))
        };

        // Build complete YAML
        let yaml = format!(
            r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: {}
  namespace: {}
  labels:
    generated-by: paqtra
    autopolicy: "true"
spec:
  endpointSelector:
    matchLabels:
{}
  egress:
{}
"#,
            policy_name,
            crate::modules::yaml_escape(namespace),
            self.format_labels(labels, 6),
            egress_rules.join("\n"),
        );

        let confidence = self.calculate_confidence(&observations);

        Ok(GeneratedPolicy {
            name: policy_name,
            namespace: namespace.to_string(),
            yaml,
            patterns: observations.iter().map(|o| o.pattern.clone()).collect(),
            confidence,
        })
    }

    /// Format egress rule
    fn format_egress_rule(
        &self,
        dst_namespace: &str,
        dst_labels: &LabelSet,
        ports: &[(u16, Protocol)],
    ) -> String {
        let mut rule = String::new();

        // Add destination selector
        rule.push_str("    - toEndpoints:\n");
        rule.push_str("        - matchLabels:\n");

        if !dst_labels.is_empty() {
            for (k, v) in dst_labels.iter() {
                rule.push_str(&format!(
                    "            {}: \"{}\"\n",
                    crate::modules::sanitize_k8s_name(k),
                    crate::modules::yaml_escape(v)
                ));
            }
        } else {
            rule.push_str(&format!(
                "            k8s:io.kubernetes.pod.namespace: \"{}\"\n",
                crate::modules::yaml_escape(dst_namespace)
            ));
        }

        // Add ports
        if !ports.is_empty() {
            rule.push_str("      toPorts:\n");

            // Group by protocol
            let mut tcp_ports = Vec::new();
            let mut udp_ports = Vec::new();

            for (port, proto) in ports {
                match proto {
                    Protocol::TCP => tcp_ports.push(*port),
                    Protocol::UDP => udp_ports.push(*port),
                    _ => {}
                }
            }

            if !tcp_ports.is_empty() {
                rule.push_str("        - ports:\n");
                for port in tcp_ports {
                    rule.push_str(&format!("            - port: \"{}\"\n", port));
                    rule.push_str("              protocol: TCP\n");
                }
            }

            if !udp_ports.is_empty() {
                rule.push_str("        - ports:\n");
                for port in udp_ports {
                    rule.push_str(&format!("            - port: \"{}\"\n", port));
                    rule.push_str("              protocol: UDP\n");
                }
            }
        }

        rule
    }

    /// Format labels for YAML
    fn format_labels(&self, labels: &LabelSet, indent: usize) -> String {
        let spaces = " ".repeat(indent);
        labels
            .iter()
            .map(|(k, v)| {
                format!(
                    "{}{}: \"{}\"",
                    spaces,
                    crate::modules::sanitize_k8s_name(k),
                    crate::modules::yaml_escape(v)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Calculate confidence score
    fn calculate_confidence(&self, observations: &[&TrafficObservation]) -> f32 {
        if observations.is_empty() {
            return 0.0;
        }

        let total_obs: u64 = observations.iter().map(|o| o.count).sum();

        // More observations = higher confidence
        let obs_score = (total_obs as f32 / (self.config.min_observations as f32 * 10.0)).min(1.0);

        // More patterns = lower confidence (more complex)
        let pattern_score = 1.0 / (observations.len() as f32).sqrt();

        // Combined score
        (obs_score * 0.7 + pattern_score * 0.3).min(1.0)
    }

    /// Apply generated policies
    pub async fn apply_policies(&mut self) -> Result<usize> {
        if !self.config.auto_apply {
            return Ok(0);
        }

        let mut applied = 0;

        for policy in &self.generated_policies {
            // Apply via kubectl
            if self.apply_policy_yaml(&policy.yaml).await.is_ok() {
                applied += 1;
                println!(
                    "✅ Applied policy: {} (confidence: {:.0}%)",
                    policy.name,
                    policy.confidence * 100.0
                );
            }
        }

        self.state = LearningState::Applied { count: applied };

        Ok(applied)
    }

    /// Apply policy YAML
    async fn apply_policy_yaml(&self, yaml: &str) -> Result<()> {
        self.k8s_client.apply_custom_resource(None, yaml).await
    }

    /// Get current state
    pub fn state(&self) -> &LearningState {
        &self.state
    }

    /// Get observations
    pub fn observations(&self) -> &HashMap<TrafficPattern, TrafficObservation> {
        &self.observations
    }

    /// Get generated policies
    pub fn policies(&self) -> &[GeneratedPolicy] {
        &self.generated_policies
    }

    /// Get statistics
    pub fn stats(&self) -> AutoPolicyStats {
        AutoPolicyStats {
            state: self.state.clone(),
            total_observations: self.observations.values().map(|o| o.count).sum(),
            unique_patterns: self.observations.len(),
            policies_generated: self.generated_policies.len(),
            avg_confidence: if !self.generated_policies.is_empty() {
                self.generated_policies
                    .iter()
                    .map(|p| p.confidence)
                    .sum::<f32>()
                    / self.generated_policies.len() as f32
            } else {
                0.0
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ebpf::MockMapReader;

    #[tokio::test]
    #[ignore] // Requires a live Kubernetes cluster
    async fn test_autopolicy_creation() {
        let config = AutoPolicyConfig::default();
        let reader = MockMapReader;
        let k8s_client = K8sClient::new().await.unwrap();

        let autopolicy = AutoPolicy::new(config, reader, k8s_client);
        assert_eq!(autopolicy.state, LearningState::NotStarted);
    }

    #[tokio::test]
    #[ignore] // Requires a live Kubernetes cluster
    async fn test_start_learning() {
        let config = AutoPolicyConfig::default();
        let reader = MockMapReader;
        let k8s_client = K8sClient::new().await.unwrap();

        let mut autopolicy = AutoPolicy::new(config, reader, k8s_client);
        autopolicy.start_learning().await.unwrap();

        assert!(matches!(autopolicy.state, LearningState::Learning { .. }));
    }

    #[test]
    fn test_protocol_conversion() {
        assert_eq!(Protocol::from_number(6), Protocol::TCP);
        assert_eq!(Protocol::from_number(17), Protocol::UDP);
        assert_eq!(Protocol::from_number(1), Protocol::ICMP);
    }
}

/// Enhanced implementation for EnrichedMapReader
impl AutoPolicy<EnrichedMapReader> {
    /// Update learning with enriched traffic (includes pod labels)
    pub async fn update_enriched(&mut self) -> Result<LearningStats> {
        if !self.config.enabled {
            return Ok(LearningStats::default());
        }

        // Read enriched connections from eBPF
        let enriched_connections = self.ebpf_reader.read_enriched_connections()?;

        let mut stats = LearningStats {
            connections_observed: enriched_connections.len(),
            ..Default::default()
        };

        // Process each enriched connection
        for (conn, info) in &enriched_connections {
            if let Some(pattern) = self.enriched_connection_to_pattern(conn, info)? {
                self.record_observation(pattern);
                stats.patterns_learned += 1;
            }
        }

        // Update learning progress
        self.update_progress()?;

        // Check if learning is complete
        if self.is_learning_complete()? {
            println!("✅ Learning phase completed!");
            self.complete_learning()?;
        }

        stats.unique_patterns = self.observations.len();
        Ok(stats)
    }

    /// Convert enriched connection to traffic pattern with real labels
    fn enriched_connection_to_pattern(
        &self,
        conn: &ConntrackEntry,
        info: &crate::ebpf::EnrichedConnectionInfo,
    ) -> Result<Option<TrafficPattern>> {
        // Extract labels into HashMap
        let src_labels_map = self.labels_vec_to_map(&info.src_labels);
        let dst_labels_map = self.labels_vec_to_map(&info.dst_labels);

        let pattern = TrafficPattern {
            src_namespace: info
                .src_namespace
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            src_labels: LabelSet::new(src_labels_map),
            dst_namespace: info
                .dst_namespace
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            dst_labels: LabelSet::new(dst_labels_map),
            port: conn.dst_port,
            protocol: Protocol::from_number(conn.protocol),
        };

        Ok(Some(pattern))
    }

    /// Convert label vector to HashMap
    fn labels_vec_to_map(&self, labels: &[String]) -> HashMap<String, String> {
        labels
            .iter()
            .filter_map(|label| {
                let parts: Vec<&str> = label.splitn(2, '=').collect();
                if parts.len() == 2 {
                    Some((parts[0].to_string(), parts[1].to_string()))
                } else {
                    None
                }
            })
            .collect()
    }
}
