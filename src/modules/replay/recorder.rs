// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Traffic Recorder
///
/// Helper utilities for recording traffic
use super::*;
use anyhow::Result;

pub struct TrafficRecorder;

impl TrafficRecorder {
    /// Create a snapshot of current traffic
    pub fn snapshot_traffic<M: MapReader>(ebpf_reader: &M) -> Result<Vec<ConntrackEntry>> {
        ebpf_reader.read_conntrack_map()
    }

    /// Filter flows by criteria
    pub fn filter_flows(flows: Vec<RecordedFlow>, filter: &ReplayFilter) -> Vec<RecordedFlow> {
        let mut filtered = flows;

        if let Some(namespaces) = &filter.namespaces {
            filtered.retain(|f| {
                namespaces.contains(&f.src_namespace) || namespaces.contains(&f.dst_namespace)
            });
        }

        if let Some(src_labels) = &filter.src_labels {
            filtered.retain(|f| {
                src_labels
                    .iter()
                    .all(|(k, v)| f.src_labels.get(k).map(|fv| fv == v).unwrap_or(false))
            });
        }

        if let Some(dst_labels) = &filter.dst_labels {
            filtered.retain(|f| {
                dst_labels
                    .iter()
                    .all(|(k, v)| f.dst_labels.get(k).map(|fv| fv == v).unwrap_or(false))
            });
        }

        if let Some(ports) = &filter.ports {
            filtered.retain(|f| ports.contains(&f.dst_port));
        }

        if let Some(protocols) = &filter.protocols {
            filtered.retain(|f| protocols.contains(&f.protocol));
        }

        if let Some(verdicts) = &filter.verdicts {
            filtered.retain(|f| verdicts.contains(&f.verdict));
        }

        if let Some(limit) = filter.limit {
            filtered.truncate(limit);
        }

        filtered
    }

    /// Deduplicate flows (remove exact duplicates)
    pub fn deduplicate_flows(flows: Vec<RecordedFlow>) -> Vec<RecordedFlow> {
        use std::collections::HashSet;

        let mut seen = HashSet::new();
        let mut unique = Vec::new();

        for flow in flows {
            let key = (
                flow.src_ip,
                flow.dst_ip,
                flow.src_port,
                flow.dst_port,
                flow.protocol,
            );

            if seen.insert(key) {
                unique.push(flow);
            }
        }

        unique
    }

    /// Sample flows (take every Nth flow)
    pub fn sample_flows(flows: Vec<RecordedFlow>, sample_rate: usize) -> Vec<RecordedFlow> {
        if sample_rate <= 1 {
            return flows;
        }

        flows
            .into_iter()
            .enumerate()
            .filter(|(i, _)| i % sample_rate == 0)
            .map(|(_, f)| f)
            .collect()
    }

    /// Aggregate statistics from flows
    pub fn aggregate_stats(flows: &[RecordedFlow]) -> RecordingStats {
        let total_flows = flows.len();

        let total_bytes: u64 = flows.iter().map(|f| f.bytes).sum();
        let total_packets: u64 = flows.iter().map(|f| f.packets).sum();

        let mut by_protocol: HashMap<u8, usize> = HashMap::new();
        for flow in flows {
            *by_protocol.entry(flow.protocol).or_insert(0) += 1;
        }

        let mut by_namespace: HashMap<String, usize> = HashMap::new();
        for flow in flows {
            *by_namespace.entry(flow.src_namespace.clone()).or_insert(0) += 1;
        }

        let allowed = flows
            .iter()
            .filter(|f| f.verdict == PolicyVerdict::Allow)
            .count();

        let denied = flows
            .iter()
            .filter(|f| f.verdict == PolicyVerdict::Deny)
            .count();

        RecordingStats {
            total_flows,
            total_bytes,
            total_packets,
            by_protocol,
            by_namespace,
            allowed,
            denied,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RecordingStats {
    pub total_flows: usize,
    pub total_bytes: u64,
    pub total_packets: u64,
    pub by_protocol: HashMap<u8, usize>,
    pub by_namespace: HashMap<String, usize>,
    pub allowed: usize,
    pub denied: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_filter_flows() {
        let flows = vec![
            RecordedFlow {
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
            },
            RecordedFlow {
                timestamp: 1001,
                offset_ms: 1,
                src_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3)),
                dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 4)),
                src_port: 54321,
                dst_port: 443,
                protocol: 6,
                src_identity: 300,
                dst_identity: 400,
                src_namespace: "prod".to_string(),
                dst_namespace: "prod".to_string(),
                src_labels: HashMap::new(),
                dst_labels: HashMap::new(),
                verdict: PolicyVerdict::Allow,
                bytes: 2048,
                packets: 20,
                http_method: None,
                http_path: None,
                http_status: None,
            },
        ];

        let filter = ReplayFilter {
            ports: Some(vec![80]),
            ..Default::default()
        };

        let filtered = TrafficRecorder::filter_flows(flows, &filter);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].dst_port, 80);
    }

    #[test]
    fn test_deduplicate_flows() {
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

        let flows = vec![flow.clone(), flow.clone(), flow];
        let unique = TrafficRecorder::deduplicate_flows(flows);

        assert_eq!(unique.len(), 1);
    }

    #[test]
    fn test_sample_flows() {
        let flows: Vec<_> = (0..10)
            .map(|i| RecordedFlow {
                timestamp: 1000 + i,
                offset_ms: i as u64,
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
            })
            .collect();

        let sampled = TrafficRecorder::sample_flows(flows, 2);
        assert_eq!(sampled.len(), 5); // Every other flow
    }

    #[test]
    fn test_aggregate_stats() {
        let flows = vec![
            RecordedFlow {
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
            },
            RecordedFlow {
                timestamp: 1001,
                offset_ms: 1,
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
                verdict: PolicyVerdict::Deny,
                bytes: 512,
                packets: 5,
                http_method: None,
                http_path: None,
                http_status: None,
            },
        ];

        let stats = TrafficRecorder::aggregate_stats(&flows);

        assert_eq!(stats.total_flows, 2);
        assert_eq!(stats.total_bytes, 1536);
        assert_eq!(stats.total_packets, 15);
        assert_eq!(stats.allowed, 1);
        assert_eq!(stats.denied, 1);
    }
}
