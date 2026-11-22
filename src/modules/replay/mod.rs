// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Traffic Replay Module
///
/// Records real network traffic and replays it in different contexts
/// to validate policy changes, test migrations, and compare cluster behaviors.
///
/// Features:
/// - Flow recording from eBPF/Hubble
/// - Storage and persistence
/// - Cross-cluster replay
/// - Outcome comparison
/// - Policy validation
use anyhow::Result;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ebpf::{ConntrackEntry, MapReader, PolicyVerdict};
use crate::kubernetes::K8sClient;

pub mod types;
pub use types::*;

pub mod comparator;
pub mod player;
pub mod recorder;
pub mod storage;

/// Traffic Replay Engine
pub struct ReplayEngine<M: MapReader> {
    config: ReplayConfig,
    ebpf_reader: M,
    k8s_client: K8sClient,

    /// Current recording session
    current_recording: Option<RecordingSession>,

    /// Available recordings
    recordings: Vec<Recording>,
}

/// Active recording session
struct RecordingSession {
    recording: Recording,
    flows: Vec<RecordedFlow>,
    start_time: SystemTime,
}

impl<M: MapReader> ReplayEngine<M> {
    pub fn new(config: ReplayConfig, ebpf_reader: M, k8s_client: K8sClient) -> Self {
        // Ensure recording directory exists
        if let Err(e) = std::fs::create_dir_all(&config.recording_dir) {
            eprintln!("Failed to create recording directory: {}", e);
        }

        Self {
            config,
            ebpf_reader,
            k8s_client,
            current_recording: None,
            recordings: Vec::new(),
        }
    }

    /// Start recording traffic
    pub async fn start_recording(&mut self, name: String) -> Result<String> {
        if self.current_recording.is_some() {
            anyhow::bail!("Recording already in progress");
        }

        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        let id = format!("rec-{}-{}", now, uuid::Uuid::new_v4());

        let recording = Recording {
            id: id.clone(),
            name,
            source_cluster: self
                .k8s_client
                .get_current_context()
                .await
                .unwrap_or_else(|_| {
                    std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string())
                }),
            start_time: now,
            end_time: 0,
            flow_count: 0,
            total_bytes: 0,
            namespaces: Vec::new(),
            services: Vec::new(),
            file_path: self.config.recording_dir.join(format!("{}.json", id)),
            compressed: self.config.compress,
        };

        self.current_recording = Some(RecordingSession {
            recording,
            flows: Vec::new(),
            start_time: SystemTime::now(),
        });

        println!("🔴 Recording started: {}", id);

