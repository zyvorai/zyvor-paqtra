// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// Enriched Map Reader
///
/// Combines eBPF map reading with Kubernetes identity resolution
/// to provide contextual information for intelligence modules.
use anyhow::Result;
use std::sync::Arc;

use super::bpf_reader::CiliumMapReader;
use super::{
    ConntrackEntry, DropReason, IPCacheEntry, LoadBalancerEntry, MapReader, PolicyDecision,
};
use crate::kubernetes::K8sIdentityResolver;

/// Enriched Map Reader with Kubernetes context
#[allow(dead_code)]
#[derive(Clone)]
pub struct EnrichedMapReader {
    cilium_reader: Arc<CiliumMapReader>,
    identity_resolver: Arc<K8sIdentityResolver>,
}

impl EnrichedMapReader {
    /// Create a new enriched map reader
    pub fn new(
        cilium_reader: CiliumMapReader,
        identity_resolver: Arc<K8sIdentityResolver>,
    ) -> Self {
        Self {
            cilium_reader: Arc::new(cilium_reader),
            identity_resolver,
        }
    }

    /// Create from Arc-wrapped reader (avoids double-wrapping)
    pub fn from_arc(
        cilium_reader: Arc<CiliumMapReader>,
        identity_resolver: Arc<K8sIdentityResolver>,
    ) -> Self {
        Self {
            cilium_reader,
            identity_resolver,
        }
    }

    /// Resolve IP address to namespace and pod name
    pub fn resolve_ip_to_pod(&self, ip: &str) -> Option<(String, String)> {
        let identity = self.identity_resolver.resolve_ip(ip)?;
        let info = self.identity_resolver.resolve_identity(identity)?;
        Some((info.namespace, info.pod_name))
    }

    /// Resolve IP address to namespace only
    pub fn resolve_ip_to_namespace(&self, ip: &str) -> Option<String> {
        let identity = self.identity_resolver.resolve_ip(ip)?;
        let info = self.identity_resolver.resolve_identity(identity)?;
        Some(info.namespace)
    }

    /// Resolve IP address to labels
    pub fn resolve_ip_to_labels(&self, ip: &str) -> Option<Vec<String>> {
        let identity = self.identity_resolver.resolve_ip(ip)?;
        let info = self.identity_resolver.resolve_identity(identity)?;
        Some(info.labels)
    }

    /// Get enriched connection with pod context
    pub fn enrich_connection(&self, conn: &ConntrackEntry) -> EnrichedConnectionInfo {
        let src_pod = self.resolve_ip_to_pod(&conn.src_ip);
        let dst_pod = self.resolve_ip_to_pod(&conn.dst_ip);

        EnrichedConnectionInfo {
            src_namespace: src_pod.as_ref().map(|(ns, _)| ns.clone()),
            src_pod: src_pod.as_ref().map(|(_, pod)| pod.clone()),
            dst_namespace: dst_pod.as_ref().map(|(ns, _)| ns.clone()),
            dst_pod: dst_pod.as_ref().map(|(_, pod)| pod.clone()),
            src_labels: self.resolve_ip_to_labels(&conn.src_ip).unwrap_or_default(),
            dst_labels: self.resolve_ip_to_labels(&conn.dst_ip).unwrap_or_default(),
        }
    }

    /// Get enriched drop reason with pod context
    pub fn enrich_drop(&self, drop: &DropReason) -> EnrichedDropInfo {
        let src_pod = self.resolve_ip_to_pod(&drop.src_ip);
        let dst_pod = self.resolve_ip_to_pod(&drop.dst_ip);

        EnrichedDropInfo {
            src_namespace: src_pod.as_ref().map(|(ns, _)| ns.clone()),
            src_pod: src_pod.as_ref().map(|(_, pod)| pod.clone()),
            dst_namespace: dst_pod.as_ref().map(|(ns, _)| ns.clone()),
            dst_pod: dst_pod.as_ref().map(|(_, pod)| pod.clone()),
        }
    }

    /// Get enriched connections (batch operation)
    pub fn read_enriched_connections(
        &self,
    ) -> Result<Vec<(ConntrackEntry, EnrichedConnectionInfo)>> {
        let connections = self.read_conntrack_map()?;
        let enriched = connections
            .into_iter()
            .map(|conn| {
                let info = self.enrich_connection(&conn);
                (conn, info)
            })
            .collect();
        Ok(enriched)
    }

    /// Get enriched drops (batch operation)
    pub fn read_enriched_drops(&self) -> Result<Vec<(DropReason, EnrichedDropInfo)>> {
        let drops = self.read_drop_map()?;
        let enriched = drops
            .into_iter()
            .map(|drop| {
                let info = self.enrich_drop(&drop);
                (drop, info)
            })
            .collect();
        Ok(enriched)
    }

