// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Root-Cause Engine Module
///
/// Analyzes packet drops and provides human-readable explanations
/// with actionable fixes.
///
/// Features:
/// - Drop reason analysis from eBPF
/// - Policy correlation
/// - Human-readable explanations
/// - Actionable fix suggestions
/// - Historical pattern analysis
use anyhow::Result;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ebpf::{DropReason as EbpfDropReason, MapReader, PolicyDecision};
use crate::kubernetes::K8sClient;
use crate::modules::yaml_escape;
use crate::policies::PolicyManager;

pub mod types;
pub use types::*;

pub mod analyzer;
pub mod correlator;
pub mod explainer;

/// Root-Cause Engine
pub struct RootCauseEngine<M: MapReader> {
    config: RootCauseConfig,
    ebpf_reader: M,
    k8s_client: K8sClient,
    policy_manager: PolicyManager,

    /// Historical drop events
    drop_history: Vec<DropEvent>,

    /// Drop pattern counts
    pattern_counts: HashMap<DropPattern, u64>,
}

impl<M: MapReader> RootCauseEngine<M> {
    pub fn new(config: RootCauseConfig, ebpf_reader: M, k8s_client: K8sClient) -> Self {
        let policy_manager = PolicyManager::new(k8s_client.clone());

        Self {
            config,
            ebpf_reader,
            k8s_client,
            policy_manager,
            drop_history: Vec::new(),
            pattern_counts: HashMap::new(),
        }
    }

    /// Analyze current drops
    pub async fn analyze_drops(&mut self) -> Result<Vec<RootCauseAnalysis>> {
        if !self.config.enabled {
            return Ok(Vec::new());
        }

        // Read drops from eBPF
        let ebpf_drops = self.ebpf_reader.read_drop_map()?;

        // Pre-read IPCache once for all IP resolutions in this analysis cycle
        let ipcache = self.ebpf_reader.read_ipcache_map().unwrap_or_default();

        // Convert to drop events
        let mut new_events = Vec::new();
        for ebpf_drop in &ebpf_drops {
            let event = self.ebpf_drop_to_event(ebpf_drop, &ipcache).await?;
            new_events.push(event);
        }

        // Update history
        self.update_history(new_events.clone());

        // Analyze each significant drop pattern
        let mut analyses = Vec::new();
        let significant_drops = self.find_significant_drops();

        for drop in significant_drops {
            let analysis = self.analyze_single_drop(&drop).await?;
            analyses.push(analysis);
        }

        Ok(analyses)
    }

    /// Resolve labels for a security identity via IPCache.
    /// Falls back to a single "security.identity" label if not found.
    fn resolve_labels_for_identity(
        &self,
        identity: u32,
        ipcache: &[crate::ebpf::IPCacheEntry],
    ) -> HashMap<String, String> {
        if let Some(entry) = ipcache.iter().find(|e| e.identity == identity) {
            let labels: HashMap<String, String> = entry
                .labels
                .iter()
                .filter_map(|l| l.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            if !labels.is_empty() {
                return labels;
            }
        }
        HashMap::from([("security.identity".to_string(), identity.to_string())])
    }

    /// Resolve an IP address to identity and namespace via a pre-read IPCache
    fn resolve_ip_info(
        &self,
        ip: &str,
        ipcache: &[crate::ebpf::IPCacheEntry],
    ) -> (u32, Option<String>) {
        if let Some(entry) = ipcache.iter().find(|e| e.ip == ip) {
            let namespace = if entry.namespace.is_empty() {
                None
            } else {
                Some(entry.namespace.clone())
            };
            return (entry.identity, namespace);
        }
        (0, None)
    }

    /// Convert eBPF drop to drop event
    async fn ebpf_drop_to_event(
        &self,
        ebpf_drop: &EbpfDropReason,
        ipcache: &[crate::ebpf::IPCacheEntry],
    ) -> Result<DropEvent> {
        // Parse IP addresses, logging warnings on failure
        let src_ip: IpAddr = ebpf_drop.src_ip.parse().unwrap_or_else(|e| {
            tracing::warn!(ip = %ebpf_drop.src_ip, error = %e, "Failed to parse source IP in drop event");
            IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)
        });
        let dst_ip: IpAddr = ebpf_drop.dst_ip.parse().unwrap_or_else(|e| {
            tracing::warn!(ip = %ebpf_drop.dst_ip, error = %e, "Failed to parse destination IP in drop event");
            IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)
        });

