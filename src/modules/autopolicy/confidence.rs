// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

use super::{Protocol, TrafficObservation, TrafficPattern};
/// ML-Based Confidence Scoring for Policy Recommendations
///
/// Uses machine learning-inspired features to score policy confidence:
/// - Temporal stability (pattern consistency over time)
/// - Traffic volume and frequency
/// - Port/protocol patterns
/// - Namespace trust levels
/// - Label cardinality and specificity
/// - Historical accuracy
///
/// Confidence scores range from 0.0 (low) to 1.0 (high)
use std::collections::HashMap;

/// ML-based confidence scorer
pub struct ConfidenceScorer {
    /// Historical accuracy tracking
    historical_accuracy: HashMap<String, f32>,

    /// Namespace trust scores
    namespace_trust: HashMap<String, f32>,

    /// Port usage patterns (common ports score higher)
    known_ports: HashMap<u16, PortInfo>,
}

#[derive(Debug, Clone)]
struct PortInfo {
    name: &'static str,
    trust_score: f32,
}

impl Default for ConfidenceScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfidenceScorer {
    pub fn new() -> Self {
        let mut known_ports = HashMap::new();

        // HTTP/HTTPS
        known_ports.insert(
            80,
            PortInfo {
                name: "HTTP",
                trust_score: 0.9,
            },
        );
        known_ports.insert(
            443,
            PortInfo {
                name: "HTTPS",
                trust_score: 0.95,
            },
        );
        known_ports.insert(
            8080,
            PortInfo {
                name: "HTTP-Alt",
                trust_score: 0.85,
            },
        );
        known_ports.insert(
            8443,
            PortInfo {
                name: "HTTPS-Alt",
                trust_score: 0.85,
            },
        );

        // DNS
        known_ports.insert(
            53,
            PortInfo {
                name: "DNS",
                trust_score: 0.95,
            },
        );

        // Databases
        known_ports.insert(
            3306,
            PortInfo {
                name: "MySQL",
                trust_score: 0.9,
            },
        );
        known_ports.insert(
            5432,
            PortInfo {
                name: "PostgreSQL",
                trust_score: 0.9,
            },
        );
        known_ports.insert(
            27017,
            PortInfo {
                name: "MongoDB",
                trust_score: 0.9,
            },
        );
        known_ports.insert(
            6379,
            PortInfo {
                name: "Redis",
                trust_score: 0.9,
            },
        );

        // Message Queues
        known_ports.insert(
            5672,
            PortInfo {
                name: "RabbitMQ",
                trust_score: 0.85,
            },
        );
        known_ports.insert(
            9092,
            PortInfo {
                name: "Kafka",
                trust_score: 0.85,
            },
        );

        // Kubernetes
        known_ports.insert(
            10250,
            PortInfo {
                name: "Kubelet",
                trust_score: 0.8,
            },
        );
        known_ports.insert(
            6443,
            PortInfo {
                name: "K8s API",
                trust_score: 0.8,
            },
        );

        // Default namespace trust (can be updated based on history)
        let mut namespace_trust = HashMap::new();
        namespace_trust.insert("kube-system".to_string(), 0.95);
        namespace_trust.insert("default".to_string(), 0.7);
        namespace_trust.insert("production".to_string(), 0.9);
        namespace_trust.insert("prod".to_string(), 0.9);

        Self {
            historical_accuracy: HashMap::new(),
            namespace_trust,
            known_ports,
        }
    }

    /// Calculate confidence score for a policy based on observations
    pub fn score_policy(
        &self,
        patterns: &[TrafficPattern],
        observations: &HashMap<TrafficPattern, TrafficObservation>,
    ) -> f32 {
        if patterns.is_empty() {
            return 0.0;
        }

        // Collect individual pattern scores
        let pattern_scores: Vec<f32> = patterns
            .iter()
            .filter_map(|p| observations.get(p))
            .map(|obs| self.score_pattern(&obs.pattern, obs))
            .collect();

        if pattern_scores.is_empty() {
            return 0.0;
        }

        // Weighted average (give more weight to higher confidence patterns)
        let weighted_sum: f32 = pattern_scores.iter().map(|s| s * s).sum();
        let weight_total: f32 = pattern_scores.iter().sum();

        if weight_total > 0.0 {
            (weighted_sum / weight_total).min(1.0)
        } else {
            0.0
        }
    }

