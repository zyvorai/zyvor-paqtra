// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// MTU-specific healing logic
use super::*;

pub struct MTUHealer;

impl MTUHealer {
    /// Detect MTU mismatch issues by checking for FragmentationNeeded drops
    pub fn detect_mtu_issues<M: MapReader>(reader: &M) -> Vec<Problem> {
        let mut problems = Vec::new();

        // Check for FragmentationNeeded drop reasons in eBPF metrics
        if let Ok(drops) = reader.read_drop_map() {
            for drop in &drops {
                if drop.reason == DropReasonType::FragmentationNeeded {
                    // VXLAN/Geneve overhead is typically 50 bytes
                    // Default MTU is 1500, so effective MTU is ~1450
                    problems.push(Problem::MTUMismatch {
                        namespace: "unknown".to_string(),
                        pod: drop.src_ip.clone(),
                        expected: 1500,
                        actual: 1450, // Likely needs VXLAN overhead adjustment
                    });
                }
            }
        }

        problems
    }

    pub fn generate_mtu_fix(namespace: &str, pod: &str, new_mtu: u16) -> Fix {
        Fix {
            problem: Problem::MTUMismatch {
                namespace: namespace.to_string(),
                pod: pod.to_string(),
                expected: 1500,
                actual: new_mtu,
            },
            action: FixAction::AdjustMTU {
                namespace: namespace.to_string(),
                pod: pod.to_string(),
                new_mtu,
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
    use crate::ebpf::{DropReason, DropReasonType, MapReader, MockMapReader};

    #[test]
    fn test_no_mtu_issues_with_mock() {
        let reader = MockMapReader;
        let problems = MTUHealer::detect_mtu_issues(&reader);
        // MockMapReader returns empty drop map, so no MTU issues
        assert!(problems.is_empty());
    }

    #[test]
    fn test_detect_fragmentation_needed() {
        // Create a custom reader that returns FragmentationNeeded drops
        struct FragDropReader;
        impl MapReader for FragDropReader {
            fn read_policy_map(&self) -> anyhow::Result<Vec<crate::ebpf::PolicyDecision>> {
                Ok(vec![])
            }
            fn read_conntrack_map(&self) -> anyhow::Result<Vec<crate::ebpf::ConntrackEntry>> {
                Ok(vec![])
            }
            fn read_lb_map(&self) -> anyhow::Result<Vec<crate::ebpf::LoadBalancerEntry>> {
                Ok(vec![])
            }
            fn read_ipcache_map(&self) -> anyhow::Result<Vec<crate::ebpf::IPCacheEntry>> {
                Ok(vec![])
            }
            fn read_drop_map(&self) -> anyhow::Result<Vec<DropReason>> {
                Ok(vec![DropReason {
                    src_ip: "10.0.0.5".to_string(),
                    dst_ip: "10.0.0.6".to_string(),
                    port: 80,
                    protocol: 6,
                    reason: DropReasonType::FragmentationNeeded,
                    timestamp: 0,
                }])
            }
        }

        let reader = FragDropReader;
        let problems = MTUHealer::detect_mtu_issues(&reader);
        assert_eq!(problems.len(), 1);
        match &problems[0] {
            Problem::MTUMismatch {
                pod,
                expected,
                actual,
                ..
            } => {
                assert_eq!(pod, "10.0.0.5");
                assert_eq!(*expected, 1500);
                assert_eq!(*actual, 1450);
            }
            _ => panic!("Expected MTUMismatch problem"),
        }
    }

    #[test]
    fn test_non_fragmentation_drops_ignored() {
        struct PolicyDropReader;
        impl MapReader for PolicyDropReader {
            fn read_policy_map(&self) -> anyhow::Result<Vec<crate::ebpf::PolicyDecision>> {
                Ok(vec![])
            }
            fn read_conntrack_map(&self) -> anyhow::Result<Vec<crate::ebpf::ConntrackEntry>> {
                Ok(vec![])
            }
            fn read_lb_map(&self) -> anyhow::Result<Vec<crate::ebpf::LoadBalancerEntry>> {
                Ok(vec![])
            }
            fn read_ipcache_map(&self) -> anyhow::Result<Vec<crate::ebpf::IPCacheEntry>> {
                Ok(vec![])
            }
            fn read_drop_map(&self) -> anyhow::Result<Vec<DropReason>> {
                Ok(vec![DropReason {
                    src_ip: "10.0.0.5".to_string(),
                    dst_ip: "10.0.0.6".to_string(),
                    port: 80,
                    protocol: 6,
                    reason: DropReasonType::PolicyDenied,
                    timestamp: 0,
                }])
            }
        }

        let reader = PolicyDropReader;
        let problems = MTUHealer::detect_mtu_issues(&reader);
        assert!(problems.is_empty());
    }

    #[test]
    fn test_generate_mtu_fix() {
        let fix = MTUHealer::generate_mtu_fix("production", "web-pod", 1450);
        assert!(!fix.applied);
        assert_eq!(
            fix.action,
            FixAction::AdjustMTU {
                namespace: "production".to_string(),
                pod: "web-pod".to_string(),
                new_mtu: 1450,
            }
        );
        match &fix.problem {
            Problem::MTUMismatch {
                namespace,
                pod,
                expected,
                actual,
            } => {
                assert_eq!(namespace, "production");
                assert_eq!(pod, "web-pod");
                assert_eq!(*expected, 1500);
                assert_eq!(*actual, 1450);
            }
            _ => panic!("Expected MTUMismatch problem"),
        }
    }
}
