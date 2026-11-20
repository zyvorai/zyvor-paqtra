// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Baseline Learning - Establishes normal behavior patterns
use anyhow::Result;
use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::Path;

use super::Metric;

/// Default directory for persisted baselines
pub const DEFAULT_BASELINE_DIR: &str = "/var/lib/paqtra/baselines/";

/// Learns and maintains baseline behavior for metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineLearner {
    learning_period_hours: u64,
    baselines: HashMap<String, MetricBaseline>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricBaseline {
    pub metric_key: String,
    pub data_points: VecDeque<DataPoint>,
    pub stats: BaselineStats,
    pub seasonal_patterns: SeasonalPatterns,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    pub timestamp: DateTime<Utc>,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineStats {
    pub mean: f64,
    pub median: f64,
    pub std_dev: f64,
    pub min: f64,
    pub max: f64,
    pub percentile_95: f64,
    pub percentile_99: f64,
    pub sample_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonalPatterns {
    pub hourly_patterns: HashMap<u32, f64>, // Hour of day -> avg value
    pub daily_patterns: HashMap<u32, f64>,  // Day of week -> avg value
    pub weekly_trend: f64,
}

impl BaselineLearner {
    pub fn new(learning_period_hours: u64) -> Self {
        Self {
            learning_period_hours,
            baselines: HashMap::new(),
        }
    }

    /// Update baseline with new metric
    pub fn update(&mut self, metric: &Metric) -> Result<()> {
        let key = self.get_metric_key(metric);
        let cutoff = Utc::now() - Duration::hours(self.learning_period_hours as i64);

        // Get or create baseline
        if !self.baselines.contains_key(&key) {
            self.baselines.insert(
                key.clone(),
                MetricBaseline {
                    metric_key: key.clone(),
                    data_points: VecDeque::new(),
                    stats: BaselineStats::default(),
                    seasonal_patterns: SeasonalPatterns::default(),
                    last_updated: Utc::now(),
                },
            );
        }

        // Now we can safely get mutable reference
        let baseline = self.baselines.get_mut(&key).unwrap();

        // Add data point
        baseline.data_points.push_back(DataPoint {
            timestamp: metric.timestamp,
            value: metric.value,
        });

        // Remove old data points
        while let Some(front) = baseline.data_points.front() {
            if front.timestamp < cutoff {
                baseline.data_points.pop_front();
            } else {
                break;
            }
        }

        // Clone data points for calculations
        let data_points = baseline.data_points.clone();

        // Recalculate statistics
        baseline.stats = Self::calculate_stats_static(&data_points);
        baseline.seasonal_patterns = Self::calculate_seasonal_patterns_static(&data_points);
        baseline.last_updated = Utc::now();

        Ok(())
    }

    /// Get baseline for a metric
    pub fn get_baseline(&self, metric: &Metric) -> Option<&MetricBaseline> {
        let key = self.get_metric_key(metric);
        self.baselines.get(&key)
    }

    /// Get all baseline statistics
    pub fn get_stats(&self) -> HashMap<String, BaselineStats> {
        self.baselines
            .iter()
            .map(|(k, v)| (k.clone(), v.stats.clone()))
            .collect()
    }

    /// Process a batch of flow metrics and build/update the baseline.
    ///
    /// This is the high-level entry point for baseline learning: it iterates
    /// over a slice of metrics, updates the underlying statistical model for
    /// each one, and returns the total number of observations across all
    /// metric keys.
    pub fn learn(&mut self, metrics: &[Metric]) -> Result<usize> {
        for metric in metrics {
            self.update(metric)?;
        }
        Ok(self.observation_count())
    }

    /// Total number of data points across all baselines
    pub fn observation_count(&self) -> usize {
        self.baselines.values().map(|b| b.data_points.len()).sum()
    }

    /// Serialize the learned baselines to a JSON file at `path`.
    pub fn save_baseline(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        tracing::info!(
            "Saved baseline ({} keys) to {}",
            self.baselines.len(),
            path.display()
        );
        Ok(())
    }

    /// Deserialize a `BaselineLearner` from a JSON file at `path`.
    pub fn load_baseline(path: &Path) -> Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let learner: Self = serde_json::from_str(&json)?;
        tracing::info!(
            "Loaded baseline ({} keys, {} observations) from {}",
            learner.baselines.len(),
            learner.observation_count(),
            path.display()
        );
        Ok(learner)
    }

    fn get_metric_key(&self, metric: &Metric) -> String {
        format!(
            "{}:{}:{}:{:?}",
            metric.namespace, metric.service, metric.port, metric.metric_type
        )
    }

    fn calculate_stats_static(data_points: &VecDeque<DataPoint>) -> BaselineStats {
        if data_points.is_empty() {
            return BaselineStats::default();
        }

        let values: Vec<f64> = data_points.iter().map(|dp| dp.value).collect();
        let mut sorted_values = values.clone();
        sorted_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let n = values.len();
        let sum: f64 = values.iter().sum();
        let mean = sum / n as f64;

        let variance: f64 = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
        let std_dev = variance.sqrt();

        let median = if n.is_multiple_of(2) {
            (sorted_values[n / 2 - 1] + sorted_values[n / 2]) / 2.0
        } else {
            sorted_values[n / 2]
        };

        let percentile_95 = sorted_values[((n as f64 * 0.95) as usize).min(n - 1)];
        let percentile_99 = sorted_values[((n as f64 * 0.99) as usize).min(n - 1)];

        BaselineStats {
            mean,
            median,
            std_dev,
            min: *sorted_values.first().unwrap(),
            max: *sorted_values.last().unwrap(),
            percentile_95,
            percentile_99,
            sample_count: n,
        }
    }

    fn calculate_seasonal_patterns_static(data_points: &VecDeque<DataPoint>) -> SeasonalPatterns {
        let mut hourly_patterns: HashMap<u32, Vec<f64>> = HashMap::new();
        let mut daily_patterns: HashMap<u32, Vec<f64>> = HashMap::new();

        for dp in data_points {
            let hour = dp.timestamp.hour();
            let day = dp.timestamp.weekday().num_days_from_monday();

            hourly_patterns.entry(hour).or_default().push(dp.value);

            daily_patterns.entry(day).or_default().push(dp.value);
        }

        let hourly_avgs = hourly_patterns
            .into_iter()
            .map(|(h, values)| (h, values.iter().sum::<f64>() / values.len() as f64))
            .collect();

        let daily_avgs = daily_patterns
            .into_iter()
            .map(|(d, values)| (d, values.iter().sum::<f64>() / values.len() as f64))
            .collect();

        // Calculate weekly trend (simple linear regression)
        let weekly_trend = if data_points.len() > 2 {
            let first_half: f64 = data_points
                .iter()
                .take(data_points.len() / 2)
                .map(|dp| dp.value)
                .sum::<f64>()
                / (data_points.len() / 2) as f64;

            let second_half: f64 = data_points
                .iter()
                .skip(data_points.len() / 2)
                .map(|dp| dp.value)
                .sum::<f64>()
                / (data_points.len() - data_points.len() / 2) as f64;

            if first_half.abs() < f64::EPSILON {
                0.0
            } else {
                (second_half - first_half) / first_half
            }
        } else {
            0.0
        };

        SeasonalPatterns {
            hourly_patterns: hourly_avgs,
            daily_patterns: daily_avgs,
            weekly_trend,
        }
    }
}

impl Default for BaselineStats {
    fn default() -> Self {
        Self {
            mean: 0.0,
            median: 0.0,
            std_dev: 0.0,
            min: 0.0,
            max: 0.0,
            percentile_95: 0.0,
            percentile_99: 0.0,
            sample_count: 0,
        }
    }
}

impl Default for SeasonalPatterns {
    fn default() -> Self {
        Self {
            hourly_patterns: HashMap::new(),
            daily_patterns: HashMap::new(),
            weekly_trend: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::anomaly_detection::MetricType;

    fn make_metric(value: f64) -> Metric {
        Metric {
            timestamp: chrono::Utc::now(),
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

    #[test]
    fn test_baseline_learner_creation() {
        let learner = BaselineLearner::new(168);
        assert!(learner.get_stats().is_empty());
    }

    #[test]
    fn test_update_creates_baseline() {
        let mut learner = BaselineLearner::new(168);
        let metric = make_metric(100.0);
        learner.update(&metric).unwrap();

        let stats = learner.get_stats();
        assert_eq!(stats.len(), 1);
    }

    #[test]
    fn test_get_baseline_after_update() {
        let mut learner = BaselineLearner::new(168);
        let metric = make_metric(50.0);
        learner.update(&metric).unwrap();

        let baseline = learner.get_baseline(&metric);
        assert!(baseline.is_some());
        assert_eq!(baseline.unwrap().stats.sample_count, 1);
    }

    #[test]
    fn test_stats_calculation_mean() {
        let mut learner = BaselineLearner::new(168);
        for v in [10.0, 20.0, 30.0] {
            let metric = make_metric(v);
            learner.update(&metric).unwrap();
        }

        let metric = make_metric(0.0); // just for key lookup
        let baseline = learner.get_baseline(&metric).unwrap();
        assert!((baseline.stats.mean - 20.0).abs() < 0.001);
        assert_eq!(baseline.stats.sample_count, 3);
    }

    #[test]
    fn test_stats_calculation_std_dev() {
        let mut learner = BaselineLearner::new(168);
        // All same values -> std_dev should be 0
        for _ in 0..10 {
            let metric = make_metric(100.0);
            learner.update(&metric).unwrap();
        }

        let metric = make_metric(0.0);
        let baseline = learner.get_baseline(&metric).unwrap();
        assert_eq!(baseline.stats.std_dev, 0.0);
        assert_eq!(baseline.stats.mean, 100.0);
    }

    #[test]
    fn test_stats_min_max() {
        let mut learner = BaselineLearner::new(168);
        for v in [5.0, 10.0, 15.0, 20.0, 25.0] {
            let metric = make_metric(v);
            learner.update(&metric).unwrap();
        }

        let metric = make_metric(0.0);
        let baseline = learner.get_baseline(&metric).unwrap();
        assert_eq!(baseline.stats.min, 5.0);
        assert_eq!(baseline.stats.max, 25.0);
    }

    #[test]
    fn test_stats_median_odd() {
        let mut learner = BaselineLearner::new(168);
        for v in [1.0, 3.0, 5.0, 7.0, 9.0] {
            let metric = make_metric(v);
            learner.update(&metric).unwrap();
        }

        let metric = make_metric(0.0);
        let baseline = learner.get_baseline(&metric).unwrap();
        assert_eq!(baseline.stats.median, 5.0);
    }

    #[test]
    fn test_stats_median_even() {
        let mut learner = BaselineLearner::new(168);
        for v in [1.0, 3.0, 5.0, 7.0] {
            let metric = make_metric(v);
            learner.update(&metric).unwrap();
        }

        let metric = make_metric(0.0);
        let baseline = learner.get_baseline(&metric).unwrap();
        // Median of [1, 3, 5, 7] = (3 + 5) / 2 = 4.0
        assert_eq!(baseline.stats.median, 4.0);
    }

    #[test]
    fn test_empty_data_points_default_stats() {
        let data_points = std::collections::VecDeque::new();
        let stats = BaselineLearner::calculate_stats_static(&data_points);
        assert_eq!(stats.sample_count, 0);
        assert_eq!(stats.mean, 0.0);
    }

    #[test]
    fn test_seasonal_patterns_populated() {
        let mut learner = BaselineLearner::new(168);
        for _ in 0..5 {
            let metric = make_metric(100.0);
            learner.update(&metric).unwrap();
        }

        let metric = make_metric(0.0);
        let baseline = learner.get_baseline(&metric).unwrap();
        // Should have at least one hourly pattern entry for the current hour
        assert!(!baseline.seasonal_patterns.hourly_patterns.is_empty());
    }

    #[test]
    fn test_baseline_stats_default() {
        let stats = BaselineStats::default();
        assert_eq!(stats.mean, 0.0);
        assert_eq!(stats.std_dev, 0.0);
        assert_eq!(stats.sample_count, 0);
    }
}
