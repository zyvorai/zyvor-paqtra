// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::collections::HashMap;
use std::net::IpAddr;

pub use crate::ebpf::DropReasonType;

/// Root-cause engine configuration
#[derive(Debug, Clone)]
pub struct RootCauseConfig {
    /// Enable root-cause analysis
    pub enabled: bool,

    /// Analyze only recent drops (seconds)
    pub analysis_window_secs: u64,

    /// Minimum drop count to trigger analysis
    pub min_drop_count: u64,

    /// Enable automatic correlation with policies
    pub auto_correlate: bool,

    /// Enable historical pattern analysis
    pub pattern_analysis: bool,
}

impl Default for RootCauseConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            analysis_window_secs: 300, // 5 minutes
            min_drop_count: 5,
            auto_correlate: true,
            pattern_analysis: true,
        }
    }
}

/// Drop event from eBPF
#[derive(Debug, Clone)]
pub struct DropEvent {
    pub timestamp: u64,
    pub src_ip: IpAddr,
    pub dst_ip: IpAddr,
    pub src_port: u16,
    pub dst_port: u16,
    pub protocol: u8,
    pub reason: DropReason,
    pub identity_src: u32,
    pub identity_dst: u32,
    pub namespace: Option<String>,
    pub pod_name: Option<String>,
}

/// Drop reason (enhanced from eBPF)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DropReason {
    /// Policy denied the packet
    PolicyDenied,

    /// Invalid source IP
    InvalidSourceIP,

    /// Invalid packet (malformed)
    InvalidPacket,

    /// Connection tracking state mismatch
    CTStateMismatch,

    /// Port not allowed by policy
    PortNotAllowed,

    /// Unknown L3 protocol
    UnknownL3Protocol,

    /// Unknown L4 protocol
    UnknownL4Protocol,

    /// Unsupported L3 protocol
    UnsupportedL3Protocol,

    /// No mapping for destination
    NoMapping,

    /// Destination identity unknown
    UnknownDestination,

    /// Load balancer error
    LBError,

    /// Service not found
    ServiceNotFound,

    /// No healthy backend
    NoBackend,

    /// Fragment needed (MTU issue)
    FragNeeded,

    /// TTL exceeded
    TTLExceeded,

    /// Other reason
    Other(u32),
}

impl DropReason {
    pub fn from_ebpf(reason: &DropReasonType) -> Self {
        match reason {
            DropReasonType::PolicyDenied => DropReason::PolicyDenied,
            DropReasonType::InvalidPacket => DropReason::InvalidPacket,
            DropReasonType::NoRoute => DropReason::NoMapping,
            DropReasonType::UnknownL4Protocol => DropReason::UnknownL4Protocol,
            DropReasonType::FragmentationNeeded => DropReason::FragNeeded,
            DropReasonType::CTMapFull => DropReason::CTStateMismatch,
            DropReasonType::NATMapFull => DropReason::LBError,
            DropReasonType::InvalidSourceIP => DropReason::InvalidSourceIP,
            DropReasonType::InvalidDestIP => DropReason::UnknownDestination,
            DropReasonType::UnsupportedL3Protocol => DropReason::UnsupportedL3Protocol,
            DropReasonType::MissedTailCall => DropReason::Other(133u32),
            DropReasonType::ErrorWritingToPacket => DropReason::Other(134u32),
            DropReasonType::UnknownL4ICMPType => DropReason::Other(135u32),
            DropReasonType::UnknownICMPv6Type => DropReason::Other(136u32),
            DropReasonType::UnknownICMPv6Code => DropReason::Other(137u32),
            DropReasonType::ServiceBackendNotFound => DropReason::NoBackend,
            DropReasonType::NoTunnelEndpoint => DropReason::NoMapping,
            DropReasonType::HostUnreachable => DropReason::UnknownDestination,
            DropReasonType::StaleOrUnroutable => DropReason::NoMapping,
            DropReasonType::ConnectionTrackingInvalid => DropReason::CTStateMismatch,
            DropReasonType::AuthRequired => DropReason::PolicyDenied,
            DropReasonType::NATNotNeeded => DropReason::Other(184u32),
            DropReasonType::IsClusterIP => DropReason::Other(185u32),
            DropReasonType::Other(code) => DropReason::Other(*code),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            DropReason::PolicyDenied => "Policy Denied",
            DropReason::InvalidSourceIP => "Invalid Source IP",
            DropReason::InvalidPacket => "Invalid Packet",
            DropReason::CTStateMismatch => "Connection Tracking State Mismatch",
            DropReason::PortNotAllowed => "Port Not Allowed",
            DropReason::UnknownL3Protocol => "Unknown L3 Protocol",
            DropReason::UnknownL4Protocol => "Unknown L4 Protocol",
            DropReason::UnsupportedL3Protocol => "Unsupported L3 Protocol",
            DropReason::NoMapping => "No Mapping",
            DropReason::UnknownDestination => "Unknown Destination",
            DropReason::LBError => "Load Balancer Error",
            DropReason::ServiceNotFound => "Service Not Found",
            DropReason::NoBackend => "No Healthy Backend",
            DropReason::FragNeeded => "Fragment Needed (MTU)",
            DropReason::TTLExceeded => "TTL Exceeded",
            DropReason::Other(_) => "Other",
        }
    }
}