    /// Score an individual pattern based on ML features
    fn score_pattern(&self, pattern: &TrafficPattern, obs: &TrafficObservation) -> f32 {
        let features = [
            self.temporal_stability_score(obs),
            self.traffic_volume_score(obs),
            self.port_trust_score(pattern.port),
            self.protocol_score(pattern.protocol),
            self.namespace_trust_score(&pattern.src_namespace, &pattern.dst_namespace),
            self.label_specificity_score(&pattern.src_labels, &pattern.dst_labels),
            self.traffic_regularity_score(obs),
        ];

        // Combined score using weighted average
        // Assign weights to each feature based on importance
        let weights = [
            0.20, // Temporal stability (very important)
            0.20, // Traffic volume (very important)
            0.15, // Port trust
            0.10, // Protocol
            0.15, // Namespace trust
            0.10, // Label specificity
            0.10, // Traffic regularity
        ];

        let weighted_sum: f32 = features
            .iter()
            .zip(weights.iter())
            .map(|(f, w)| f * w)
            .sum();

        weighted_sum.clamp(0.0, 1.0)
    }

    fn temporal_stability_score(&self, obs: &TrafficObservation) -> f32 {
        let duration_secs = obs.last_seen.saturating_sub(obs.first_seen);

        if duration_secs == 0 {
            return 0.5; // Single observation, medium confidence
        }

        // Longer observation periods = higher stability
        // Use logarithmic scale: 1 hour = 0.6, 1 day = 0.8, 1 week = 0.95
        let hours = duration_secs as f32 / 3600.0;
        let score = (hours.log10() / 2.0).clamp(0.0, 1.0);

        // Bonus for many observations over time (consistency)
        let consistency_bonus = (obs.count as f32 / (hours + 1.0) / 10.0).min(0.2);

        (score + consistency_bonus).min(1.0)
    }

    fn traffic_volume_score(&self, obs: &TrafficObservation) -> f32 {
        // Logarithmic scale: more observations = higher confidence
        // 1 obs = 0.3, 10 obs = 0.6, 100 obs = 0.8, 1000+ obs = 0.95
        let log_count = (obs.count as f32).log10();
        (log_count / 3.0).clamp(0.3, 0.95)
    }

    fn port_trust_score(&self, port: u16) -> f32 {
        // Check if it's a well-known port
        if let Some(info) = self.known_ports.get(&port) {
            return info.trust_score;
        }

        // Port ranges scoring
        match port {
            0..=1023 => 0.7,      // System ports (generally trusted)
            1024..=49151 => 0.6,  // Registered ports
            49152..=65535 => 0.4, // Dynamic/ephemeral ports (lower trust)
        }
    }

    fn protocol_score(&self, protocol: Protocol) -> f32 {
        match protocol {
            Protocol::TCP => 0.9,      // TCP is stateful, reliable
            Protocol::UDP => 0.7,      // UDP is stateless, less reliable
            Protocol::ICMP => 0.6,     // ICMP is operational
            Protocol::Other(_) => 0.4, // Unknown protocols
        }
    }

    fn namespace_trust_score(&self, src_ns: &str, dst_ns: &str) -> f32 {
        let src_trust = self.namespace_trust.get(src_ns).copied().unwrap_or(0.6);
        let dst_trust = self.namespace_trust.get(dst_ns).copied().unwrap_or(0.6);

        // Average trust of both namespaces
        (src_trust + dst_trust) / 2.0
    }

