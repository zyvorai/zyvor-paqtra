// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Alert Management - Intelligent alerting with noise reduction
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use tokio::sync::RwLock;

use super::{Anomaly, Severity};

/// Manages alerts with intelligent deduplication and noise reduction
pub struct AlertManager {
    confidence_threshold: f64,
    alert_history: RwLock<Vec<Anomaly>>,
    suppression_rules: HashMap<String, SuppressionRule>,
}

#[derive(Debug, Clone)]
struct SuppressionRule {
    window_minutes: u64,
    max_alerts: usize,
}

impl AlertManager {
    pub fn new(confidence_threshold: f64) -> Self {
        let mut suppression_rules = HashMap::new();

        // Default suppression rules to prevent alert fatigue
        suppression_rules.insert(
            "traffic_spike".to_string(),
            SuppressionRule {
                window_minutes: 15,
                max_alerts: 3,
            },
        );

        suppression_rules.insert(
            "default".to_string(),
            SuppressionRule {
                window_minutes: 30,
                max_alerts: 5,
            },
        );

        Self {
            confidence_threshold,
            alert_history: RwLock::new(Vec::new()),
            suppression_rules,
        }
    }

    /// Send alerts with intelligent deduplication
    pub async fn send_alerts(&self, anomalies: &[Anomaly]) -> Result<()> {
        let mut history = self.alert_history.write().await;

        for anomaly in anomalies {
            // Check if alert should be suppressed
            if self.should_suppress(anomaly, &history) {
                tracing::debug!(
                    "Suppressing alert for {:?} (deduplication)",
                    anomaly.anomaly_type
                );
                continue;
            }

            // Send alert based on severity
            self.send_alert(anomaly).await?;

            // Add to history
            history.push(anomaly.clone());
        }

        // Clean old alerts (keep last 24 hours)
        let cutoff = Utc::now() - Duration::hours(24);
        history.retain(|a| a.timestamp > cutoff);

        Ok(())
    }

    async fn send_alert(&self, anomaly: &Anomaly) -> Result<()> {
        match anomaly.severity {
            Severity::Critical => {
                tracing::error!(
                    "[CRITICAL] ANOMALY: {:?} in {}/{}",
                    anomaly.anomaly_type,
                    anomaly.context.namespace,
                    anomaly.context.service
                );
                // In production: PagerDuty, Slack, email
            }
            Severity::High => {
                tracing::warn!(
                    "[WARNING] HIGH SEVERITY: {:?} in {}/{}",
                    anomaly.anomaly_type,
                    anomaly.context.namespace,
                    anomaly.context.service
                );
                // In production: Slack, email
            }
            Severity::Medium => {
                tracing::warn!("[MEDIUM] {:?} detected", anomaly.anomaly_type);
                // In production: Slack notification
            }
            Severity::Low | Severity::Info => {
                tracing::info!("[INFO] {:?} observed", anomaly.anomaly_type);
                // In production: Log aggregation only
            }
        }

        Ok(())
    }

    fn should_suppress(&self, anomaly: &Anomaly, history: &[Anomaly]) -> bool {
        let key =
            format!("{:?}", anomaly.anomaly_type)
                .chars()
                .fold(String::new(), |mut acc, c| {
                    if c.is_uppercase() && !acc.is_empty() {
                        acc.push('_');
                    }
                    acc.push(c.to_ascii_lowercase());
                    acc
                });
        let rule = self
            .suppression_rules
            .get(&key)
            .or_else(|| self.suppression_rules.get("default"))
            .unwrap();

        let window_start = Utc::now() - Duration::minutes(rule.window_minutes as i64);

        let similar_alerts = history
            .iter()
            .filter(|a| {
                a.timestamp > window_start
                    && a.anomaly_type == anomaly.anomaly_type
                    && a.context.namespace == anomaly.context.namespace
                    && a.context.service == anomaly.context.service
            })
            .count();

        similar_alerts >= rule.max_alerts
    }