impl std::fmt::Display for DropReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Root-cause analysis result
#[derive(Debug, Clone)]
pub struct RootCauseAnalysis {
    /// The drop event being analyzed
    pub event: DropEvent,

    /// Human-readable explanation
    pub explanation: String,

    /// Likely root cause
    pub likely_cause: String,

    /// Suggested fix
    pub suggested_fix: SuggestedFix,

    /// Confidence score (0.0 to 1.0)
    pub confidence: f32,

    /// Related policy (if applicable)
    pub related_policy: Option<String>,

    /// Additional context
    pub context: Vec<String>,
}

/// Suggested fix for the drop
#[derive(Debug, Clone)]
pub enum SuggestedFix {
    /// Add a policy rule
    AddPolicyRule {
        namespace: String,
        from_labels: HashMap<String, String>,
        to_labels: HashMap<String, String>,
        port: u16,
        protocol: String,
        yaml: String,
    },

    /// Update MTU settings
    UpdateMTU {
        interface: String,
        current_mtu: u16,
        suggested_mtu: u16,
        command: String,
    },

    /// Fix DNS configuration
    FixDNS {
        namespace: String,
        issue: String,
        command: String,
    },

    /// Check connection tracking
    CheckConntrack {
        issue: String,
        commands: Vec<String>,
    },

    /// Add service endpoint
    AddServiceEndpoint {
        service: String,
        namespace: String,
        reason: String,
    },

    /// Fix load balancer
    FixLoadBalancer {
        service: String,
        namespace: String,
        issue: String,
    },

    /// Manual investigation needed
    ManualInvestigation { reason: String, steps: Vec<String> },
}

/// Drop pattern for historical analysis
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct DropPattern {
    pub reason: DropReason,
    pub src_identity: u32,
    pub dst_identity: u32,
    pub dst_port: u16,
    pub protocol: u8,
}

/// Drop statistics
#[derive(Debug, Clone)]
pub struct DropStats {
    pub total_drops: u64,
    pub by_reason: HashMap<DropReason, u64>,
    pub by_namespace: HashMap<String, u64>,
    pub top_patterns: Vec<(DropPattern, u64)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_root_cause_config_defaults() {
        let config = RootCauseConfig::default();
        assert!(config.enabled);
        assert_eq!(config.analysis_window_secs, 300);
        assert_eq!(config.min_drop_count, 5);
        assert!(config.auto_correlate);
        assert!(config.pattern_analysis);
    }

    #[test]
    fn test_drop_reason_from_ebpf_conversions() {
        assert_eq!(
            DropReason::from_ebpf(&DropReasonType::PolicyDenied),
            DropReason::PolicyDenied
        );
        assert_eq!(
            DropReason::from_ebpf(&DropReasonType::InvalidPacket),
            DropReason::InvalidPacket
        );
        assert_eq!(
            DropReason::from_ebpf(&DropReasonType::NoRoute),
            DropReason::NoMapping
        );
        assert_eq!(
            DropReason::from_ebpf(&DropReasonType::UnknownL4Protocol),
            DropReason::UnknownL4Protocol
        );
        assert_eq!(
            DropReason::from_ebpf(&DropReasonType::FragmentationNeeded),
            DropReason::FragNeeded
        );
        assert_eq!(
            DropReason::from_ebpf(&DropReasonType::CTMapFull),
            DropReason::CTStateMismatch
        );
        assert_eq!(
            DropReason::from_ebpf(&DropReasonType::NATMapFull),
            DropReason::LBError
        );
        assert_eq!(
            DropReason::from_ebpf(&DropReasonType::InvalidSourceIP),
            DropReason::InvalidSourceIP
        );
        assert_eq!(
            DropReason::from_ebpf(&DropReasonType::InvalidDestIP),
            DropReason::UnknownDestination
        );
        assert_eq!(
            DropReason::from_ebpf(&DropReasonType::UnsupportedL3Protocol),
            DropReason::UnsupportedL3Protocol
        );
        assert_eq!(
            DropReason::from_ebpf(&DropReasonType::ServiceBackendNotFound),
            DropReason::NoBackend
        );
        assert_eq!(
            DropReason::from_ebpf(&DropReasonType::AuthRequired),
            DropReason::PolicyDenied
        );
        assert_eq!(
            DropReason::from_ebpf(&DropReasonType::ConnectionTrackingInvalid),
            DropReason::CTStateMismatch
        );
        assert_eq!(
            DropReason::from_ebpf(&DropReasonType::Other(999)),
            DropReason::Other(999)
        );
    }