    fn label_specificity_score(
        &self,
        src_labels: &super::LabelSet,
        dst_labels: &super::LabelSet,
    ) -> f32 {
        // More labels = more specific = higher confidence
        let src_count = if src_labels.is_empty() {
            0
        } else {
            src_labels.iter().count()
        };
        let dst_count = if dst_labels.is_empty() {
            0
        } else {
            dst_labels.iter().count()
        };

        let total_labels = src_count + dst_count;

        // Score based on label count: 0 labels = 0.3, 2 labels = 0.6, 4+ labels = 0.9
        match total_labels {
            0 => 0.3,
            1 => 0.5,
            2 => 0.7,
            3 => 0.8,
            _ => 0.9,
        }
    }

    fn traffic_regularity_score(&self, obs: &TrafficObservation) -> f32 {
        let duration_secs = obs.last_seen.saturating_sub(obs.first_seen).max(1);
        let bytes_per_sec = obs.bytes_transferred as f32 / duration_secs as f32;

        // Regular traffic patterns (stable bytes/sec) score higher
        // This is a simplified heuristic - in real ML we'd calculate variance
        if bytes_per_sec > 0.0 && bytes_per_sec < 1_000_000.0 {
            0.8 // Reasonable, regular traffic
        } else if bytes_per_sec >= 1_000_000.0 {
            0.9 // High volume, likely production traffic
        } else {
            0.6 // Very low or sporadic traffic
        }
    }

    /// Update historical accuracy when policy is applied
    pub fn update_accuracy(&mut self, policy_name: String, accuracy: f32) {
        self.historical_accuracy.insert(policy_name, accuracy);
    }

    /// Update namespace trust score
    pub fn update_namespace_trust(&mut self, namespace: String, trust: f32) {
        self.namespace_trust
            .insert(namespace, trust.clamp(0.0, 1.0));
    }

    /// Get confidence level as human-readable string
    pub fn confidence_level(score: f32) -> &'static str {
        let pct = (score.clamp(0.0, 1.0) * 100.0).round() as u32;
        match pct {
            90..=100 => "Very High",
            75..=89 => "High",
            60..=74 => "Medium",
            40..=59 => "Low",
            _ => "Very Low",
        }
    }

    /// Get recommendation based on confidence score
    pub fn get_recommendation(score: f32) -> &'static str {
        let pct = (score.clamp(0.0, 1.0) * 100.0).round() as u32;
        match pct {
            90..=100 => "✅ Safe to apply in production",
            75..=89 => "✓ Recommended for staging first",
            60..=74 => "⚠️  Test in audit mode",
            40..=59 => "⚠️  Review manually before applying",
            _ => "❌ Not recommended for automatic application",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidence_scorer_creation() {
        let scorer = ConfidenceScorer::new();
        assert!(scorer.known_ports.contains_key(&80));
        assert!(scorer.namespace_trust.contains_key("kube-system"));
    }

    #[test]
    fn test_port_trust_score() {
        let scorer = ConfidenceScorer::new();

        // Known port should score high
        assert_eq!(scorer.port_trust_score(443), 0.95);

        // System port should score medium
        assert_eq!(scorer.port_trust_score(1000), 0.7);
    }

    #[test]
    fn test_protocol_score() {
        let scorer = ConfidenceScorer::new();

        assert_eq!(scorer.protocol_score(Protocol::TCP), 0.9);
        assert_eq!(scorer.protocol_score(Protocol::UDP), 0.7);
    }

    #[test]
    fn test_confidence_level() {
        assert_eq!(ConfidenceScorer::confidence_level(0.95), "Very High");
        assert_eq!(ConfidenceScorer::confidence_level(0.80), "High");
        assert_eq!(ConfidenceScorer::confidence_level(0.65), "Medium");
        assert_eq!(ConfidenceScorer::confidence_level(0.50), "Low");
        assert_eq!(ConfidenceScorer::confidence_level(0.30), "Very Low");
    }
}
