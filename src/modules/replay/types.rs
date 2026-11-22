// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;

use crate::ebpf::PolicyVerdict;

/// Replay configuration
#[derive(Debug, Clone)]
pub struct ReplayConfig {
    /// Enable replay system
    pub enabled: bool,

    /// Recording directory
    pub recording_dir: PathBuf,

    /// Maximum recording size (bytes)
    pub max_recording_size: usize,

    /// Maximum recording duration (seconds)
    pub max_recording_duration: u64,

    /// Enable compression
    pub compress: bool,

    /// Replay rate multiplier (1.0 = real-time)
    pub replay_rate: f32,

    /// Enable detailed comparison
    pub detailed_comparison: bool,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            recording_dir: PathBuf::from("/tmp/paqtra/recordings"),
            max_recording_size: 100 * 1024 * 1024, // 100 MB
            max_recording_duration: 300,           // 5 minutes
            compress: true,
            replay_rate: 1.0,
            detailed_comparison: true,
        }
    }
}

/// Recorded flow
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecordedFlow {
    /// Timestamp when recorded
    pub timestamp: u64,

    /// Time offset from recording start (for replay timing)
    pub offset_ms: u64,

    /// Source IP
    pub src_ip: IpAddr,

    /// Destination IP
    pub dst_ip: IpAddr,

    /// Source port
    pub src_port: u16,

    /// Destination port
    pub dst_port: u16,

    /// Protocol (6=TCP, 17=UDP, etc.)
    pub protocol: u8,

    /// Source identity
    pub src_identity: u32,

    /// Destination identity
    pub dst_identity: u32,

    /// Source namespace
    pub src_namespace: String,

    /// Destination namespace
    pub dst_namespace: String,

    /// Source pod labels
    pub src_labels: HashMap<String, String>,

    /// Destination pod labels
    pub dst_labels: HashMap<String, String>,

    /// Policy verdict
    pub verdict: PolicyVerdict,

    /// Bytes transferred
    pub bytes: u64,

    /// Packets count
    pub packets: u64,

    /// HTTP method (if L7)
    pub http_method: Option<String>,

    /// HTTP path (if L7)
    pub http_path: Option<String>,

    /// HTTP status (if L7)
    pub http_status: Option<u16>,
}

/// Recording metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Recording {
    /// Recording ID
    pub id: String,

    /// Name/description
    pub name: String,

    /// Source cluster
    pub source_cluster: String,

    /// Recording start time
    pub start_time: u64,

    /// Recording end time
    pub end_time: u64,

    /// Total flows recorded
    pub flow_count: usize,

    /// Total bytes
    pub total_bytes: u64,

    /// Unique namespaces
    pub namespaces: Vec<String>,

    /// Unique services
    pub services: Vec<String>,

    /// File path
    pub file_path: PathBuf,

    /// Compressed
    pub compressed: bool,
}

/// Replay result
#[derive(Debug, Clone)]
pub struct ReplayResult {
    /// Recording that was replayed
    pub recording: Recording,

    /// Target cluster
    pub target_cluster: String,

    /// Flows attempted
    pub flows_attempted: usize,

    /// Flows successful
    pub flows_successful: usize,

    /// Flows failed
    pub flows_failed: usize,

    /// Comparison results
    pub comparison: Option<ComparisonResult>,

    /// Replay duration
    pub replay_duration_ms: u64,

    /// Errors encountered
    pub errors: Vec<String>,
}

/// Comparison between original and replay
#[derive(Debug, Clone)]
pub struct ComparisonResult {
    /// Total flows compared
    pub total_flows: usize,

    /// Flows with identical behavior
    pub identical: usize,

    /// Flows with different verdicts
    pub verdict_changed: usize,

    /// Flows with different latency
    pub latency_changed: usize,

    /// New drops (worked before, dropped now)
    pub new_drops: Vec<FlowDifference>,

    /// Fixed drops (dropped before, works now)
    pub fixed_drops: Vec<FlowDifference>,

    /// Performance differences
    pub performance: PerformanceDifference,

    /// Overall similarity score (0.0 to 1.0)
    pub similarity_score: f32,
}

/// Difference between original and replayed flow
#[derive(Debug, Clone)]
pub struct FlowDifference {
    pub flow: RecordedFlow,
    pub original_verdict: PolicyVerdict,
    pub replay_verdict: PolicyVerdict,
    pub original_latency_ms: Option<f64>,
    pub replay_latency_ms: Option<f64>,
}

/// Performance difference
#[derive(Debug, Clone)]
pub struct PerformanceDifference {
    pub avg_latency_original_ms: f64,
    pub avg_latency_replay_ms: f64,
    pub latency_delta_percent: f64,
    pub throughput_original_mbps: f64,
    pub throughput_replay_mbps: f64,
    pub throughput_delta_percent: f64,
}

