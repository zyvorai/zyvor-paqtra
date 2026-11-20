// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// DNS-specific healing logic
use super::*;

pub struct DNSHealer;

impl DNSHealer {
    pub fn detect_dns_issues(drops: &[DropReason]) -> Vec<Problem> {
        let mut problems = Vec::new();

        // Group DNS drops by source
        let mut dns_drops_by_source: HashMap<String, u64> = HashMap::new();

        for drop in drops {
            if drop.port == 53 {
                *dns_drops_by_source.entry(drop.src_ip.clone()).or_insert(0) += 1;
            }
        }

        // Create problems for sources with many DNS drops
        for (src_ip, count) in dns_drops_by_source {
            if count > 5 {
                // Threshold: more than 5 DNS drops (consistent with SelfHealer)
                // Namespace cannot be resolved here because DNSHealer operates on raw
                // DropReason data without access to IPCache. Use the enriched variant
                // (SelfHealer::detect_problems_enriched) when pod context is needed.
                problems.push(Problem::DNSDrops {
                    namespace: "unknown".to_string(),
                    pod: src_ip,
                    count,
                });
            }
        }

        problems
    }

    pub fn generate_dns_fix(namespace: &str) -> Fix {
        Fix {
            problem: Problem::DNSDrops {
                namespace: namespace.to_string(),
                pod: "unknown".to_string(),
                count: 0,
            },
            action: FixAction::CreateDNSPolicy {
                namespace: namespace.to_string(),
            },
            applied: false,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ebpf::{DropReason, DropReasonType};

    fn make_dns_drop(src_ip: &str) -> DropReason {
        DropReason {
            src_ip: src_ip.to_string(),
            dst_ip: "10.96.0.10".to_string(),
            port: 53,
            protocol: 17,
            reason: DropReasonType::PolicyDenied,
            timestamp: 0,
        }
    }

    #[test]
    fn test_no_dns_drops() {
        let drops: Vec<DropReason> = vec![];
        let problems = DNSHealer::detect_dns_issues(&drops);
        assert!(problems.is_empty());
    }

    #[test]
    fn test_dns_drops_below_threshold() {
        // Only 5 drops from same source - at threshold (> 5 needed)
        let drops: Vec<DropReason> = (0..5).map(|_| make_dns_drop("10.0.0.1")).collect();
        let problems = DNSHealer::detect_dns_issues(&drops);
        assert!(problems.is_empty());
    }

    #[test]
    fn test_dns_drops_above_threshold() {
        // 6 drops from same source - above threshold
        let drops: Vec<DropReason> = (0..6).map(|_| make_dns_drop("10.0.0.1")).collect();
        let problems = DNSHealer::detect_dns_issues(&drops);
        assert_eq!(problems.len(), 1);
        match &problems[0] {
            Problem::DNSDrops { pod, count, .. } => {
                assert_eq!(pod, "10.0.0.1");
                assert_eq!(*count, 6);
            }
            _ => panic!("Expected DNSDrops problem"),
        }
    }

    #[test]
    fn test_dns_drops_multiple_sources() {
        let mut drops = Vec::new();
        // 7 drops from source A (above threshold)
        for _ in 0..7 {
            drops.push(make_dns_drop("10.0.0.1"));
        }
        // 8 drops from source B (above threshold)
        for _ in 0..8 {
            drops.push(make_dns_drop("10.0.0.2"));
        }
        // 3 drops from source C (below threshold)
        for _ in 0..3 {
            drops.push(make_dns_drop("10.0.0.3"));
        }

        let problems = DNSHealer::detect_dns_issues(&drops);
        assert_eq!(problems.len(), 2); // Only A and B
    }

    #[test]
    fn test_non_dns_drops_ignored() {
        // Drops on non-DNS port should be ignored
        let drops = vec![
            DropReason {
                src_ip: "10.0.0.1".to_string(),
                dst_ip: "10.0.0.2".to_string(),
                port: 80,
                protocol: 6,
                reason: DropReasonType::PolicyDenied,
                timestamp: 0,
            },
            DropReason {
                src_ip: "10.0.0.1".to_string(),
                dst_ip: "10.0.0.2".to_string(),
                port: 80,
                protocol: 6,
                reason: DropReasonType::PolicyDenied,
                timestamp: 0,
            },
            DropReason {
                src_ip: "10.0.0.1".to_string(),
                dst_ip: "10.0.0.2".to_string(),
                port: 80,
                protocol: 6,
                reason: DropReasonType::PolicyDenied,
                timestamp: 0,
            },
        ];
        let problems = DNSHealer::detect_dns_issues(&drops);
        assert!(problems.is_empty());
    }

    #[test]
    fn test_generate_dns_fix() {
        let fix = DNSHealer::generate_dns_fix("production");
        assert_eq!(
            fix.action,
            FixAction::CreateDNSPolicy {
                namespace: "production".to_string(),
            }
        );
        assert!(!fix.applied);
        match &fix.problem {
            Problem::DNSDrops { namespace, .. } => {
                assert_eq!(namespace, "production");
            }
            _ => panic!("Expected DNSDrops problem"),
        }
    }
}
