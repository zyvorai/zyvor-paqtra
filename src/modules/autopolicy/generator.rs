// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// Policy Generator
///
/// Generates CiliumNetworkPolicy resources from learned patterns
use super::*;
use crate::modules::{sanitize_k8s_name, yaml_escape};

pub struct PolicyGenerator {
    min_observations: u64,
    audit_mode: bool,
}

impl PolicyGenerator {
    pub fn new(min_observations: u64, audit_mode: bool) -> Self {
        Self {
            min_observations,
            audit_mode,
        }
    }

    /// Generate policies from observations
    pub fn generate(
        &self,
        observations: &HashMap<TrafficPattern, TrafficObservation>,
    ) -> Vec<GeneratedPolicy> {
        // Group by source
        let by_source = self.group_by_source(observations);

        let mut policies = Vec::new();

        for ((namespace, labels), patterns) in by_source {
            if let Ok(policy) = self.generate_for_source(&namespace, &labels, patterns) {
                policies.push(policy);
            }
        }

        policies
    }

    /// Group observations by source
    fn group_by_source<'a>(
        &self,
        observations: &'a HashMap<TrafficPattern, TrafficObservation>,
    ) -> HashMap<(String, LabelSet), Vec<&'a TrafficObservation>> {
        let mut grouped: HashMap<(String, LabelSet), Vec<&TrafficObservation>> = HashMap::new();

        for obs in observations.values() {
            if obs.count >= self.min_observations {
                let key = (
                    obs.pattern.src_namespace.clone(),
                    obs.pattern.src_labels.clone(),
                );
                grouped.entry(key).or_default().push(obs);
            }
        }

        grouped
    }

    /// Generate policy for a specific source
    fn generate_for_source(
        &self,
        namespace: &str,
        labels: &LabelSet,
        observations: Vec<&TrafficObservation>,
    ) -> Result<GeneratedPolicy> {
        let policy_name = self.generate_policy_name(namespace, labels);

        let yaml = self.build_policy_yaml(&policy_name, namespace, labels, &observations)?;

        let confidence = self.calculate_confidence(&observations);

        Ok(GeneratedPolicy {
            name: policy_name,
            namespace: namespace.to_string(),
            yaml,
            patterns: observations.iter().map(|o| o.pattern.clone()).collect(),
            confidence,
        })
    }

    /// Generate policy name
    fn generate_policy_name(&self, namespace: &str, labels: &LabelSet) -> String {
        if let Some(app) = labels.get("app") {
            format!("auto-{}-egress", app)
        } else if let Some(name) = labels.get("name") {
            format!("auto-{}-egress", name)
        } else {
            format!("auto-{}-egress", namespace)
        }
    }

    /// Build complete policy YAML
    fn build_policy_yaml(
        &self,
        name: &str,
        namespace: &str,
        labels: &LabelSet,
        observations: &[&TrafficObservation],
    ) -> Result<String> {
        let mut yaml = String::new();

        // Header
        yaml.push_str(&format!(
            r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: {}
  namespace: {}
  labels:
    generated-by: paqtra
    autopolicy: "true"
  annotations:
    description: "Auto-generated from {} traffic observations"
    audit-mode: "{}"
spec:
  endpointSelector:
    matchLabels:
"#,
            sanitize_k8s_name(name),
            sanitize_k8s_name(namespace),
            observations.len(),
            self.audit_mode
        ));

        // Selector labels
        for (k, v) in labels.iter() {
            yaml.push_str(&format!("      {}: {}\n", yaml_escape(k), yaml_escape(v)));
        }

        // Egress rules
        yaml.push_str("  egress:\n");

        // Group by destination
        let egress_rules = self.build_egress_rules(observations);
        yaml.push_str(&egress_rules);

        // Add deny-all at the end (zero-trust)
        if !self.audit_mode {
            yaml.push_str("\n  # Deny all other traffic (zero-trust)\n");
        }

        Ok(yaml)
    }

    /// Build egress rules
    fn build_egress_rules(&self, observations: &[&TrafficObservation]) -> String {
        // Group by destination
        let mut by_dest: HashMap<(String, LabelSet), Vec<(u16, Protocol)>> = HashMap::new();

        for obs in observations {
            let key = (
                obs.pattern.dst_namespace.clone(),
                obs.pattern.dst_labels.clone(),
            );
            by_dest
                .entry(key)
                .or_default()
                .push((obs.pattern.port, obs.pattern.protocol));
        }

        let mut rules = String::new();

        for ((dst_ns, dst_labels), ports) in by_dest {
            rules.push_str("    - toEndpoints:\n");

            if !dst_labels.is_empty() {
                rules.push_str("        - matchLabels:\n");
                for (k, v) in dst_labels.iter() {
                    rules.push_str(&format!(
                        "            {}: {}\n",
                        yaml_escape(k),
                        yaml_escape(v)
                    ));
                }
            } else {
                // Just namespace
                rules.push_str("        - matchLabels:\n");
                rules.push_str(&format!(
                    "            k8s:io.kubernetes.pod.namespace: {}\n",
                    yaml_escape(&dst_ns)
                ));
            }

            // Ports
            if !ports.is_empty() {
                rules.push_str("      toPorts:\n");

                let mut seen_ports = HashSet::new();

                for (port, proto) in ports {
                    let key = (port, proto);
                    if seen_ports.insert(key) {
                        rules.push_str("        - ports:\n");
                        rules.push_str(&format!("            - port: \"{}\"\n", port));
                        rules.push_str(&format!("              protocol: {}\n", proto));
                    }
                }
            }
        }

        rules
    }

    /// Calculate confidence score
    fn calculate_confidence(&self, observations: &[&TrafficObservation]) -> f32 {
        if observations.is_empty() {
            return 0.0;
        }

        let total_obs: u64 = observations.iter().map(|o| o.count).sum();

        // Confidence based on:
        // 1. Number of observations (more = better)
        let obs_score = (total_obs as f32 / (self.min_observations as f32 * 100.0)).clamp(0.5, 1.0);

        // 2. Pattern consistency (fewer unique patterns = more consistent)
        let pattern_score = if observations.len() < 10 { 1.0 } else { 0.5 };

        // 3. Time span (longer = better)
        let time_score = if observations.iter().any(|o| {
            o.last_seen - o.first_seen > 86400 // At least 1 day
        }) {
            1.0
        } else {
            0.7
        };

        // Weighted average
        (obs_score * 0.4 + pattern_score * 0.3 + time_score * 0.3).min(1.0)
    }

    /// Generate policy preview (without applying)
    pub fn preview_policy(
        &self,
        namespace: &str,
        labels: &LabelSet,
        observations: &[&TrafficObservation],
    ) -> Result<String> {
        let name = self.generate_policy_name(namespace, labels);
        self.build_policy_yaml(&name, namespace, labels, observations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generator_creation() {
        let generator = PolicyGenerator::new(10, true);
        assert_eq!(generator.min_observations, 10);
        assert!(generator.audit_mode);
    }

    #[test]
    fn test_policy_name_generation() {
        let generator = PolicyGenerator::new(10, true);

        let labels = LabelSet::new(HashMap::from([("app".to_string(), "web".to_string())]));
        let name = generator.generate_policy_name("default", &labels);
        assert_eq!(name, "auto-web-egress");
    }
}
