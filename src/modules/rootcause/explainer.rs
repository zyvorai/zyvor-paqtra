// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Drop Explainer
///
/// Generates human-readable explanations for packet drops
use super::*;

pub struct DropExplainer;

impl DropExplainer {
    /// Generate human-readable explanation for a drop event
    pub fn explain(event: &DropEvent) -> String {
        match &event.reason {
            DropReason::PolicyDenied => Self::explain_policy_denied(event),
            DropReason::InvalidSourceIP => {
                format!(
                    "Packet from {} was dropped because the source IP is invalid or not recognized by Cilium",
                    event.src_ip
                )
            }
            DropReason::InvalidPacket => {
                format!(
                    "Malformed packet from {} to {}:{} was dropped during parsing",
                    event.src_ip, event.dst_ip, event.dst_port
                )
            }
            DropReason::CTStateMismatch => Self::explain_ct_state_mismatch(event),
            DropReason::PortNotAllowed => {
                format!(
                    "Connection to {}:{} was blocked because port {} is not allowed by any policy",
                    event.dst_ip, event.dst_port, event.dst_port
                )
            }
            DropReason::UnknownL3Protocol => {
                "Packet dropped due to unknown or unsupported Layer 3 protocol (not IPv4/IPv6)"
                    .to_string()
            }
            DropReason::UnknownL4Protocol => {
                format!(
                    "Packet dropped due to unknown Layer 4 protocol (protocol number: {})",
                    event.protocol
                )
            }
            DropReason::UnsupportedL3Protocol => {
                "Packet dropped because the Layer 3 protocol is not supported by this policy"
                    .to_string()
            }
            DropReason::NoMapping => {
                format!(
                    "No endpoint mapping found for destination {}. The destination may not exist in this cluster.",
                    event.dst_ip
                )
            }
            DropReason::UnknownDestination => Self::explain_unknown_destination(event),
            DropReason::LBError => {
                format!(
                    "Load balancer error occurred while processing connection to {}:{}",
                    event.dst_ip, event.dst_port
                )
            }
            DropReason::ServiceNotFound => Self::explain_service_not_found(event),
            DropReason::NoBackend => Self::explain_no_backend(event),
            DropReason::FragNeeded => Self::explain_frag_needed(event),
            DropReason::TTLExceeded => {
                format!(
                    "Packet from {} to {} was dropped because TTL (Time To Live) reached zero",
                    event.src_ip, event.dst_ip
                )
            }
            DropReason::Other(code) => {
                format!(
                    "Packet dropped with reason code {} (uncommon drop reason)",
                    code
                )
            }
        }
    }

    fn explain_policy_denied(event: &DropEvent) -> String {
        let protocol = match event.protocol {
            6 => "TCP",
            17 => "UDP",
            1 => "ICMP",
            _ => "unknown protocol",
        };

        if let (Some(ns), Some(pod)) = (&event.namespace, &event.pod_name) {
            format!(
                "Policy denied {} traffic from pod {} in namespace {} to {}:{} (identity {} → {}). \
                No CiliumNetworkPolicy allows this communication.",
                protocol,
                pod,
                ns,
                event.dst_ip,
                event.dst_port,
                event.identity_src,
                event.identity_dst
            )
        } else {
            format!(
                "Policy denied {} traffic from {} to {}:{} (identity {} → {}). \
                No CiliumNetworkPolicy allows this communication.",
                protocol,
                event.src_ip,
                event.dst_ip,
                event.dst_port,
                event.identity_src,
                event.identity_dst
            )
        }
    }

    fn explain_ct_state_mismatch(event: &DropEvent) -> String {
        format!(
            "Connection tracking mismatch for traffic from {} to {}:{}. \
            This usually happens when:\n\
            • Connection table is full (max entries reached)\n\
            • Connection state was cleared/reset\n\
            • Asymmetric routing is occurring\n\
            • Connection was established before Cilium was installed",
            event.src_ip, event.dst_ip, event.dst_port
        )
    }

