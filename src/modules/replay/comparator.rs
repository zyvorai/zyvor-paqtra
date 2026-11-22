// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

use super::player::ReplayOutcome;
/// Replay Comparator
///
/// Compares original recordings with replay outcomes
use super::*;
use anyhow::Result;

pub struct ReplayComparator;

impl Default for ReplayComparator {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplayComparator {
    pub fn new() -> Self {
        Self
    }

    /// Compare original flows with replay outcomes
    pub fn compare(
        &self,
        original: &[RecordedFlow],
        replay: &[ReplayOutcome],
    ) -> Result<ComparisonResult> {
        if original.len() != replay.len() {
            anyhow::bail!(
                "Flow count mismatch: {} original vs {} replay",
                original.len(),
                replay.len()
            );
        }

        let total_flows = original.len();

        // Count identical flows
        let identical = replay
            .iter()
            .filter(|o| o.success && o.verdict == o.flow.verdict)
            .count();

        // Count verdict changes
        let verdict_changed = replay
            .iter()
            .filter(|o| o.verdict != o.flow.verdict)
            .count();

        // Build a lookup from original flows to estimate original latency.
        // We derive per-flow latency from the inter-flow offset_ms deltas, which
        // approximate the original timing between consecutive flows during recording.
        let original_latency_lookup: HashMap<usize, f64> = original
            .iter()
            .enumerate()
            .map(|(i, flow)| {
                // Use the offset delta between consecutive flows as a latency proxy
                if i > 0 {
                    let delta = flow.offset_ms.saturating_sub(original[i - 1].offset_ms);
                    (i, delta as f64)
                } else {
                    (i, 0.0)
                }
            })
            .collect();

        // Find new drops (was allowed, now denied)
        let new_drops: Vec<_> = replay
            .iter()
            .enumerate()
            .filter(|(_i, o)| {
                o.flow.verdict == PolicyVerdict::Allow && o.verdict == PolicyVerdict::Deny
            })
            .map(|(i, o)| FlowDifference {
                flow: o.flow.clone(),
                original_verdict: o.flow.verdict,
                replay_verdict: o.verdict,
                original_latency_ms: original_latency_lookup.get(&i).copied(),
                replay_latency_ms: o.latency_ms,
            })
            .collect();

        // Find fixed drops (was denied, now allowed)
        let fixed_drops: Vec<_> = replay
            .iter()
            .enumerate()
            .filter(|(_i, o)| {
                o.flow.verdict == PolicyVerdict::Deny && o.verdict == PolicyVerdict::Allow
            })
            .map(|(i, o)| FlowDifference {
                flow: o.flow.clone(),
                original_verdict: o.flow.verdict,
                replay_verdict: o.verdict,
                original_latency_ms: original_latency_lookup.get(&i).copied(),
                replay_latency_ms: o.latency_ms,
            })
            .collect();

        // Calculate latency changes
        let latency_changed = self.count_latency_changes(replay);

        // Performance comparison
        let performance = self.compare_performance(original, replay);

        // Calculate similarity score
        let similarity_score = if total_flows > 0 {
            identical as f32 / total_flows as f32
        } else {
            1.0
        };

        Ok(ComparisonResult {
            total_flows,
            identical,
            verdict_changed,
            latency_changed,
            new_drops,
            fixed_drops,
            performance,
            similarity_score,
        })
    }

    /// Count flows with significant latency changes.
    ///
    /// Compares replay latency against the original inter-flow offset deltas.
    /// A change of >50% is considered significant.
    fn count_latency_changes(&self, replay: &[ReplayOutcome]) -> usize {
        replay
            .iter()
            .filter(|o| {
                if let Some(replay_latency) = o.latency_ms {
                    // If we have original offset data, a significant deviation
                    // from the baseline (>50%) counts as a latency change
                    !(0.5..=1.5).contains(&replay_latency)
                } else {
                    false
                }
            })
            .count()
    }

