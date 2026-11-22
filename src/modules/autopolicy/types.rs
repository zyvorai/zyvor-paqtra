// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

use std::collections::HashMap;
use std::time::Duration;

/// LabelSet - A hashable, ordered set of labels
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct LabelSet {
    labels: Vec<(String, String)>,
}

impl LabelSet {
    pub fn new(labels: HashMap<String, String>) -> Self {
        let mut sorted: Vec<_> = labels.into_iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        Self { labels: sorted }
    }

    pub fn to_hashmap(&self) -> HashMap<String, String> {
        self.labels.iter().cloned().collect()
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.labels.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &(String, String)> {
        self.labels.iter()
    }
}

impl From<HashMap<String, String>> for LabelSet {
    fn from(map: HashMap<String, String>) -> Self {
        Self::new(map)
    }
}

/// AutoPolicy configuration
#[derive(Debug, Clone)]
pub struct AutoPolicyConfig {
    /// Enable learning
    pub enabled: bool,

    /// Learning duration (e.g., 7 days)
    pub learning_duration: Duration,

    /// Minimum observations before creating policy
    pub min_observations: u64,

    /// Generate policies automatically after learning
    pub auto_generate: bool,

    /// Apply policies automatically (dangerous!)
    pub auto_apply: bool,

    /// Enable audit mode (log but don't enforce)
    pub audit_mode: bool,

    /// Namespaces to learn from (empty = all)
    pub target_namespaces: Vec<String>,

    /// Update interval for learning
    pub update_interval_secs: u64,
}

impl Default for AutoPolicyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            learning_duration: Duration::from_secs(7 * 24 * 60 * 60), // 7 days
            min_observations: 10,
            auto_generate: false,
            auto_apply: false,
            audit_mode: true, // Safe default
            target_namespaces: vec![],
            update_interval_secs: 300, // 5 minutes
        }
    }
}

/// Learning state
#[derive(Debug, Clone, PartialEq)]
pub enum LearningState {
    NotStarted,
    Learning { started_at: u64, progress: f32 },
    Completed { learned_at: u64 },
    Generating,
    Generated { count: usize },
    Applied { count: usize },
}

/// Learned traffic pattern
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct TrafficPattern {
    pub src_namespace: String,
    pub src_labels: LabelSet,
    pub dst_namespace: String,
    pub dst_labels: LabelSet,
    pub port: u16,
    pub protocol: Protocol,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum Protocol {
    TCP,
    UDP,
    ICMP,
    Other(u8),
}

impl Protocol {
    pub fn from_number(proto: u8) -> Self {
        match proto {
            6 => Protocol::TCP,
            17 => Protocol::UDP,
            1 => Protocol::ICMP,
            _ => Protocol::Other(proto),
        }
    }
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::TCP => write!(f, "TCP"),
            Protocol::UDP => write!(f, "UDP"),
            Protocol::ICMP => write!(f, "ICMP"),
            Protocol::Other(n) => write!(f, "{}", n),
        }
    }
}

/// Traffic observation
#[derive(Debug, Clone)]
pub struct TrafficObservation {
    pub pattern: TrafficPattern,
    pub count: u64,
    pub first_seen: u64,
    pub last_seen: u64,
    pub bytes_transferred: u64,
}

/// Generated policy
#[derive(Debug, Clone)]
pub struct GeneratedPolicy {
    pub name: String,
    pub namespace: String,
    pub yaml: String,
    pub patterns: Vec<TrafficPattern>,
    pub confidence: f32, // 0.0 to 1.0
}

#[derive(Debug, Clone, Default)]
pub struct LearningStats {
    pub connections_observed: usize,
    pub patterns_learned: usize,
    pub unique_patterns: usize,
}