    /// Get identity resolver stats
    pub fn identity_stats(&self) -> crate::kubernetes::CacheStats {
        self.identity_resolver.stats()
    }
}

/// Implement MapReader trait for transparent usage
impl MapReader for EnrichedMapReader {
    fn read_policy_map(&self) -> Result<Vec<PolicyDecision>> {
        self.cilium_reader.read_policy_map()
    }

    fn read_conntrack_map(&self) -> Result<Vec<ConntrackEntry>> {
        self.cilium_reader.read_conntrack_map()
    }

    fn read_lb_map(&self) -> Result<Vec<LoadBalancerEntry>> {
        self.cilium_reader.read_lb_map()
    }

    fn read_ipcache_map(&self) -> Result<Vec<IPCacheEntry>> {
        self.cilium_reader.read_ipcache_map()
    }

    fn read_drop_map(&self) -> Result<Vec<DropReason>> {
        self.cilium_reader.read_drop_map()
    }
}

/// Enriched connection information
#[derive(Debug, Clone)]
pub struct EnrichedConnectionInfo {
    pub src_namespace: Option<String>,
    pub src_pod: Option<String>,
    pub dst_namespace: Option<String>,
    pub dst_pod: Option<String>,
    pub src_labels: Vec<String>,
    pub dst_labels: Vec<String>,
}

/// Enriched drop information
#[derive(Debug, Clone)]
pub struct EnrichedDropInfo {
    pub src_namespace: Option<String>,
    pub src_pod: Option<String>,
    pub dst_namespace: Option<String>,
    pub dst_pod: Option<String>,
}

impl EnrichedConnectionInfo {
    /// Get source identity as namespace/pod
    pub fn src_identity(&self) -> String {
        match (&self.src_namespace, &self.src_pod) {
            (Some(ns), Some(pod)) => format!("{}/{}", ns, pod),
            (Some(ns), None) => ns.clone(),
            _ => "unknown".to_string(),
        }
    }

    /// Get destination identity as namespace/pod
    pub fn dst_identity(&self) -> String {
        match (&self.dst_namespace, &self.dst_pod) {
            (Some(ns), Some(pod)) => format!("{}/{}", ns, pod),
            (Some(ns), None) => ns.clone(),
            _ => "unknown".to_string(),
        }
    }

    /// Get labels by key
    pub fn get_src_label(&self, key: &str) -> Option<String> {
        self.src_labels
            .iter()
            .find(|label| label.starts_with(&format!("{}=", key)))
            .and_then(|label| label.split_once('=').map(|x| x.1))
            .map(|s| s.to_string())
    }

    /// Get labels by key
    pub fn get_dst_label(&self, key: &str) -> Option<String> {
        self.dst_labels
            .iter()
            .find(|label| label.starts_with(&format!("{}=", key)))
            .and_then(|label| label.split_once('=').map(|x| x.1))
            .map(|s| s.to_string())
    }
}

impl EnrichedDropInfo {
    /// Get source identity as namespace/pod
    pub fn src_identity(&self) -> String {
        match (&self.src_namespace, &self.src_pod) {
            (Some(ns), Some(pod)) => format!("{}/{}", ns, pod),
            (Some(ns), None) => ns.clone(),
            _ => "unknown".to_string(),
        }
    }

    /// Get destination identity as namespace/pod
    pub fn dst_identity(&self) -> String {
        match (&self.dst_namespace, &self.dst_pod) {
            (Some(ns), Some(pod)) => format!("{}/{}", ns, pod),
            (Some(ns), None) => ns.clone(),
            _ => "unknown".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enriched_connection_info() {
        let info = EnrichedConnectionInfo {
            src_namespace: Some("prod".to_string()),
            src_pod: Some("web-123".to_string()),
            dst_namespace: Some("prod".to_string()),
            dst_pod: Some("api-456".to_string()),
            src_labels: vec!["app=web".to_string(), "tier=frontend".to_string()],
            dst_labels: vec!["app=api".to_string(), "tier=backend".to_string()],
        };

        assert_eq!(info.src_identity(), "prod/web-123");
        assert_eq!(info.dst_identity(), "prod/api-456");
        assert_eq!(info.get_src_label("app"), Some("web".to_string()));
        assert_eq!(info.get_dst_label("app"), Some("api".to_string()));
    }

    #[test]
    fn test_enriched_drop_info() {
        let info = EnrichedDropInfo {
            src_namespace: Some("staging".to_string()),
            src_pod: Some("test-789".to_string()),
            dst_namespace: None,
            dst_pod: None,
        };

        assert_eq!(info.src_identity(), "staging/test-789");
        assert_eq!(info.dst_identity(), "unknown");
    }
}
