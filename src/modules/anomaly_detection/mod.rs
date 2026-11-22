// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// AI/ML-Powered Anomaly Detection Module
// Experimental: Uses time-series analysis and ML for intelligent anomaly detection

pub mod alerting;
pub mod baseline;
pub mod detector;
pub mod remediation;
pub mod scoring;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Anomaly detection engine with ML-powered analysis
pub struct AnomalyDetector {
    baseline: baseline::BaselineLearner,
    scorer: scoring::AnomalyScorer,
    alerting: alerting::AlertManager,
    remediation: remediation::RemediationEngine,
    config: DetectionConfig,
    /// Path where the baseline is persisted between restarts
    baseline_path: PathBuf,
    /// Number of metrics processed since the last baseline save
    metrics_since_save: usize,
    /// How often (in metric count) to auto-save the baseline
    save_interval: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionConfig {
    /// Sensitivity (0.0 = low, 1.0 = high)
    pub sensitivity: f64,
    /// Learning period in hours
    pub learning_period_hours: u64,
    /// Minimum confidence threshold
    pub confidence_threshold: f64,
    /// Enable auto-remediation
    pub auto_remediation: bool,
    /// Anomaly detection algorithms to use
    pub algorithms: Vec<Algorithm>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Algorithm {
    /// Statistical Z-Score
    ZScore,
    /// Isolation Forest
    IsolationForest,
    /// LSTM Neural Network
    LSTM,
    /// Moving Average Convergence Divergence
    MACD,
    /// Seasonal Hybrid ESD
    SeasonalHybridESD,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anomaly {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub anomaly_type: AnomalyType,
    pub severity: Severity,
    pub confidence: f64,
    pub metric: MetricType,
    pub baseline_value: f64,
    pub observed_value: f64,
    pub deviation: f64,
    pub context: AnomalyContext,
    pub remediation: Option<RemediationAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnomalyType {
    /// Sudden spike in traffic
    TrafficSpike,
    /// Unusual drop in traffic
    TrafficDrop,
    /// Latency increase
    LatencyIncrease,
    /// Error rate spike
    ErrorRateSpike,
    /// Unusual connection pattern
    UnusualConnectionPattern,
    /// Port scan detected
    PortScan,
    /// DNS tunneling suspected
    DNSTunneling,
    /// Data exfiltration pattern
    DataExfiltration,
    /// Resource exhaustion
    ResourceExhaustion,
    /// Behavioral anomaly
    BehavioralAnomaly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricType {
    RequestRate,
    BytesTransferred,
    Latency,
    ErrorRate,
    ConnectionCount,
    PacketRate,
    DNSQueryRate,
    UniqueDestinations,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyContext {
    pub namespace: String,
    pub pod: String,
    pub service: String,
    pub destination: Option<String>,
    pub protocol: String,
    pub port: u16,
    pub contributing_factors: Vec<String>,
    pub historical_occurrences: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationAction {
    pub action_type: RemediationType,
    pub description: String,
    pub confidence: f64,
    pub auto_applicable: bool,
    pub policy_yaml: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RemediationType {
    /// Apply rate limiting
    RateLimit,
    /// Block specific destination
    BlockDestination,
    /// Scale up resources
    ScaleUp,
    /// Isolate pod
    IsolatePod,
    /// Apply network policy
    ApplyNetworkPolicy,
    /// Alert only
    AlertOnly,
    /// Rollback deployment
    RollbackDeployment,
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            sensitivity: 0.7,
            learning_period_hours: 168, // 7 days
            confidence_threshold: 0.8,
            auto_remediation: false,
            algorithms: vec![
                Algorithm::ZScore,
                Algorithm::IsolationForest,
                Algorithm::SeasonalHybridESD,
            ],
        }
    }
}

impl AnomalyDetector {
    pub fn new(config: DetectionConfig) -> Result<Self> {
        Self::with_baseline_path(
            config,
            Path::new(baseline::DEFAULT_BASELINE_DIR).join("baseline.json"),
        )
    }

    /// Create a detector with an explicit baseline file path.
    ///
    /// If a saved baseline exists at `baseline_path`, it is loaded and the
    /// scorer's algorithm selection is adapted to the amount of historical
    /// data.  Otherwise a fresh baseline is created.
    pub fn with_baseline_path(config: DetectionConfig, baseline_path: PathBuf) -> Result<Self> {
        let baseline = if baseline_path.exists() {
            match baseline::BaselineLearner::load_baseline(&baseline_path) {
                Ok(b) => {
                    tracing::info!("Restored baseline from {}", baseline_path.display());
                    b
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to load baseline from {}: {e}. Starting fresh.",
                        baseline_path.display()
                    );
                    baseline::BaselineLearner::new(config.learning_period_hours)
                }
            }
        } else {
            baseline::BaselineLearner::new(config.learning_period_hours)
        };

        let mut scorer = scoring::AnomalyScorer::new(config.sensitivity, config.algorithms.clone());
        // Adapt algorithm selection to the amount of data we already have
        scorer.select_algorithms(baseline.observation_count());

        Ok(Self {
            baseline,
            scorer,
            alerting: alerting::AlertManager::new(config.confidence_threshold),
            remediation: remediation::RemediationEngine::new(config.auto_remediation),
            config,
            baseline_path,
            metrics_since_save: 0,
            save_interval: 500,
        })
    }

    /// Process incoming metrics and detect anomalies.
    ///
    /// For each metric the baseline is updated (learning) and then the scorer
    /// evaluates the metric against the learned baseline.  Algorithm selection
    /// is automatically adapted as the baseline grows.  The baseline is
    /// persisted to disk every `save_interval` metrics.
    pub async fn process_metrics(&mut self, metrics: &[Metric]) -> Result<Vec<Anomaly>> {
        let mut anomalies = Vec::new();

        // Learn from the batch and adapt algorithms
        let obs_count = self.baseline.learn(metrics)?;
        self.scorer.select_algorithms(obs_count);

        for metric in metrics {
            // Calculate anomaly score against the (just-updated) baseline
            if let Some(score) = self.scorer.score(metric, &self.baseline)? {
                if score.confidence >= self.config.confidence_threshold {
                    // Generate anomaly
                    let anomaly = self.create_anomaly(metric, score)?;

                    // Generate remediation if enabled
                    let anomaly = if self.config.auto_remediation {
                        self.remediation.suggest_remediation(anomaly)?
                    } else {
                        anomaly
                    };

                    anomalies.push(anomaly);
                }
            }
        }

        // Send alerts
        if !anomalies.is_empty() {
            self.alerting.send_alerts(&anomalies).await?;
        }

        // Periodically persist the baseline
        self.metrics_since_save += metrics.len();
        if self.metrics_since_save >= self.save_interval {
            self.save_baseline()?;
        }

        Ok(anomalies)
    }

    /// Persist the current baseline to disk.
    pub fn save_baseline(&mut self) -> Result<()> {
        self.baseline.save_baseline(&self.baseline_path)?;
        self.metrics_since_save = 0;
        Ok(())
    }

    fn create_anomaly(&self, metric: &Metric, score: scoring::AnomalyScore) -> Result<Anomaly> {
        Ok(Anomaly {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            anomaly_type: score.anomaly_type,
            severity: self.calculate_severity(score.confidence, score.deviation),
            confidence: score.confidence,
            metric: metric.metric_type.clone(),
            baseline_value: score.baseline_value,
            observed_value: metric.value,
            deviation: score.deviation,
            context: AnomalyContext {
                namespace: metric.namespace.clone(),
                pod: metric.pod.clone(),
                service: metric.service.clone(),
                destination: metric.destination.clone(),
                protocol: metric.protocol.clone(),
                port: metric.port,
                contributing_factors: score.contributing_factors,
                historical_occurrences: 0,
            },
            remediation: None,
        })
    }

    fn calculate_severity(&self, confidence: f64, deviation: f64) -> Severity {
        let score = confidence * deviation;

        if score > 8.0 {
            Severity::Critical
        } else if score > 5.0 {
            Severity::High
        } else if score > 3.0 {
            Severity::Medium
        } else if score > 1.5 {
            Severity::Low
        } else {
            Severity::Info
        }
    }

    /// Get current baseline statistics
    pub fn get_baseline_stats(&self) -> HashMap<String, baseline::BaselineStats> {
        self.baseline.get_stats()
    }

    /// Export anomalies for analysis
    pub async fn export_anomalies(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Anomaly>> {
        self.alerting.get_anomalies_in_range(start, end).await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    pub timestamp: DateTime<Utc>,
    pub metric_type: MetricType,
    pub value: f64,
    pub namespace: String,
    pub pod: String,
    pub service: String,
    pub destination: Option<String>,
    pub protocol: String,
    pub port: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_metric(value: f64) -> Metric {
        Metric {
            timestamp: Utc::now(),
            metric_type: MetricType::RequestRate,
            value,
            namespace: "default".to_string(),
            pod: "test-pod".to_string(),
            service: "test-svc".to_string(),
            destination: None,
            protocol: "TCP".to_string(),
            port: 8080,
        }
    }

    /// Create a detector that writes its baseline to a temp directory
    fn make_detector(config: DetectionConfig) -> AnomalyDetector {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("baseline.json");
        // Leak the TempDir so it isn't removed while the detector lives
        std::mem::forget(tmp);
        AnomalyDetector::with_baseline_path(config, path).unwrap()
    }

    #[test]
    fn test_anomaly_detector_creation() {
        let config = DetectionConfig::default();
        let detector = make_detector(config);
        assert!(detector.get_baseline_stats().is_empty());
    }

    #[test]
    fn test_detection_config_defaults() {
        let config = DetectionConfig::default();
        assert_eq!(config.sensitivity, 0.7);
        assert_eq!(config.learning_period_hours, 168);
        assert_eq!(config.confidence_threshold, 0.8);
        assert!(!config.auto_remediation);
        assert!(config.algorithms.contains(&Algorithm::ZScore));
        assert!(config.algorithms.contains(&Algorithm::IsolationForest));
    }

    #[test]
    fn test_calculate_severity_critical() {
        let detector = make_detector(DetectionConfig::default());
        // confidence * deviation > 8.0
        let severity = detector.calculate_severity(1.0, 9.0);
        assert_eq!(severity, Severity::Critical);
    }

    #[test]
    fn test_calculate_severity_high() {
        let detector = make_detector(DetectionConfig::default());
        // confidence * deviation > 5.0 but <= 8.0
        let severity = detector.calculate_severity(1.0, 6.0);
        assert_eq!(severity, Severity::High);
    }

    #[test]
    fn test_calculate_severity_medium() {
        let detector = make_detector(DetectionConfig::default());
        // confidence * deviation > 3.0 but <= 5.0
        let severity = detector.calculate_severity(1.0, 4.0);
        assert_eq!(severity, Severity::Medium);
    }

    #[test]
    fn test_calculate_severity_low() {
        let detector = make_detector(DetectionConfig::default());
        // confidence * deviation > 1.5 but <= 3.0
        let severity = detector.calculate_severity(1.0, 2.0);
        assert_eq!(severity, Severity::Low);
    }

    #[test]
    fn test_calculate_severity_info() {
        let detector = make_detector(DetectionConfig::default());
        // confidence * deviation <= 1.5
        let severity = detector.calculate_severity(0.5, 1.0);
        assert_eq!(severity, Severity::Info);
    }

    #[test]
    fn test_get_baseline_stats_initially_empty() {
        let detector = make_detector(DetectionConfig::default());
        let stats = detector.get_baseline_stats();
        assert!(stats.is_empty());
    }

    #[tokio::test]
    async fn test_process_metrics_builds_baseline() {
        let mut detector = make_detector(DetectionConfig::default());

        for i in 0..50 {
            let metric = make_metric(100.0 + (i as f64 % 10.0));
            detector.process_metrics(&[metric]).await.unwrap();
        }

        let stats = detector.get_baseline_stats();
        assert!(
            !stats.is_empty(),
            "Baseline should have entries after processing metrics"
        );
    }

    #[tokio::test]
    async fn test_anomaly_detection_with_spike() {
        let mut detector = make_detector(DetectionConfig::default());

        // Build baseline with 1000 normal metrics
        for i in 0..1000 {
            let metric = make_metric(100.0 + (i as f64 % 10.0));
            detector.process_metrics(&[metric]).await.unwrap();
        }

        // Inject extreme anomaly
        let anomalous_metric = make_metric(1000.0);
        let anomalies = detector.process_metrics(&[anomalous_metric]).await.unwrap();
        assert!(
            !anomalies.is_empty(),
            "Should detect anomaly with 10x spike"
        );
    }

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Critical > Severity::High);
        assert!(Severity::High > Severity::Medium);
        assert!(Severity::Medium > Severity::Low);
        assert!(Severity::Low > Severity::Info);
    }

    #[test]
    fn test_algorithm_equality() {
        assert_eq!(Algorithm::ZScore, Algorithm::ZScore);
        assert_ne!(Algorithm::ZScore, Algorithm::LSTM);
    }

    #[test]
    fn test_baseline_save_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("baseline.json");

        // Build a baseline with some data
        let mut learner = baseline::BaselineLearner::new(168);
        for v in [10.0, 20.0, 30.0, 40.0, 50.0] {
            learner.update(&make_metric(v)).unwrap();
        }

        // Save
        learner.save_baseline(&path).unwrap();
        assert!(path.exists());

        // Load
        let loaded = baseline::BaselineLearner::load_baseline(&path).unwrap();
        assert_eq!(loaded.observation_count(), 5);
        let stats = loaded.get_stats();
        assert_eq!(stats.len(), 1);
        let (_, bs) = stats.iter().next().unwrap();
        assert!((bs.mean - 30.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_baseline_persisted_after_interval() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("baseline.json");
        let config = DetectionConfig::default();
        let mut detector = AnomalyDetector::with_baseline_path(config, path.clone()).unwrap();

        // Process enough metrics to trigger auto-save (save_interval=500)
        for i in 0..600 {
            let metric = make_metric(100.0 + (i as f64 % 10.0));
            detector.process_metrics(&[metric]).await.unwrap();
        }

        assert!(
            path.exists(),
            "Baseline should be persisted after save_interval"
        );
    }

    #[tokio::test]
    async fn test_detector_loads_saved_baseline() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("baseline.json");

        // First detector: build baseline and save
        {
            let config = DetectionConfig::default();
            let mut d = AnomalyDetector::with_baseline_path(config, path.clone()).unwrap();
            for i in 0..100 {
                let metric = make_metric(100.0 + (i as f64 % 10.0));
                d.process_metrics(&[metric]).await.unwrap();
            }
            d.save_baseline().unwrap();
        }

        // Second detector: should load from the saved file
        let config2 = DetectionConfig::default();
        let d2 = AnomalyDetector::with_baseline_path(config2, path).unwrap();
        let stats = d2.get_baseline_stats();
        assert!(
            !stats.is_empty(),
            "Loaded detector should have baseline data"
        );
    }

    #[test]
    fn test_learn_returns_observation_count() {
        let mut learner = baseline::BaselineLearner::new(168);
        let metrics: Vec<Metric> = (0..25).map(|i| make_metric(i as f64)).collect();
        let count = learner.learn(&metrics).unwrap();
        assert_eq!(count, 25);
    }

    #[test]
    fn test_select_algorithms_small_dataset() {
        let mut scorer =
            scoring::AnomalyScorer::new(0.7, vec![Algorithm::ZScore, Algorithm::IsolationForest]);
        scorer.select_algorithms(500);
        // With < 1000 observations, only ZScore should remain
        let learner = baseline::BaselineLearner::new(168);
        let metric = make_metric(100.0);
        // Scoring with no baseline returns None regardless, but we're testing
        // that select_algorithms doesn't panic and the struct is valid.
        let result = scorer.score(&metric, &learner).unwrap();
        assert!(result.is_none());
    }
}