/// Replay filter
#[derive(Debug, Clone, Default)]
pub struct ReplayFilter {
    /// Filter by namespace
    pub namespaces: Option<Vec<String>>,

    /// Filter by source labels
    pub src_labels: Option<HashMap<String, String>>,

    /// Filter by destination labels
    pub dst_labels: Option<HashMap<String, String>>,

    /// Filter by ports
    pub ports: Option<Vec<u16>>,

    /// Filter by protocols
    pub protocols: Option<Vec<u8>>,

    /// Filter by verdict
    pub verdicts: Option<Vec<PolicyVerdict>>,

    /// Limit flow count
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ReplayStats {
    pub total_recordings: usize,
    pub recording_in_progress: bool,
    pub total_flows_recorded: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_replay_config_defaults() {
        let config = ReplayConfig::default();
        assert!(config.enabled);
        assert_eq!(
            config.recording_dir,
            PathBuf::from("/tmp/paqtra/recordings")
        );
        assert_eq!(config.max_recording_size, 100 * 1024 * 1024);
        assert_eq!(config.max_recording_duration, 300);
        assert!(config.compress);
        assert!((config.replay_rate - 1.0).abs() < f32::EPSILON);
        assert!(config.detailed_comparison);
    }

    #[test]
    fn test_recorded_flow_creation() {
        let flow = RecordedFlow {
            timestamp: 1234567890,
            offset_ms: 500,
            src_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            src_port: 45000,
            dst_port: 80,
            protocol: 6,
            src_identity: 100,
            dst_identity: 200,
            src_namespace: "default".to_string(),
            dst_namespace: "backend".to_string(),
            src_labels: HashMap::new(),
            dst_labels: HashMap::new(),
            verdict: PolicyVerdict::Allow,
            bytes: 1024,
            packets: 10,
            http_method: Some("GET".to_string()),
            http_path: Some("/api/v1/health".to_string()),
            http_status: Some(200),
        };

        assert_eq!(flow.timestamp, 1234567890);
        assert_eq!(flow.offset_ms, 500);
        assert_eq!(flow.dst_port, 80);
        assert_eq!(flow.protocol, 6);
        assert_eq!(flow.verdict, PolicyVerdict::Allow);
        assert_eq!(flow.http_method.as_deref(), Some("GET"));
        assert_eq!(flow.http_status, Some(200));
    }

    #[test]
    fn test_replay_filter_default_all_none() {
        let filter = ReplayFilter::default();
        assert!(filter.namespaces.is_none());
        assert!(filter.src_labels.is_none());
        assert!(filter.dst_labels.is_none());
        assert!(filter.ports.is_none());
        assert!(filter.protocols.is_none());
        assert!(filter.verdicts.is_none());
        assert!(filter.limit.is_none());
    }

    #[test]
    fn test_recording_creation() {
        let recording = Recording {
            id: "rec-001".to_string(),
            name: "Production traffic capture".to_string(),
            source_cluster: "prod-us-east-1".to_string(),
            start_time: 1000,
            end_time: 2000,
            flow_count: 5000,
            total_bytes: 1_048_576,
            namespaces: vec!["default".to_string(), "kube-system".to_string()],
            services: vec!["web".to_string(), "api".to_string()],
            file_path: PathBuf::from("/tmp/paqtra/recordings/rec-001.bin"),
            compressed: true,
        };

        assert_eq!(recording.id, "rec-001");
        assert_eq!(recording.name, "Production traffic capture");
        assert_eq!(recording.source_cluster, "prod-us-east-1");
        assert_eq!(recording.end_time - recording.start_time, 1000);
        assert_eq!(recording.flow_count, 5000);
        assert_eq!(recording.namespaces.len(), 2);
        assert_eq!(recording.services.len(), 2);
        assert!(recording.compressed);
    }

    #[test]
    fn test_replay_stats_creation() {
        let stats = ReplayStats {
            total_recordings: 3,
            recording_in_progress: false,
            total_flows_recorded: 15000,
        };

        assert_eq!(stats.total_recordings, 3);
        assert!(!stats.recording_in_progress);
        assert_eq!(stats.total_flows_recorded, 15000);
    }

    #[test]
    fn test_recorded_flow_without_http_fields() {
        let flow = RecordedFlow {
            timestamp: 1000,
            offset_ms: 0,
            src_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
            src_port: 50000,
            dst_port: 53,
            protocol: 17, // UDP
            src_identity: 1,
            dst_identity: 2,
            src_namespace: "default".to_string(),
            dst_namespace: "kube-system".to_string(),
            src_labels: HashMap::new(),
            dst_labels: HashMap::new(),
            verdict: PolicyVerdict::Allow,
            bytes: 64,
            packets: 1,
            http_method: None,
            http_path: None,
            http_status: None,
        };

        assert_eq!(flow.protocol, 17);
        assert!(flow.http_method.is_none());
        assert!(flow.http_path.is_none());
        assert!(flow.http_status.is_none());
    }
}