#[derive(Debug, Clone)]
pub struct AutoPolicyStats {
    pub state: LearningState,
    pub total_observations: u64,
    pub unique_patterns: usize,
    pub policies_generated: usize,
    pub avg_confidence: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_label_set_creation_keys_sorted() {
        let mut map = HashMap::new();
        map.insert("z-label".to_string(), "val-z".to_string());
        map.insert("a-label".to_string(), "val-a".to_string());
        map.insert("m-label".to_string(), "val-m".to_string());

        let label_set = LabelSet::new(map);
        let keys: Vec<&str> = label_set.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["a-label", "m-label", "z-label"]);
    }

    #[test]
    fn test_label_set_from_hashmap() {
        let mut map = HashMap::new();
        map.insert("app".to_string(), "web".to_string());
        map.insert("tier".to_string(), "frontend".to_string());

        let label_set: LabelSet = map.into();
        assert_eq!(label_set.get("app"), Some(&"web".to_string()));
        assert_eq!(label_set.get("tier"), Some(&"frontend".to_string()));
        assert_eq!(label_set.get("missing"), None);
        assert!(!label_set.is_empty());
    }

    #[test]
    fn test_auto_policy_config_defaults() {
        let config = AutoPolicyConfig::default();
        assert!(config.enabled);
        assert_eq!(
            config.learning_duration,
            Duration::from_secs(7 * 24 * 60 * 60)
        );
        assert_eq!(config.min_observations, 10);
        assert!(!config.auto_generate);
        assert!(!config.auto_apply);
        assert!(config.audit_mode);
        assert!(config.target_namespaces.is_empty());
        assert_eq!(config.update_interval_secs, 300);
    }

    #[test]
    fn test_learning_state_variants() {
        let not_started = LearningState::NotStarted;
        let learning = LearningState::Learning {
            started_at: 1000,
            progress: 0.5,
        };
        let completed = LearningState::Completed { learned_at: 2000 };
        let generating = LearningState::Generating;
        let generated = LearningState::Generated { count: 5 };
        let applied = LearningState::Applied { count: 3 };

        assert_eq!(not_started, LearningState::NotStarted);
        assert_ne!(not_started, generating);

        // Verify Learning state holds values
        if let LearningState::Learning {
            started_at,
            progress,
        } = learning
        {
            assert_eq!(started_at, 1000);
            assert!((progress - 0.5).abs() < f32::EPSILON);
        } else {
            panic!("Expected Learning variant");
        }

        if let LearningState::Completed { learned_at } = completed {
            assert_eq!(learned_at, 2000);
        }
        if let LearningState::Generated { count } = generated {
            assert_eq!(count, 5);
        }
        if let LearningState::Applied { count } = applied {
            assert_eq!(count, 3);
        }
    }

    #[test]
    fn test_protocol_from_number_and_to_string() {
        assert_eq!(Protocol::from_number(6), Protocol::TCP);
        assert_eq!(Protocol::from_number(17), Protocol::UDP);
        assert_eq!(Protocol::from_number(1), Protocol::ICMP);
        assert_eq!(Protocol::from_number(42), Protocol::Other(42));

        assert_eq!(Protocol::TCP.to_string(), "TCP");
        assert_eq!(Protocol::UDP.to_string(), "UDP");
        assert_eq!(Protocol::ICMP.to_string(), "ICMP");
        assert_eq!(Protocol::Other(42).to_string(), "42");

        // Round-trip: from_number -> to_string
        assert_eq!(Protocol::from_number(6).to_string(), "TCP");
        assert_eq!(Protocol::from_number(17).to_string(), "UDP");
        assert_eq!(Protocol::from_number(1).to_string(), "ICMP");
    }

    #[test]
    fn test_traffic_pattern_equality() {
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());

        let pattern1 = TrafficPattern {
            src_namespace: "default".to_string(),
            src_labels: LabelSet::new(labels.clone()),
            dst_namespace: "backend".to_string(),
            dst_labels: LabelSet::new(labels.clone()),
            port: 80,
            protocol: Protocol::TCP,
        };

        let pattern2 = TrafficPattern {
            src_namespace: "default".to_string(),
            src_labels: LabelSet::new(labels.clone()),
            dst_namespace: "backend".to_string(),
            dst_labels: LabelSet::new(labels.clone()),
            port: 80,
            protocol: Protocol::TCP,
        };

        assert_eq!(pattern1, pattern2);

        let pattern3 = TrafficPattern {
            src_namespace: "default".to_string(),
            src_labels: LabelSet::new(labels.clone()),
            dst_namespace: "backend".to_string(),
            dst_labels: LabelSet::new(labels.clone()),
            port: 443,
            protocol: Protocol::TCP,
        };

        assert_ne!(pattern1, pattern3);
    }

    #[test]
    fn test_generated_policy_creation() {
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "api".to_string());

        let pattern = TrafficPattern {
            src_namespace: "default".to_string(),
            src_labels: LabelSet::new(labels.clone()),
            dst_namespace: "backend".to_string(),
            dst_labels: LabelSet::new(labels.clone()),
            port: 8080,
            protocol: Protocol::TCP,
        };

        let policy = GeneratedPolicy {
            name: "allow-api-to-backend".to_string(),
            namespace: "default".to_string(),
            yaml: "apiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy".to_string(),
            patterns: vec![pattern],
            confidence: 0.95,
        };

        assert_eq!(policy.name, "allow-api-to-backend");
        assert_eq!(policy.namespace, "default");
        assert!(policy.yaml.contains("CiliumNetworkPolicy"));
        assert_eq!(policy.patterns.len(), 1);
        assert!(policy.confidence >= 0.0 && policy.confidence <= 1.0);
    }
}
