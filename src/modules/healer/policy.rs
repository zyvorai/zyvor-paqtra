// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Policy-specific healing logic
use super::*;
use crate::modules::{sanitize_k8s_name, yaml_escape};

pub struct PolicyHealer;

impl PolicyHealer {
    pub fn detect_policy_gaps(drops: &[DropReason]) -> Vec<Problem> {
        let mut problems = Vec::new();

        for drop in drops {
            if drop.reason == DropReasonType::PolicyDenied {
                problems.push(Problem::PolicyGap {
                    src_namespace: "unknown".to_string(),
                    src_pod: drop.src_ip.clone(),
                    dst_namespace: "unknown".to_string(),
                    dst_pod: drop.dst_ip.clone(),
                    port: drop.port,
                    protocol: protocol_name(drop.protocol),
                });
            }
        }

        problems
    }

    pub fn suggest_policy(src: &str, dst: &str, port: u16, protocol: &str) -> String {
        format!(
            r#"
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: auto-allow-{}-to-{}
spec:
  endpointSelector:
    matchLabels:
      app: {}
  egress:
    - toEndpoints:
        - matchLabels:
            app: {}
      toPorts:
        - ports:
            - port: "{}"
              protocol: {}
"#,
            sanitize_k8s_name(src),
            sanitize_k8s_name(dst),
            yaml_escape(src),
            yaml_escape(dst),
            port,
            yaml_escape(protocol)
        )
    }
}

fn protocol_name(proto_num: u8) -> String {
    match proto_num {
        6 => "TCP".to_string(),
        17 => "UDP".to_string(),
        _ => format!("{}", proto_num),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ebpf::{DropReason, DropReasonType};

    fn make_policy_drop(src: &str, dst: &str, port: u16, protocol: u8) -> DropReason {
        DropReason {
            src_ip: src.to_string(),
            dst_ip: dst.to_string(),
            port,
            protocol,
            reason: DropReasonType::PolicyDenied,
            timestamp: 0,
        }
    }

    #[test]
    fn test_no_policy_gaps_empty() {
        let drops: Vec<DropReason> = vec![];
        let problems = PolicyHealer::detect_policy_gaps(&drops);
        assert!(problems.is_empty());
    }

    #[test]
    fn test_detect_policy_gap() {
        let drops = vec![make_policy_drop("10.0.0.1", "10.0.0.2", 80, 6)];
        let problems = PolicyHealer::detect_policy_gaps(&drops);
        assert_eq!(problems.len(), 1);
        match &problems[0] {
            Problem::PolicyGap {
                src_pod,
                dst_pod,
                port,
                protocol,
                ..
            } => {
                assert_eq!(src_pod, "10.0.0.1");
                assert_eq!(dst_pod, "10.0.0.2");
                assert_eq!(*port, 80);
                assert_eq!(protocol, "TCP");
            }
            other => unreachable!("Expected PolicyGap, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_policy_gap_udp() {
        let drops = vec![make_policy_drop("10.0.0.1", "10.96.0.10", 53, 17)];
        let problems = PolicyHealer::detect_policy_gaps(&drops);
        assert_eq!(problems.len(), 1);
        match &problems[0] {
            Problem::PolicyGap { port, protocol, .. } => {
                assert_eq!(*port, 53);
                assert_eq!(protocol, "UDP");
            }
            other => unreachable!("Expected PolicyGap, got {:?}", other),
        }
    }

    #[test]
    fn test_non_policy_drops_ignored() {
        let drops = vec![
            DropReason {
                src_ip: "10.0.0.1".to_string(),
                dst_ip: "10.0.0.2".to_string(),
                port: 80,
                protocol: 6,
                reason: DropReasonType::FragmentationNeeded,
                timestamp: 0,
            },
            DropReason {
                src_ip: "10.0.0.1".to_string(),
                dst_ip: "10.0.0.2".to_string(),
                port: 80,
                protocol: 6,
                reason: DropReasonType::NoRoute,
                timestamp: 0,
            },
        ];
        let problems = PolicyHealer::detect_policy_gaps(&drops);
        assert!(problems.is_empty());
    }

    #[test]
    fn test_multiple_policy_gaps() {
        let drops = vec![
            make_policy_drop("10.0.0.1", "10.0.0.2", 80, 6),
            make_policy_drop("10.0.0.3", "10.0.0.4", 443, 6),
            make_policy_drop("10.0.0.5", "10.0.0.6", 5432, 6),
        ];
        let problems = PolicyHealer::detect_policy_gaps(&drops);
        assert_eq!(problems.len(), 3);
    }

    #[test]
    fn test_suggest_policy_tcp() {
        let yaml = PolicyHealer::suggest_policy("frontend", "backend", 80, "TCP");
        assert!(yaml.contains("CiliumNetworkPolicy"));
        assert!(yaml.contains("auto-allow-frontend-to-backend"));
        assert!(yaml.contains("app: \"frontend\""));
        assert!(yaml.contains("app: \"backend\""));
        assert!(yaml.contains("port: \"80\""));
        assert!(yaml.contains("protocol: \"TCP\""));
    }

    #[test]
    fn test_suggest_policy_udp_dns() {
        let yaml = PolicyHealer::suggest_policy("app", "kube-dns", 53, "UDP");
        assert!(yaml.contains("auto-allow-app-to-kube-dns"));
        assert!(yaml.contains("port: \"53\""));
        assert!(yaml.contains("protocol: \"UDP\""));
    }

    #[test]
    fn test_protocol_name_tcp() {
        assert_eq!(protocol_name(6), "TCP");
    }

    #[test]
    fn test_protocol_name_udp() {
        assert_eq!(protocol_name(17), "UDP");
    }

    #[test]
    fn test_protocol_name_other() {
        assert_eq!(protocol_name(1), "1");
        assert_eq!(protocol_name(0), "0");
    }
}