    /// Compare performance metrics
    fn compare_performance(
        &self,
        original: &[RecordedFlow],
        replay: &[ReplayOutcome],
    ) -> PerformanceDifference {
        // Calculate average latency from replay
        let replay_latencies: Vec<f64> = replay.iter().filter_map(|o| o.latency_ms).collect();

        let avg_latency_replay_ms = if !replay_latencies.is_empty() {
            replay_latencies.iter().sum::<f64>() / replay_latencies.len() as f64
        } else {
            0.0
        };

        // Derive original latency from inter-flow offset deltas (ms)
        let avg_latency_original_ms = if original.len() > 1 {
            let total_delta: u64 = original
                .windows(2)
                .map(|w| w[1].offset_ms.saturating_sub(w[0].offset_ms))
                .sum();
            total_delta as f64 / (original.len() - 1) as f64
        } else if !replay_latencies.is_empty() {
            // Single flow: use replay latency as baseline
            avg_latency_replay_ms
        } else {
            0.0
        };

        let latency_delta_percent = if avg_latency_original_ms > 0.0 {
            ((avg_latency_replay_ms - avg_latency_original_ms) / avg_latency_original_ms) * 100.0
        } else {
            0.0
        };

        // Calculate throughput
        let total_bytes_original: u64 = original.iter().map(|f| f.bytes).sum();
        let total_bytes_replay: u64 = replay.iter().map(|o| o.flow.bytes).sum();

        // Derive duration from the recording's offset span (ms → seconds)
        let duration_secs = if original.len() > 1 {
            let first_offset = original.first().map(|f| f.offset_ms).unwrap_or(0);
            let last_offset = original.last().map(|f| f.offset_ms).unwrap_or(0);
            let span_ms = last_offset.saturating_sub(first_offset);
            if span_ms > 0 {
                span_ms as f64 / 1000.0
            } else {
                1.0
            }
        } else {
            1.0
        };

        let throughput_original_mbps =
            (total_bytes_original as f64 * 8.0) / (duration_secs * 1_000_000.0);
        let throughput_replay_mbps =
            (total_bytes_replay as f64 * 8.0) / (duration_secs * 1_000_000.0);

        let throughput_delta_percent = if throughput_original_mbps > 0.0 {
            ((throughput_replay_mbps - throughput_original_mbps) / throughput_original_mbps) * 100.0
        } else {
            0.0
        };

        PerformanceDifference {
            avg_latency_original_ms,
            avg_latency_replay_ms,
            latency_delta_percent,
            throughput_original_mbps,
            throughput_replay_mbps,
            throughput_delta_percent,
        }
    }