    fn explain_unknown_destination(event: &DropEvent) -> String {
        format!(
            "Destination identity {} is not found in Cilium's identity map. \
            Possible causes:\n\
            • Destination pod/service doesn't exist\n\
            • Identity not yet propagated to this node\n\
            • Destination is outside the cluster and no egress policy exists\n\
            Target: {}:{}",
            event.identity_dst, event.dst_ip, event.dst_port
        )
    }

    fn explain_service_not_found(event: &DropEvent) -> String {
        format!(
            "Service for {}:{} was not found in the load balancer map. \
            This means:\n\
            • No Kubernetes Service exists for this destination\n\
            • Service exists but Cilium hasn't synced it yet\n\
            • Service selector doesn't match any pods",
            event.dst_ip, event.dst_port
        )
    }

    fn explain_no_backend(event: &DropEvent) -> String {
        format!(
            "Service {}:{} exists but has no healthy backend pods. \
            Possible reasons:\n\
            • All backend pods are not ready (failing health checks)\n\
            • Backend pods were recently deleted\n\
            • Service selector doesn't match any running pods\n\
            • Backend pods exist but on nodes that are unreachable",
            event.dst_ip, event.dst_port
        )
    }

    fn explain_frag_needed(event: &DropEvent) -> String {
        format!(
            "Packet to {}:{} requires fragmentation but DF (Don't Fragment) flag is set. \
            This is an MTU mismatch issue:\n\
            • Source is trying to send packets larger than the path MTU\n\
            • Common with overlay networks (VXLAN, Geneve)\n\
            • Typical solution: reduce MTU on pod interfaces to 1450 or enable MTU discovery",
            event.dst_ip, event.dst_port
        )
    }

    /// Generate a short summary for TUI display
    pub fn short_summary(event: &DropEvent) -> String {
        match &event.reason {
            DropReason::PolicyDenied => {
                format!("Policy denied → {}:{}", event.dst_ip, event.dst_port)
            }
            DropReason::NoBackend => {
                format!("No healthy backends for port {}", event.dst_port)
            }
            DropReason::ServiceNotFound => {
                format!("Service not found for port {}", event.dst_port)
            }
            DropReason::FragNeeded => {
                format!("MTU issue to {}", event.dst_ip)
            }
            DropReason::CTStateMismatch => {
                format!("Conntrack mismatch → {}:{}", event.dst_ip, event.dst_port)
            }
            _ => {
                format!("{} → {}:{}", event.reason, event.dst_ip, event.dst_port)
            }
        }
    }

    /// Generate detailed technical explanation
    pub fn technical_details(event: &DropEvent) -> String {
        format!(
            r#"Technical Details:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Drop Reason:     {}
Timestamp:       {}
Source:          {} (identity: {})
Destination:     {} (identity: {})
Port:            {} → {}
Protocol:        {}
Namespace:       {}
Pod:             {}
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#,
            event.reason,
            event.timestamp,
            event.src_ip,
            event.identity_src,
            event.dst_ip,
            event.identity_dst,
            event.src_port,
            event.dst_port,
            Self::protocol_name(event.protocol),
            event.namespace.as_deref().unwrap_or("unknown"),
            event.pod_name.as_deref().unwrap_or("unknown")
        )
    }

    fn protocol_name(proto: u8) -> &'static str {
        match proto {
            1 => "ICMP",
            6 => "TCP",
            17 => "UDP",
            58 => "ICMPv6",
            _ => "Other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_explain_policy_denied() {
        let event = DropEvent {
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
        };

        let explanation = DropExplainer::explain(&event);
        assert!(explanation.contains("Policy denied"));
        assert!(explanation.contains("TCP"));
        assert!(explanation.contains("80"));
    }

    #[test]
    fn test_short_summary() {
        let event = DropEvent {
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
        };

        let summary = DropExplainer::short_summary(&event);
        assert!(summary.contains("Policy denied"));
    }
}