        // Resolve IPs to pod info via pre-read IPCache
        let (identity_src, src_ns) = self.resolve_ip_info(&ebpf_drop.src_ip, ipcache);
        let (identity_dst, dst_ns) = self.resolve_ip_info(&ebpf_drop.dst_ip, ipcache);

        let event = DropEvent {
            timestamp: ebpf_drop.timestamp,
            src_ip,
            dst_ip,
            src_port: 0,
            dst_port: ebpf_drop.port,
            protocol: ebpf_drop.protocol,
            reason: DropReason::from_ebpf(&ebpf_drop.reason),
            identity_src,
            identity_dst,
            namespace: src_ns.or(dst_ns),
            pod_name: None,
        };

        Ok(event)
    }

    /// Update drop history
    fn update_history(&mut self, new_events: Vec<DropEvent>) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Update pattern counts only for new events
        for event in &new_events {
            let pattern = DropPattern {
                reason: event.reason.clone(),
                src_identity: event.identity_src,
                dst_identity: event.identity_dst,
                dst_port: event.dst_port,
                protocol: event.protocol,
            };
            *self.pattern_counts.entry(pattern).or_insert(0) += 1;
        }

        // Add new events
        self.drop_history.extend(new_events);

        // Cap history size to prevent unbounded growth
        const MAX_HISTORY: usize = 10_000;
        if self.drop_history.len() > MAX_HISTORY {
            // Remove oldest entries (front of the vec) and decrement their pattern counts
            let excess = self.drop_history.len() - MAX_HISTORY;
            for event in self.drop_history.drain(..excess) {
                let pattern = DropPattern {
                    reason: event.reason.clone(),
                    src_identity: event.identity_src,
                    dst_identity: event.identity_dst,
                    dst_port: event.dst_port,
                    protocol: event.protocol,
                };
                if let Some(count) = self.pattern_counts.get_mut(&pattern) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        self.pattern_counts.remove(&pattern);
                    }
                }
            }
        }

        // Clean old events and decrement their pattern counts
        let cutoff = now.saturating_sub(self.config.analysis_window_secs);
        self.drop_history.retain(|e| {
            if e.timestamp >= cutoff {
                true
            } else {
                let pattern = DropPattern {
                    reason: e.reason.clone(),
                    src_identity: e.identity_src,
                    dst_identity: e.identity_dst,
                    dst_port: e.dst_port,
                    protocol: e.protocol,
                };
                if let Some(count) = self.pattern_counts.get_mut(&pattern) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        self.pattern_counts.remove(&pattern);
                    }
                }
                false
            }
        });
    }

    /// Find significant drops (above threshold)
    fn find_significant_drops(&self) -> Vec<DropEvent> {
        let mut significant = Vec::new();

        for (pattern, count) in &self.pattern_counts {
            if *count >= self.config.min_drop_count {
                // Find a representative event for this pattern
                if let Some(event) = self.drop_history.iter().find(|e| {
                    e.reason == pattern.reason
                        && e.identity_src == pattern.src_identity
                        && e.identity_dst == pattern.dst_identity
                        && e.dst_port == pattern.dst_port
                        && e.protocol == pattern.protocol
                }) {
                    significant.push(event.clone());
                }
            }
        }

        significant
    }

    /// Analyze a single drop event
    async fn analyze_single_drop(&self, event: &DropEvent) -> Result<RootCauseAnalysis> {
        use correlator::PolicyCorrelator;
        use explainer::DropExplainer;

        // Get human-readable explanation
        let explanation = DropExplainer::explain(event);

        // Correlate with policies if enabled
        let (related_policy, policy_context) = if self.config.auto_correlate {
            PolicyCorrelator::correlate_with_policy(event, &self.ebpf_reader).await?
        } else {
            (None, Vec::new())
        };

        // Determine likely cause
        let likely_cause = self.determine_likely_cause(event, &policy_context);

        // Generate suggested fix
        let suggested_fix = self.generate_fix(event, &related_policy).await?;

        // Calculate confidence
        let confidence = self.calculate_confidence(event);

        Ok(RootCauseAnalysis {
            event: event.clone(),
            explanation,
            likely_cause,
            suggested_fix,
            confidence,
            related_policy,
            context: policy_context,
        })
    }

    /// Determine likely root cause
    fn determine_likely_cause(&self, event: &DropEvent, context: &[String]) -> String {
        match &event.reason {
            DropReason::PolicyDenied => {
                if context.is_empty() {
                    "No CiliumNetworkPolicy allows this traffic".to_string()
                } else {
                    format!("Policy exists but doesn't match: {}", context.join(", "))
                }
            }
            DropReason::PortNotAllowed => {
                format!("Port {} is not allowed by any policy", event.dst_port)
            }
            DropReason::CTStateMismatch => {
                "Connection tracking table is out of sync or full".to_string()
            }
            DropReason::NoBackend => "Service has no healthy backend pods".to_string(),
            DropReason::ServiceNotFound => {
                "Service does not exist or is not registered".to_string()
            }
            DropReason::FragNeeded => {
                "MTU mismatch - packet too large for network path".to_string()
            }
            DropReason::UnknownDestination => {
                "Destination identity not found in identity map".to_string()
            }
            _ => {
                format!("Drop reason: {}", event.reason)
            }
        }
    }

    /// Generate suggested fix
    async fn generate_fix(
        &self,
        event: &DropEvent,
        _related_policy: &Option<String>,
    ) -> Result<SuggestedFix> {
        // Pre-read IPCache once for label resolution in this fix generation
        let ipcache = self.ebpf_reader.read_ipcache_map().unwrap_or_default();

        match &event.reason {
            DropReason::PolicyDenied | DropReason::PortNotAllowed => {
                // Suggest adding a policy rule
                let protocol = match event.protocol {
                    6 => "TCP",
                    17 => "UDP",
                    _ => "UNKNOWN",
                };

                let namespace = event
                    .namespace
                    .clone()
                    .unwrap_or_else(|| "default".to_string());

                // Resolve labels from IPCache; fall back to identity-based labels
                let from_labels = self.resolve_labels_for_identity(event.identity_src, &ipcache);
                let to_labels = self.resolve_labels_for_identity(event.identity_dst, &ipcache);

                // Build label selectors for the YAML from resolved labels
                let from_label_yaml: String = from_labels
                    .iter()
                    .map(|(k, v)| format!("      {}: {}", yaml_escape(k), yaml_escape(v)))
                    .collect::<Vec<_>>()
                    .join("\n");
                let to_label_yaml: String = to_labels
                    .iter()
                    .map(|(k, v)| format!("        {}: {}", yaml_escape(k), yaml_escape(v)))
                    .collect::<Vec<_>>()
                    .join("\n");

                let yaml = format!(
                    r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: allow-traffic-fix
  namespace: {}
spec:
  endpointSelector:
    matchLabels:
{}
  egress:
  - toEndpoints:
    - matchLabels:
{}
    toPorts:
    - ports:
      - port: "{}"
        protocol: {}
"#,
                    yaml_escape(&namespace),
                    from_label_yaml,
                    to_label_yaml,
                    event.dst_port,
                    protocol
                );

                Ok(SuggestedFix::AddPolicyRule {
                    namespace,
                    from_labels,
                    to_labels,
                    port: event.dst_port,
                    protocol: protocol.to_string(),
                    yaml,
                })
            }

            DropReason::FragNeeded => Ok(SuggestedFix::UpdateMTU {
                interface: "cilium_host".to_string(),
                current_mtu: 1500,
                suggested_mtu: 1450,
                command: "ip link set cilium_host mtu 1450".to_string(),
            }),

            DropReason::CTStateMismatch => Ok(SuggestedFix::CheckConntrack {
                issue: "Connection tracking state mismatch".to_string(),
                commands: vec![
                    "cilium bpf ct list global".to_string(),
                    "cilium bpf ct flush".to_string(),
                ],
            }),

            DropReason::NoBackend => {
                let service = event
                    .namespace
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                Ok(SuggestedFix::AddServiceEndpoint {
                    service: format!("service-on-port-{}", event.dst_port),
                    namespace: service,
                    reason: "Service has no healthy backends".to_string(),
                })
            }

            DropReason::ServiceNotFound => {
                let service = event
                    .namespace
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                Ok(SuggestedFix::FixLoadBalancer {
                    service: format!("service-on-port-{}", event.dst_port),
                    namespace: service,
                    issue: "Service not found in load balancer map".to_string(),
                })
            }

            _ => Ok(SuggestedFix::ManualInvestigation {
                reason: format!("Uncommon drop reason: {}", event.reason),
                steps: vec![
                    "Check cilium monitor output".to_string(),
                    "Review Hubble flows".to_string(),
                    "Inspect eBPF maps directly".to_string(),
                ],
            }),
        }
    }

    /// Calculate confidence in the analysis
    fn calculate_confidence(&self, event: &DropEvent) -> f32 {
        let pattern = DropPattern {
            reason: event.reason.clone(),
            src_identity: event.identity_src,
            dst_identity: event.identity_dst,
            dst_port: event.dst_port,
            protocol: event.protocol,
        };

        let count = self.pattern_counts.get(&pattern).copied().unwrap_or(0);

        // More occurrences = higher confidence
        let frequency_score = (count as f32 / 100.0).min(1.0);

        // Well-known reasons = higher confidence
        let reason_score = match event.reason {
            DropReason::PolicyDenied | DropReason::PortNotAllowed => 1.0,
            DropReason::NoBackend | DropReason::ServiceNotFound => 0.9,
            DropReason::FragNeeded | DropReason::CTStateMismatch => 0.8,
            _ => 0.5,
        };

        // Combined confidence
        (frequency_score * 0.4 + reason_score * 0.6).min(1.0)
    }

    /// Get drop statistics
    pub fn get_stats(&self) -> DropStats {
        let total_drops = self.drop_history.len() as u64;

        let mut by_reason: HashMap<DropReason, u64> = HashMap::new();
        for event in &self.drop_history {
            *by_reason.entry(event.reason.clone()).or_insert(0) += 1;
        }

        let mut by_namespace: HashMap<String, u64> = HashMap::new();
        for event in &self.drop_history {
            if let Some(ns) = &event.namespace {
                *by_namespace.entry(ns.clone()).or_insert(0) += 1;
            }
        }

        let mut top_patterns: Vec<_> = self
            .pattern_counts
            .iter()
            .map(|(p, c)| (p.clone(), *c))
            .collect();
        top_patterns.sort_by_key(|a| std::cmp::Reverse(a.1));
        top_patterns.truncate(10);

        DropStats {
            total_drops,
            by_reason,
            by_namespace,
            top_patterns,
        }
    }

    /// Clear history
    pub fn clear_history(&mut self) {
        self.drop_history.clear();
        self.pattern_counts.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ebpf::MockMapReader;

    #[tokio::test]
    #[ignore] // Requires a live Kubernetes cluster
    async fn test_rootcause_creation() {
        let config = RootCauseConfig::default();
        let reader = MockMapReader;
        let k8s_client = K8sClient::new().await.unwrap();

        let engine = RootCauseEngine::new(config, reader, k8s_client);
        assert_eq!(engine.drop_history.len(), 0);
    }

    #[test]
    fn test_drop_reason_conversion() {
        let ebpf_reason = DropReasonType::PolicyDenied;
        let reason = DropReason::from_ebpf(&ebpf_reason);
        assert_eq!(reason, DropReason::PolicyDenied);
    }

    #[test]
    fn test_drop_reason_to_string() {
        assert_eq!(DropReason::PolicyDenied.to_string(), "Policy Denied");
        assert_eq!(DropReason::FragNeeded.to_string(), "Fragment Needed (MTU)");
    }
}
