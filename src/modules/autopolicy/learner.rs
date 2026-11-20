// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// Traffic Learning Engine
///
/// Learns traffic patterns over time by observing connections
use super::*;
use std::collections::HashMap;

pub struct TrafficLearner {
    observations: HashMap<TrafficPattern, TrafficObservation>,
    start_time: u64,
    min_observations: u64,
}

impl TrafficLearner {
    pub fn new(min_observations: u64) -> Self {
        let start_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            observations: HashMap::new(),
            start_time,
            min_observations,
        }
    }

    /// Record a connection observation
    pub fn observe(&mut self, pattern: TrafficPattern, bytes: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some(obs) = self.observations.get_mut(&pattern) {
            obs.count += 1;
            obs.last_seen = now;
            obs.bytes_transferred += bytes;
        } else {
            self.observations.insert(
                pattern.clone(),
                TrafficObservation {
                    pattern,
                    count: 1,
                    first_seen: now,
                    last_seen: now,
                    bytes_transferred: bytes,
                },
            );
        }
    }

    /// Get significant patterns (with enough observations)
    pub fn significant_patterns(&self) -> Vec<&TrafficObservation> {
        self.observations
            .values()
            .filter(|obs| obs.count >= self.min_observations)
            .collect()
    }

    /// Get all observations
    pub fn all_observations(&self) -> &HashMap<TrafficPattern, TrafficObservation> {
        &self.observations
    }

    /// Get learning duration
    pub fn learning_duration(&self) -> Duration {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Duration::from_secs(now - self.start_time)
    }

    /// Analyze traffic by namespace
    pub fn by_namespace(&self) -> HashMap<String, Vec<&TrafficObservation>> {
        let mut by_ns = HashMap::new();

        for obs in self.observations.values() {
            by_ns
                .entry(obs.pattern.src_namespace.clone())
                .or_insert_with(Vec::new)
                .push(obs);
        }

        by_ns
    }

    /// Analyze traffic by application
    pub fn by_application(&self) -> HashMap<String, Vec<&TrafficObservation>> {
        let mut by_app = HashMap::new();

        for obs in self.observations.values() {
            if let Some(app) = obs.pattern.src_labels.get("app") {
                by_app.entry(app.clone()).or_insert_with(Vec::new).push(obs);
            }
        }

        by_app
    }

    /// Get top communicating pairs
    pub fn top_pairs(&self, limit: usize) -> Vec<&TrafficObservation> {
        let mut observations: Vec<&TrafficObservation> = self.observations.values().collect();
        observations.sort_by_key(|a| std::cmp::Reverse(a.count));
        observations.into_iter().take(limit).collect()
    }

    /// Get statistics
    pub fn stats(&self) -> LearnerStats {
        LearnerStats {
            total_patterns: self.observations.len(),
            significant_patterns: self.significant_patterns().len(),
            total_observations: self.observations.values().map(|o| o.count).sum(),
            learning_duration: self.learning_duration(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LearnerStats {
    pub total_patterns: usize,
    pub significant_patterns: usize,
    pub total_observations: u64,
    pub learning_duration: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_learner_creation() {
        let learner = TrafficLearner::new(10);
        assert_eq!(learner.observations.len(), 0);
    }

    #[test]
    fn test_observe_pattern() {
        let mut learner = TrafficLearner::new(10);

        let pattern = TrafficPattern {
            src_namespace: "default".to_string(),
            src_labels: LabelSet::new(HashMap::from([("app".to_string(), "web".to_string())])),
            dst_namespace: "default".to_string(),
            dst_labels: LabelSet::new(HashMap::from([("app".to_string(), "db".to_string())])),
            port: 5432,
            protocol: Protocol::TCP,
        };

        learner.observe(pattern.clone(), 1024);
        learner.observe(pattern.clone(), 2048);

        assert_eq!(learner.observations.len(), 1);
        let obs = learner.observations.get(&pattern).unwrap();
        assert_eq!(obs.count, 2);
        assert_eq!(obs.bytes_transferred, 3072);
    }
}
