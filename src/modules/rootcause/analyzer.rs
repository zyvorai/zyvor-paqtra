// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Drop Pattern Analyzer
///
/// Analyzes drop patterns and trends over time
use super::*;
use std::collections::HashMap;

pub struct DropAnalyzer;

impl DropAnalyzer {
    /// Analyze drop trends over time
    pub fn analyze_trends(history: &[DropEvent], window_secs: u64) -> TrendAnalysis {
        if history.is_empty() {
            return TrendAnalysis::default();
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let cutoff = now.saturating_sub(window_secs);

        // Filter to time window
        let recent: Vec<_> = history.iter().filter(|e| e.timestamp >= cutoff).collect();

        // Calculate rate
        let drop_rate = if window_secs > 0 {
            recent.len() as f64 / window_secs as f64
        } else {
            0.0
        };

        // Detect spikes
        let is_spike = Self::detect_spike(&recent, window_secs);

        // Most common reason
        let mut reason_counts: HashMap<DropReason, usize> = HashMap::new();
        for event in &recent {
            *reason_counts.entry(event.reason.clone()).or_insert(0) += 1;
        }

        let most_common_reason = reason_counts
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(reason, _)| reason.clone());

        TrendAnalysis {
            total_drops: recent.len(),
            drop_rate_per_sec: drop_rate,
            is_spike,
            most_common_reason,
            unique_reasons: reason_counts.len(),
            window_secs,
        }
    }

    /// Detect if there's a spike in drops
    fn detect_spike(events: &[&DropEvent], window_secs: u64) -> bool {
        if events.len() < 10 {
            return false;
        }

        // Simple spike detection: more than 10 drops per second on average
        let rate = events.len() as f64 / window_secs as f64;
        rate > 10.0
    }

    /// Find repeating patterns
    pub fn find_repeating_patterns(
        pattern_counts: &HashMap<DropPattern, u64>,
        min_count: u64,
    ) -> Vec<RepeatingPattern> {
        let mut patterns = Vec::new();

        for (pattern, count) in pattern_counts {
            if *count >= min_count {
                let severity = Self::calculate_severity(pattern, *count);

                patterns.push(RepeatingPattern {
                    pattern: pattern.clone(),
                    occurrences: *count,
                    severity,
                    description: Self::describe_pattern(pattern),
                });
            }
        }

        // Sort by severity
        patterns.sort_by(|a, b| b.severity.cmp(&a.severity));

        patterns
    }

    /// Calculate severity of a repeating pattern
    fn calculate_severity(pattern: &DropPattern, count: u64) -> Severity {
        // High severity for policy denies and missing services
        let reason_weight = match pattern.reason {
            DropReason::PolicyDenied | DropReason::PortNotAllowed => 3,
            DropReason::ServiceNotFound | DropReason::NoBackend => 2,
            _ => 1,
        };

        let score = count * reason_weight;

        if score > 1000 {
            Severity::Critical
        } else if score > 100 {
            Severity::High
        } else if score > 10 {
            Severity::Medium
        } else {
            Severity::Low
        }
    }

    /// Describe a drop pattern in human-readable form
    fn describe_pattern(pattern: &DropPattern) -> String {
        let protocol = match pattern.protocol {
            6 => "TCP",
            17 => "UDP",
            1 => "ICMP",
            _ => "unknown",
        };

        format!(
            "{} - identity {} → {} on port {} ({})",
            pattern.reason, pattern.src_identity, pattern.dst_identity, pattern.dst_port, protocol
        )
    }

    /// Group drops by namespace
    pub fn group_by_namespace(events: &[DropEvent]) -> HashMap<String, Vec<DropEvent>> {
        let mut grouped: HashMap<String, Vec<DropEvent>> = HashMap::new();

        for event in events {
            let namespace = event
                .namespace
                .clone()
                .unwrap_or_else(|| "unknown".to_string());

            grouped.entry(namespace).or_default().push(event.clone());
        }

        grouped
    }