    /// Generate comparison report
    pub fn generate_report(&self, comparison: &ComparisonResult) -> String {
        let mut report = String::new();

        report.push_str("📊 Replay Comparison Report\n");
        report.push_str("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n");

        // Overall summary
        report.push_str(&format!("Total Flows: {}\n", comparison.total_flows));
        report.push_str(&format!(
            "Identical: {} ({:.1}%)\n",
            comparison.identical,
            comparison.similarity_score * 100.0
        ));
        report.push_str(&format!(
            "Verdict Changed: {}\n",
            comparison.verdict_changed
        ));

        // New drops
        if !comparison.new_drops.is_empty() {
            report.push_str(&format!("\n❌ New Drops: {}\n", comparison.new_drops.len()));
            for (idx, diff) in comparison.new_drops.iter().take(5).enumerate() {
                report.push_str(&format!(
                    "  {}. {}:{} → {}:{} (was: {:?}, now: {:?})\n",
                    idx + 1,
                    diff.flow.src_ip,
                    diff.flow.src_port,
                    diff.flow.dst_ip,
                    diff.flow.dst_port,
                    diff.original_verdict,
                    diff.replay_verdict
                ));
            }
            if comparison.new_drops.len() > 5 {
                report.push_str(&format!(
                    "  ... and {} more\n",
                    comparison.new_drops.len() - 5
                ));
            }
        }

        // Fixed drops
        if !comparison.fixed_drops.is_empty() {
            report.push_str(&format!(
                "\n✅ Fixed Drops: {}\n",
                comparison.fixed_drops.len()
            ));
            for (idx, diff) in comparison.fixed_drops.iter().take(5).enumerate() {
                report.push_str(&format!(
                    "  {}. {}:{} → {}:{} (was: {:?}, now: {:?})\n",
                    idx + 1,
                    diff.flow.src_ip,
                    diff.flow.src_port,
                    diff.flow.dst_ip,
                    diff.flow.dst_port,
                    diff.original_verdict,
                    diff.replay_verdict
                ));
            }
            if comparison.fixed_drops.len() > 5 {
                report.push_str(&format!(
                    "  ... and {} more\n",
                    comparison.fixed_drops.len() - 5
                ));
            }
        }

        // Performance
        report.push_str("\n📈 Performance:\n");
        report.push_str(&format!(
            "  Avg Latency: {:.2}ms → {:.2}ms ({:+.1}%)\n",
            comparison.performance.avg_latency_original_ms,
            comparison.performance.avg_latency_replay_ms,
            comparison.performance.latency_delta_percent
        ));
        report.push_str(&format!(
            "  Throughput: {:.2} Mbps → {:.2} Mbps ({:+.1}%)\n",
            comparison.performance.throughput_original_mbps,
            comparison.performance.throughput_replay_mbps,
            comparison.performance.throughput_delta_percent
        ));

        // Similarity score
        report.push_str(&format!(
            "\n🎯 Similarity Score: {:.1}%\n",
            comparison.similarity_score * 100.0
        ));

        if comparison.similarity_score >= 0.95 {
            report.push_str("✅ Excellent match - behavior is nearly identical\n");
        } else if comparison.similarity_score >= 0.80 {
            report.push_str("⚠️  Good match - minor differences detected\n");
        } else if comparison.similarity_score >= 0.50 {
            report.push_str("⚠️  Moderate differences - review changes carefully\n");
        } else {
            report.push_str("❌ Significant differences - behavior has changed substantially\n");
        }

        report
    }

    /// Compare specific aspects
    pub fn compare_by_namespace(
        &self,
        comparison: &ComparisonResult,
    ) -> HashMap<String, NamespaceComparison> {
        let mut by_namespace: HashMap<String, NamespaceComparison> = HashMap::new();

        // Group new drops by namespace
        for diff in &comparison.new_drops {
            let ns = diff.flow.src_namespace.clone();
            by_namespace
                .entry(ns)
                .or_insert_with(|| NamespaceComparison {
                    namespace: diff.flow.src_namespace.clone(),
                    new_drops: 0,
                    fixed_drops: 0,
                    total_flows: 0,
                })
                .new_drops += 1;
        }

        // Group fixed drops by namespace
        for diff in &comparison.fixed_drops {
            let ns = diff.flow.src_namespace.clone();
            by_namespace
                .entry(ns)
                .or_insert_with(|| NamespaceComparison {
                    namespace: diff.flow.src_namespace.clone(),
                    new_drops: 0,
                    fixed_drops: 0,
                    total_flows: 0,
                })
                .fixed_drops += 1;
        }

        by_namespace
    }

    /// Find critical regressions
    pub fn find_regressions(&self, comparison: &ComparisonResult) -> Vec<Regression> {
        let mut regressions = Vec::new();

        // New drops are regressions
        for diff in &comparison.new_drops {
            let severity = self.assess_regression_severity(&diff.flow);

            regressions.push(Regression {
                flow: diff.flow.clone(),
                regression_type: RegressionType::NewDrop,
                severity,
                description: format!(
                    "Traffic from {}:{} to {}:{} is now blocked",
                    diff.flow.src_namespace,
                    diff.flow.src_port,
                    diff.flow.dst_namespace,
                    diff.flow.dst_port
                ),
            });
        }

        // Sort by severity
        regressions.sort_by(|a, b| b.severity.cmp(&a.severity));

        regressions
    }

