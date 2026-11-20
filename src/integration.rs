// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// Integration layer combining eBPF and Kubernetes
///
/// Provides enriched data structures with both kernel-level network data
/// and Kubernetes pod/identity information
use anyhow::Result;
use std::sync::Arc;

use crate::ebpf::{CiliumMapReader, ConntrackEntry, IPCacheEntry, MapReader};
use crate::kubernetes::{K8sClient, K8sIdentityResolver};

/// Enriched connection with Kubernetes context
#[derive(Debug, Clone)]
pub struct EnrichedConnection {
    /// Base connection tracking entry
    pub conn: ConntrackEntry,

    /// Source pod information
    pub src_pod: Option<PodInfo>,

    /// Destination pod information
    pub dst_pod: Option<PodInfo>,
}

/// Pod information
#[derive(Debug, Clone)]
pub struct PodInfo {
    pub namespace: String,
    pub pod_name: String,
    pub labels: Vec<String>,
    pub identity: u32,
}

/// Integrated data provider
pub struct IntegratedDataProvider {
    ebpf_reader: CiliumMapReader,
    identity_resolver: Arc<K8sIdentityResolver>,
}

impl IntegratedDataProvider {
    /// Create a new integrated data provider
    pub async fn new(k8s_client: K8sClient) -> Result<Self> {
        // Initialize eBPF reader
        let ebpf_reader = CiliumMapReader::new()?;

        // Initialize identity resolver
        let identity_resolver =
            Arc::new(K8sIdentityResolver::new(k8s_client.client().clone()).await?);

        // Do initial refresh
        identity_resolver.refresh().await?;

        // Start background refresh (every 30 seconds)
        identity_resolver.clone().start_refresh_task(30);

        Ok(Self {
            ebpf_reader,
            identity_resolver,
        })
    }

    /// Get enriched connections
    pub async fn get_enriched_connections(&self) -> Result<Vec<EnrichedConnection>> {
        // Read connections from eBPF
        let connections = self.ebpf_reader.read_conntrack_map()?;

        // Enrich with Kubernetes data
        let enriched = connections
            .into_iter()
            .map(|conn| {
                let src_pod = self
                    .identity_resolver
                    .resolve_ip(&conn.src_ip)
                    .and_then(|id| self.identity_resolver.resolve_identity(id))
                    .map(|info| PodInfo {
                        namespace: info.namespace,
                        pod_name: info.pod_name,
                        labels: info.labels,
                        identity: info.identity,
                    });

                let dst_pod = self
                    .identity_resolver
                    .resolve_ip(&conn.dst_ip)
                    .and_then(|id| self.identity_resolver.resolve_identity(id))
                    .map(|info| PodInfo {
                        namespace: info.namespace,
                        pod_name: info.pod_name,
                        labels: info.labels,
                        identity: info.identity,
                    });

                EnrichedConnection {
                    conn,
                    src_pod,
                    dst_pod,
                }
            })
            .collect();

        Ok(enriched)
    }

    /// Get enriched IP cache
    pub async fn get_enriched_ipcache(&self) -> Result<Vec<IPCacheEntry>> {
        let mut entries = self.ebpf_reader.read_ipcache_map()?;

        // Enrich each entry
        for entry in &mut entries {
            self.identity_resolver.enrich_ipcache_entry(entry);
        }

        Ok(entries)
    }

    /// Get identity resolver stats
    pub fn identity_stats(&self) -> crate::kubernetes::CacheStats {
        self.identity_resolver.stats()
    }

    /// Check if eBPF data is available
    pub fn is_ebpf_available(&self) -> bool {
        self.ebpf_reader.is_cilium_available()
    }

    /// Get eBPF reader for direct access
    pub fn ebpf_reader(&self) -> &CiliumMapReader {
        &self.ebpf_reader
    }

    /// Get identity resolver for direct access
    pub fn identity_resolver(&self) -> &Arc<K8sIdentityResolver> {
        &self.identity_resolver
    }
}

/// Format enriched connection for display
pub fn format_enriched_connection(conn: &EnrichedConnection) -> String {
    let src = match &conn.src_pod {
        Some(pod) => format!("{}/{}", pod.namespace, pod.pod_name),
        None => conn.conn.src_ip.clone(),
    };

    let dst = match &conn.dst_pod {
        Some(pod) => format!("{}/{}", pod.namespace, pod.pod_name),
        None => conn.conn.dst_ip.clone(),
    };

    format!(
        "{}:{} -> {}:{} ({} packets, {} bytes)",
        src, conn.conn.src_port, dst, conn.conn.dst_port, conn.conn.packets, conn.conn.bytes
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_enriched_connection() {
        use crate::ebpf::ConntrackState;

        let conn = EnrichedConnection {
            conn: ConntrackEntry {
                src_ip: "10.0.1.5".to_string(),
                dst_ip: "10.0.2.10".to_string(),
                src_port: 45678,
                dst_port: 80,
                protocol: 6,
                state: ConntrackState::Established,
                packets: 100,
                bytes: 50000,
                last_seen: 0,
                src_namespace: None,
                src_pod: None,
                src_labels: None,
                dst_namespace: None,
                dst_pod: None,
                dst_labels: None,
            },
            src_pod: Some(PodInfo {
                namespace: "prod".to_string(),
                pod_name: "web-abc123".to_string(),
                labels: vec!["app=web".to_string()],
                identity: 100,
            }),
            dst_pod: Some(PodInfo {
                namespace: "prod".to_string(),
                pod_name: "api-def456".to_string(),
                labels: vec!["app=api".to_string()],
                identity: 200,
            }),
        };

        let formatted = format_enriched_connection(&conn);
        assert!(formatted.contains("prod/web-abc123"));
        assert!(formatted.contains("prod/api-def456"));
        assert!(formatted.contains("100 packets"));
    }
}