    /// Find namespaces with most drops
    pub fn top_namespaces(events: &[DropEvent], limit: usize) -> Vec<(String, usize)> {
        let grouped = Self::group_by_namespace(events);

        let mut counts: Vec<_> = grouped
            .iter()
            .map(|(ns, events)| (ns.clone(), events.len()))
            .collect();

        counts.sort_by_key(|a| std::cmp::Reverse(a.1));
        counts.into_iter().take(limit).collect()
    }

    /// Analyze drop reasons distribution
    pub fn reason_distribution(events: &[DropEvent]) -> Vec<(DropReason, usize, f32)> {
        if events.is_empty() {
            return Vec::new();
        }

        let mut counts: HashMap<DropReason, usize> = HashMap::new();
        for event in events {
            *counts.entry(event.reason.clone()).or_insert(0) += 1;
        }

        let total = events.len() as f32;
        let mut distribution: Vec<_> = counts
            .into_iter()
            .map(|(reason, count)| {
                let percentage = (count as f32 / total) * 100.0;
                (reason, count, percentage)
            })
            .collect();

        distribution.sort_by_key(|a| std::cmp::Reverse(a.1));
        distribution
    }

    /// Detect anomalies in drop patterns
    pub fn detect_anomalies(
        pattern_counts: &HashMap<DropPattern, u64>,
        baseline_counts: &HashMap<DropPattern, u64>,
    ) -> Vec<Anomaly> {
        let mut anomalies = Vec::new();

        // Find new patterns
        for (pattern, count) in pattern_counts {
            if !baseline_counts.contains_key(pattern) && *count > 5 {
                anomalies.push(Anomaly {
                    anomaly_type: AnomalyType::NewPattern,
                    pattern: pattern.clone(),
                    current_count: *count,
                    baseline_count: 0,
                    severity: Self::calculate_severity(pattern, *count),
                });
            }
        }

        // Find increased frequency
        for (pattern, current_count) in pattern_counts {
            if let Some(baseline_count) = baseline_counts.get(pattern) {
                let increase_ratio = *current_count as f64 / *baseline_count as f64;

                if increase_ratio > 3.0 {
                    // 3x increase
                    anomalies.push(Anomaly {
                        anomaly_type: AnomalyType::IncreasedFrequency,
                        pattern: pattern.clone(),
                        current_count: *current_count,
                        baseline_count: *baseline_count,
                        severity: Self::calculate_severity(pattern, *current_count),
                    });
                }
            }
        }

        // Sort by severity
        anomalies.sort_by(|a, b| b.severity.cmp(&a.severity));

        anomalies
    }

    /// Suggest investigations based on drop patterns
    pub fn suggest_investigations(patterns: &[RepeatingPattern]) -> Vec<Investigation> {
        let mut investigations = Vec::new();

        for pattern in patterns {
            let investigation = match &pattern.pattern.reason {
                DropReason::PolicyDenied | DropReason::PortNotAllowed => Investigation {
                    title: format!(
                        "Investigate policy gaps for port {}",
                        pattern.pattern.dst_port
                    ),
                    steps: vec![
                        format!(
                            "Check if traffic from identity {} to {} should be allowed",
                            pattern.pattern.src_identity, pattern.pattern.dst_identity
                        ),
                        "Review CiliumNetworkPolicy rules".to_string(),
                        "Consider adding explicit allow rule or verify deny is intentional"
                            .to_string(),
                    ],
                    priority: pattern.severity.clone(),
                },
                DropReason::NoBackend | DropReason::ServiceNotFound => Investigation {
                    title: format!(
                        "Investigate service availability on port {}",
                        pattern.pattern.dst_port
                    ),
                    steps: vec![
                        "Check if service exists: kubectl get svc --all-namespaces".to_string(),
                        "Verify pod readiness: kubectl get pods -l <selector>".to_string(),
                        "Check service endpoints: kubectl get endpoints".to_string(),
                    ],
                    priority: pattern.severity.clone(),
                },
                DropReason::FragNeeded => Investigation {
                    title: "Investigate MTU configuration".to_string(),
                    steps: vec![
                        "Check current MTU: ip link show".to_string(),
                        "Review overlay network MTU settings".to_string(),
                        "Consider reducing pod interface MTU to 1450".to_string(),
                    ],
                    priority: pattern.severity.clone(),
                },
                DropReason::CTStateMismatch => Investigation {
                    title: "Investigate connection tracking issues".to_string(),
                    steps: vec![
                        "Check conntrack table: cilium bpf ct list global".to_string(),
                        "Review conntrack max entries".to_string(),
                        "Look for asymmetric routing".to_string(),
                    ],
                    priority: pattern.severity.clone(),
                },
                _ => continue,
            };

            investigations.push(investigation);
        }

        investigations
    }
}