    #[test]
    fn test_drop_reason_to_string_all_variants() {
        assert_eq!(DropReason::PolicyDenied.to_string(), "Policy Denied");
        assert_eq!(DropReason::InvalidSourceIP.to_string(), "Invalid Source IP");
        assert_eq!(DropReason::InvalidPacket.to_string(), "Invalid Packet");
        assert_eq!(
            DropReason::CTStateMismatch.to_string(),
            "Connection Tracking State Mismatch"
        );
        assert_eq!(DropReason::PortNotAllowed.to_string(), "Port Not Allowed");
        assert_eq!(
            DropReason::UnknownL3Protocol.to_string(),
            "Unknown L3 Protocol"
        );
        assert_eq!(
            DropReason::UnknownL4Protocol.to_string(),
            "Unknown L4 Protocol"
        );
        assert_eq!(
            DropReason::UnsupportedL3Protocol.to_string(),
            "Unsupported L3 Protocol"
        );
        assert_eq!(DropReason::NoMapping.to_string(), "No Mapping");
        assert_eq!(
            DropReason::UnknownDestination.to_string(),
            "Unknown Destination"
        );
        assert_eq!(DropReason::LBError.to_string(), "Load Balancer Error");
        assert_eq!(DropReason::ServiceNotFound.to_string(), "Service Not Found");
        assert_eq!(DropReason::NoBackend.to_string(), "No Healthy Backend");
        assert_eq!(DropReason::FragNeeded.to_string(), "Fragment Needed (MTU)");
        assert_eq!(DropReason::TTLExceeded.to_string(), "TTL Exceeded");
        assert_eq!(DropReason::Other(42).to_string(), "Other");
    }

    #[test]
    fn test_suggested_fix_add_policy_rule() {
        let mut from_labels = HashMap::new();
        from_labels.insert("app".to_string(), "frontend".to_string());
        let mut to_labels = HashMap::new();
        to_labels.insert("app".to_string(), "backend".to_string());

        let fix = SuggestedFix::AddPolicyRule {
            namespace: "default".to_string(),
            from_labels,
            to_labels,
            port: 8080,
            protocol: "TCP".to_string(),
            yaml: "apiVersion: cilium.io/v2".to_string(),
        };

        if let SuggestedFix::AddPolicyRule {
            namespace,
            port,
            protocol,
            ..
        } = &fix
        {
            assert_eq!(namespace, "default");
            assert_eq!(*port, 8080);
            assert_eq!(protocol, "TCP");
        } else {
            panic!("Expected AddPolicyRule variant");
        }
    }

    #[test]
    fn test_suggested_fix_manual_investigation() {
        let fix = SuggestedFix::ManualInvestigation {
            reason: "Unknown pattern".to_string(),
            steps: vec![
                "Check logs".to_string(),
                "Inspect network policies".to_string(),
            ],
        };

        if let SuggestedFix::ManualInvestigation { reason, steps } = &fix {
            assert_eq!(reason, "Unknown pattern");
            assert_eq!(steps.len(), 2);
        } else {
            panic!("Expected ManualInvestigation variant");
        }
    }

    #[test]
    fn test_drop_pattern_creation_and_equality() {
        let pattern1 = DropPattern {
            reason: DropReason::PolicyDenied,
            src_identity: 100,
            dst_identity: 200,
            dst_port: 80,
            protocol: 6,
        };

        let pattern2 = DropPattern {
            reason: DropReason::PolicyDenied,
            src_identity: 100,
            dst_identity: 200,
            dst_port: 80,
            protocol: 6,
        };

        assert_eq!(pattern1, pattern2);

        let pattern3 = DropPattern {
            reason: DropReason::NoBackend,
            src_identity: 100,
            dst_identity: 200,
            dst_port: 80,
            protocol: 6,
        };

        assert_ne!(pattern1, pattern3);
    }

    #[test]
    fn test_drop_event_creation() {
        let event = DropEvent {
            timestamp: 1234567890,
            src_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            src_port: 45000,
            dst_port: 80,
            protocol: 6,
            reason: DropReason::PolicyDenied,
            identity_src: 100,
            identity_dst: 200,
            namespace: Some("default".to_string()),
            pod_name: Some("web-abc123".to_string()),
        };

        assert_eq!(event.dst_port, 80);
        assert_eq!(event.reason, DropReason::PolicyDenied);
        assert_eq!(event.namespace.as_deref(), Some("default"));
    }
}