        Ok(id)
    }

    /// Capture current traffic
    pub async fn capture(&mut self) -> Result<usize> {
        // Read current connections first
        let connections = self.ebpf_reader.read_conntrack_map()?;

        // Get session
        let session = self
            .current_recording
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No recording in progress"))?;

        let start_time = session.start_time;
        let mut captured = 0;

        // Read IPCache for resolution
        let ipcache = self.ebpf_reader.read_ipcache_map().unwrap_or_default();

        for conn in &connections {
            let flow = Self::conntrack_to_recorded_flow(conn, &start_time, &ipcache)?;
            session.flows.push(flow);
            captured += 1;

            // Check size limits
            if session.flows.len() * std::mem::size_of::<RecordedFlow>()
                > self.config.max_recording_size
            {
                println!("⚠️  Recording size limit reached");
                break;
            }
        }

        session.recording.flow_count = session.flows.len();

        Ok(captured)
    }

    /// Stop recording and save
    pub async fn stop_recording(&mut self) -> Result<Recording> {
        let mut session = self
            .current_recording
            .take()
            .ok_or_else(|| anyhow::anyhow!("No recording in progress"))?;

        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        session.recording.end_time = now;
        session.recording.flow_count = session.flows.len();

        // Calculate total bytes
        session.recording.total_bytes = session.flows.iter().map(|f| f.bytes).sum();

        // Extract unique namespaces
        let mut namespaces: Vec<_> = session
            .flows
            .iter()
            .flat_map(|f| vec![f.src_namespace.clone(), f.dst_namespace.clone()])
            .collect();
        namespaces.sort();
        namespaces.dedup();
        session.recording.namespaces = namespaces;

        // Save to file
        use storage::RecordingStorage;
        let storage = RecordingStorage::new(self.config.recording_dir.clone());
        storage.save(&session.recording, &session.flows)?;

        println!(
            "⏹️  Recording stopped: {} ({} flows)",
            session.recording.id, session.recording.flow_count
        );

        let recording = session.recording.clone();
        self.recordings.push(recording.clone());

        Ok(recording)
    }

    /// Resolve IP to identity and namespace from IPCache entries
    fn resolve_ip_from_cache(
        ip: &str,
        ipcache: &[crate::ebpf::IPCacheEntry],
    ) -> (u32, String, HashMap<String, String>) {
        if let Some(entry) = ipcache.iter().find(|e| e.ip == ip) {
            let namespace = if entry.namespace.is_empty() {
                "unknown".to_string()
            } else {
                entry.namespace.clone()
            };
            let labels: HashMap<String, String> = entry
                .labels
                .iter()
                .filter_map(|l| {
                    let parts: Vec<&str> = l.splitn(2, '=').collect();
                    if parts.len() == 2 {
                        Some((parts[0].to_string(), parts[1].to_string()))
                    } else {
                        None
                    }
                })
                .collect();
            return (entry.identity, namespace, labels);
        }
        (0, "unknown".to_string(), HashMap::new())
    }

    /// Convert conntrack entry to recorded flow
    fn conntrack_to_recorded_flow(
        conn: &ConntrackEntry,
        start_time: &SystemTime,
        ipcache: &[crate::ebpf::IPCacheEntry],
    ) -> Result<RecordedFlow> {
        let now = SystemTime::now();
        let offset_ms = now.duration_since(*start_time)?.as_millis() as u64;

        let timestamp = now.duration_since(UNIX_EPOCH)?.as_secs();

        let src_ip: IpAddr = conn
            .src_ip
            .parse()
            .unwrap_or_else(|_| IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)));
        let dst_ip: IpAddr = conn
            .dst_ip
            .parse()
            .unwrap_or_else(|_| IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)));

        let (src_identity, src_namespace, src_labels) =
            Self::resolve_ip_from_cache(&conn.src_ip, ipcache);
        let (dst_identity, dst_namespace, dst_labels) =
            Self::resolve_ip_from_cache(&conn.dst_ip, ipcache);

        Ok(RecordedFlow {
            timestamp,
            offset_ms,
            src_ip,
            dst_ip,
            src_port: conn.src_port,
            dst_port: conn.dst_port,
            protocol: conn.protocol,
            src_identity,
            dst_identity,
            src_namespace,
            dst_namespace,
            src_labels,
            dst_labels,
            verdict: PolicyVerdict::Allow, // Assume allowed if in conntrack
            bytes: conn.bytes,
            packets: conn.packets,
            http_method: None,
            http_path: None,
            http_status: None,
        })
    }

    /// Replay a recording
    pub async fn replay(
        &mut self,
        recording_id: &str,
        filter: Option<ReplayFilter>,
    ) -> Result<ReplayResult> {
        use comparator::ReplayComparator;
        use player::ReplayPlayer;

        // Load recording
        let recording = self.load_recording(recording_id)?;

        println!(
            "▶️  Replaying: {} ({} flows)",
            recording.name, recording.flow_count
        );

        // Load flows
        use storage::RecordingStorage;
        let storage = RecordingStorage::new(self.config.recording_dir.clone());
        let mut flows = storage.load(&recording)?;

        // Apply filter
        if let Some(f) = &filter {
            flows = Self::apply_filter(flows, f);
        }

        let start = std::time::Instant::now();

        // Replay flows
        let player =
            ReplayPlayer::new(self.config.replay_rate, &self.ebpf_reader, &self.k8s_client);

        let replay_outcomes = player.replay(&flows).await?;

        let replay_duration_ms = start.elapsed().as_millis() as u64;

        // Compare if enabled
        let comparison = if self.config.detailed_comparison {
            let comparator = ReplayComparator::new();
            Some(comparator.compare(&flows, &replay_outcomes)?)
        } else {
            None
        };

        // Count successes/failures
        let flows_successful = replay_outcomes.iter().filter(|o| o.success).count();

        let flows_failed = replay_outcomes.iter().filter(|o| !o.success).count();

        let errors = replay_outcomes
            .iter()
            .filter_map(|o| o.error.clone())
            .collect();

        Ok(ReplayResult {
            recording,
            // Remote cluster replay is not yet supported; we resolve the local
            // cluster name from the current K8s context so the result accurately
            // reflects where the replay executed.
            target_cluster: self
                .k8s_client
                .get_current_context()
                .await
                .unwrap_or_else(|_| "local".to_string()),
            flows_attempted: flows.len(),
            flows_successful,
            flows_failed,
            comparison,
            replay_duration_ms,
            errors,
        })
    }

    /// Apply filter to flows
    fn apply_filter(flows: Vec<RecordedFlow>, filter: &ReplayFilter) -> Vec<RecordedFlow> {
        let mut filtered = flows;

        if let Some(namespaces) = &filter.namespaces {
            filtered.retain(|f| {
                namespaces.contains(&f.src_namespace) || namespaces.contains(&f.dst_namespace)
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

    /// Load recording metadata
    fn load_recording(&mut self, recording_id: &str) -> Result<Recording> {
        // Try to find in memory
        if let Some(rec) = self.recordings.iter().find(|r| r.id == recording_id) {
            return Ok(rec.clone());
        }

        // Load from disk
        use storage::RecordingStorage;
        let storage = RecordingStorage::new(self.config.recording_dir.clone());
        let recording = storage.load_metadata(recording_id)?;

        self.recordings.push(recording.clone());

        Ok(recording)
    }

    /// List available recordings
    pub fn list_recordings(&mut self) -> Result<Vec<Recording>> {
        use storage::RecordingStorage;
        let storage = RecordingStorage::new(self.config.recording_dir.clone());
        let recordings = storage.list()?;

        self.recordings = recordings.clone();

        Ok(recordings)
    }

    /// Delete a recording
    pub fn delete_recording(&mut self, recording_id: &str) -> Result<()> {
        use storage::RecordingStorage;
        let storage = RecordingStorage::new(self.config.recording_dir.clone());
        storage.delete(recording_id)?;

        self.recordings.retain(|r| r.id != recording_id);

        Ok(())
    }

    /// Get cached recordings (without refreshing from disk)
    pub fn recordings(&self) -> &[Recording] {
        &self.recordings
    }

    /// Load recorded flows for a specific recording.
    /// Returns the flows or an error if the recording doesn't exist.
    pub fn load_recording_flows(&self, recording_id: &str) -> Result<Vec<RecordedFlow>> {
        if let Some(rec) = self.recordings.iter().find(|r| r.id == recording_id) {
            use storage::RecordingStorage;
            let storage = RecordingStorage::new(self.config.recording_dir.clone());
            storage.load(rec)
        } else {
            anyhow::bail!("Recording not found: {}", recording_id)
        }
    }

    /// Get statistics
    pub fn stats(&self) -> ReplayStats {
        ReplayStats {
            total_recordings: self.recordings.len(),
            recording_in_progress: self.current_recording.is_some(),
            total_flows_recorded: self.recordings.iter().map(|r| r.flow_count).sum(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ebpf::MockMapReader;

    #[ignore = "needs a Kubernetes cluster"]
    #[tokio::test]
    async fn test_replay_engine_creation() {
        let config = ReplayConfig::default();
        let reader = MockMapReader;
        let k8s_client = K8sClient::new().await.unwrap();

        let engine = ReplayEngine::new(config, reader, k8s_client);
        assert_eq!(engine.recordings.len(), 0);
    }

    #[test]
    fn test_replay_filter_default() {
        let filter = ReplayFilter::default();
        assert!(filter.namespaces.is_none());
        assert!(filter.limit.is_none());
    }

    #[ignore = "needs a Kubernetes cluster"]
    #[tokio::test]
    async fn test_start_recording() {
        let config = ReplayConfig::default();
        let reader = MockMapReader;
        let k8s_client = K8sClient::new().await.unwrap();

        let mut engine = ReplayEngine::new(config, reader, k8s_client);
        let id = engine.start_recording("test".to_string()).await.unwrap();

        assert!(!id.is_empty());
        assert!(engine.current_recording.is_some());
    }
}