    /// Get anomalies in a time range from the in-memory alert history.
    ///
    /// Queries the alert history buffer (last 24 hours) for anomalies
    /// whose timestamp falls within `[start, end]`.
    pub async fn get_anomalies_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Anomaly>> {
        let history = self.alert_history.read().await;
        let results: Vec<Anomaly> = history
            .iter()
            .filter(|a| a.timestamp >= start && a.timestamp <= end)
            .cloned()
            .collect();
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{AnomalyContext, AnomalyType, MetricType};
    use super::*;
    use chrono::Utc;

    fn make_anomaly(anomaly_type: AnomalyType, namespace: &str, service: &str) -> Anomaly {
        Anomaly {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            anomaly_type,
            severity: Severity::High,
            confidence: 0.9,
            metric: MetricType::RequestRate,
            baseline_value: 100.0,
            observed_value: 500.0,
            deviation: 4.0,
            context: AnomalyContext {
                namespace: namespace.to_string(),
                pod: "test-pod".to_string(),
                service: service.to_string(),
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
    fn test_alert_manager_creation_with_threshold() {
        let manager = AlertManager::new(0.85);
        assert_eq!(manager.confidence_threshold, 0.85);
        assert!(manager.suppression_rules.contains_key("traffic_spike"));
        assert!(manager.suppression_rules.contains_key("default"));
    }

    #[test]
    fn test_should_suppress_same_anomaly_within_window() {
        let manager = AlertManager::new(0.8);
        let anomaly = make_anomaly(AnomalyType::TrafficSpike, "default", "web");

        // Fill history with enough similar anomalies to trigger suppression.
        // The "traffic_spike" rule has max_alerts=3, so 5 similar anomalies
        // in the history window will trigger suppression.
        let history: Vec<Anomaly> = (0..5)
            .map(|_| make_anomaly(AnomalyType::TrafficSpike, "default", "web"))
            .collect();

        assert!(manager.should_suppress(&anomaly, &history));
    }

    #[test]
    fn test_should_suppress_returns_false_for_different_anomaly_types() {
        let manager = AlertManager::new(0.8);
        let anomaly = make_anomaly(AnomalyType::PortScan, "default", "web");

        // History has TrafficSpike anomalies, not PortScan
        let history: Vec<Anomaly> = (0..10)
            .map(|_| make_anomaly(AnomalyType::TrafficSpike, "default", "web"))
            .collect();

        assert!(!manager.should_suppress(&anomaly, &history));
    }

    #[test]
    fn test_should_suppress_returns_false_with_empty_history() {
        let manager = AlertManager::new(0.8);
        let anomaly = make_anomaly(AnomalyType::TrafficSpike, "default", "web");

        assert!(!manager.should_suppress(&anomaly, &[]));
    }

    #[tokio::test]
    async fn test_get_anomalies_in_range_returns_empty_without_alerts() {
        let manager = AlertManager::new(0.8);
        let start = Utc::now() - Duration::hours(1);
        let end = Utc::now();
        let result = manager.get_anomalies_in_range(start, end).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_get_anomalies_in_range_returns_matching_alerts() {
        let manager = AlertManager::new(0.8);
        let anomaly = make_anomaly(AnomalyType::TrafficSpike, "default", "web");

        // Send an alert to populate history
        manager.send_alerts(&[anomaly]).await.unwrap();

        // Query a range that includes now
        let start = Utc::now() - Duration::minutes(1);
        let end = Utc::now() + Duration::minutes(1);
        let result = manager.get_anomalies_in_range(start, end).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].context.namespace, "default");
    }

    #[tokio::test]
    async fn test_get_anomalies_in_range_excludes_out_of_range() {
        let manager = AlertManager::new(0.8);
        let anomaly = make_anomaly(AnomalyType::TrafficSpike, "default", "web");
        manager.send_alerts(&[anomaly]).await.unwrap();

        // Query a range in the past that won't include the alert
        let start = Utc::now() - Duration::hours(2);
        let end = Utc::now() - Duration::hours(1);
        let result = manager.get_anomalies_in_range(start, end).await.unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_should_suppress_returns_false_for_different_namespace() {
        let manager = AlertManager::new(0.8);
        let anomaly = make_anomaly(AnomalyType::TrafficSpike, "production", "web");

        // History has anomalies in "default" namespace, not "production"
        let history: Vec<Anomaly> = (0..10)
            .map(|_| make_anomaly(AnomalyType::TrafficSpike, "default", "web"))
            .collect();

        assert!(!manager.should_suppress(&anomaly, &history));
    }
}