#[derive(Debug, Clone, Default)]
pub struct TrendAnalysis {
    pub total_drops: usize,
    pub drop_rate_per_sec: f64,
    pub is_spike: bool,
    pub most_common_reason: Option<DropReason>,
    pub unique_reasons: usize,
    pub window_secs: u64,
}

#[derive(Debug, Clone)]
pub struct RepeatingPattern {
    pub pattern: DropPattern,
    pub occurrences: u64,
    pub severity: Severity,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone)]
pub struct Anomaly {
    pub anomaly_type: AnomalyType,
    pub pattern: DropPattern,
    pub current_count: u64,
    pub baseline_count: u64,
    pub severity: Severity,
}

#[derive(Debug, Clone)]
pub enum AnomalyType {
    NewPattern,
    IncreasedFrequency,
}

#[derive(Debug, Clone)]
pub struct Investigation {
    pub title: String,
    pub steps: Vec<String>,
    pub priority: Severity,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_analyze_trends_empty() {
        let history = Vec::new();
        let trends = DropAnalyzer::analyze_trends(&history, 300);
        assert_eq!(trends.total_drops, 0);
    }

    #[test]
    fn test_calculate_severity() {
        let pattern = DropPattern {
            reason: DropReason::PolicyDenied,
            src_identity: 100,
            dst_identity: 200,
            dst_port: 80,
            protocol: 6,
        };

        let severity = DropAnalyzer::calculate_severity(&pattern, 1000);
        assert_eq!(severity, Severity::Critical);

        let severity = DropAnalyzer::calculate_severity(&pattern, 50);
        assert_eq!(severity, Severity::High);
    }

    #[test]
    fn test_describe_pattern() {
        let pattern = DropPattern {
            reason: DropReason::PolicyDenied,
            src_identity: 100,
            dst_identity: 200,
            dst_port: 80,
            protocol: 6,
        };

        let description = DropAnalyzer::describe_pattern(&pattern);
        assert!(description.contains("Policy Denied"));
        assert!(description.contains("80"));
        assert!(description.contains("TCP"));
    }

    #[test]
    fn test_reason_distribution() {
        let events = vec![
            DropEvent {
                timestamp: 1234567890,
                src_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
                dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
                src_port: 12345,
                dst_port: 80,
                protocol: 6,
                reason: DropReason::PolicyDenied,
                identity_src: 100,
                identity_dst: 200,
                namespace: None,
                pod_name: None,
            },
            DropEvent {
                timestamp: 1234567891,
                src_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
                dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
                src_port: 12345,
                dst_port: 80,
                protocol: 6,
                reason: DropReason::PolicyDenied,
                identity_src: 100,
                identity_dst: 200,
                namespace: None,
                pod_name: None,
            },
        ];

        let distribution = DropAnalyzer::reason_distribution(&events);
        assert_eq!(distribution.len(), 1);
        assert_eq!(distribution[0].1, 2);
        assert_eq!(distribution[0].2, 100.0);
    }
}