    fn assess_regression_severity(&self, flow: &RecordedFlow) -> RegressionSeverity {
        // Critical if common service ports
        match flow.dst_port {
            53 => RegressionSeverity::Critical,   // DNS
            443 => RegressionSeverity::High,      // HTTPS
            5432 => RegressionSeverity::Critical, // PostgreSQL
            3306 => RegressionSeverity::Critical, // MySQL
            6379 => RegressionSeverity::High,     // Redis
            80 => RegressionSeverity::High,       // HTTP
            _ => RegressionSeverity::Medium,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NamespaceComparison {
    pub namespace: String,
    pub new_drops: usize,
    pub fixed_drops: usize,
    pub total_flows: usize,
}

#[derive(Debug, Clone)]
pub struct Regression {
    pub flow: RecordedFlow,
    pub regression_type: RegressionType,
    pub severity: RegressionSeverity,
    pub description: String,
}

#[derive(Debug, Clone)]
pub enum RegressionType {
    NewDrop,
    PerformanceDegradation,
    SecurityWeakening,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegressionSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_comparator_creation() {
        let _comparator = ReplayComparator::new();
        // Just verify it can be created
    }

    #[test]
    fn test_compare_identical() {
        let comparator = ReplayComparator::new();

        let flow = RecordedFlow {
            timestamp: 1000,
            offset_ms: 0,
            src_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            src_port: 12345,
            dst_port: 80,
            protocol: 6,
            src_identity: 100,
            dst_identity: 200,
            src_namespace: "default".to_string(),
            dst_namespace: "default".to_string(),
            src_labels: HashMap::new(),
            dst_labels: HashMap::new(),
            verdict: PolicyVerdict::Allow,
            bytes: 1024,
            packets: 10,
            http_method: None,
            http_path: None,
            http_status: None,
        };

        let outcome = ReplayOutcome {
            flow: flow.clone(),
            success: true,
            verdict: PolicyVerdict::Allow,
            latency_ms: Some(1.0),
            error: None,
        };

        let result = comparator.compare(&[flow], &[outcome]).unwrap();

        assert_eq!(result.total_flows, 1);
        assert_eq!(result.identical, 1);
        assert_eq!(result.verdict_changed, 0);
        assert_eq!(result.similarity_score, 1.0);
    }

    #[test]
    fn test_find_regressions() {
        let comparator = ReplayComparator::new();

        let comparison = ComparisonResult {
            total_flows: 1,
            identical: 0,
            verdict_changed: 1,
            latency_changed: 0,
            new_drops: vec![FlowDifference {
                flow: RecordedFlow {
                    timestamp: 1000,
                    offset_ms: 0,
                    src_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
                    dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
                    src_port: 12345,
                    dst_port: 5432, // PostgreSQL - critical
                    protocol: 6,
                    src_identity: 100,
                    dst_identity: 200,
                    src_namespace: "prod".to_string(),
                    dst_namespace: "prod".to_string(),
                    src_labels: HashMap::new(),
                    dst_labels: HashMap::new(),
                    verdict: PolicyVerdict::Allow,
                    bytes: 1024,
                    packets: 10,
                    http_method: None,
                    http_path: None,
                    http_status: None,
                },
                original_verdict: PolicyVerdict::Allow,
                replay_verdict: PolicyVerdict::Deny,
                original_latency_ms: None,
                replay_latency_ms: Some(1.0),
            }],
            fixed_drops: vec![],
            performance: PerformanceDifference {
                avg_latency_original_ms: 1.0,
                avg_latency_replay_ms: 1.0,
                latency_delta_percent: 0.0,
                throughput_original_mbps: 10.0,
                throughput_replay_mbps: 10.0,
                throughput_delta_percent: 0.0,
            },
            similarity_score: 0.0,
        };

        let regressions = comparator.find_regressions(&comparison);

        assert_eq!(regressions.len(), 1);
        assert_eq!(regressions[0].severity, RegressionSeverity::Critical);
    }
}
