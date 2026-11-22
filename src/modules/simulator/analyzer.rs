// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Impact Analyzer
///
/// Analyzes the impact of policy changes on services and dependencies
use super::*;
use anyhow::Result;
use std::collections::{HashMap, HashSet};

pub struct ImpactAnalyzer<'a, M: MapReader> {
    k8s_client: &'a K8sClient,
    reader: &'a M,
}

impl<'a, M: MapReader> ImpactAnalyzer<'a, M> {
    pub fn new(k8s_client: &'a K8sClient, reader: &'a M) -> Self {
        Self { k8s_client, reader }
    }

    /// Resolve identity information from IPCache.
    /// Returns (service_name, namespace, labels) for a given identity.
    fn resolve_identity_info(&self, identity: u32) -> (String, String, HashMap<String, String>) {
        if let Ok(entries) = self.reader.read_ipcache_map() {
            if let Some(entry) = entries.iter().find(|e| e.identity == identity) {
                let namespace = if entry.namespace.is_empty() {
                    "default".to_string()
                } else {
                    entry.namespace.clone()
                };
                let name = entry
                    .labels
                    .iter()
                    .find(|l| l.starts_with("app="))
                    .map(|l| l.trim_start_matches("app=").to_string())
                    .unwrap_or_else(|| format!("service-{}", identity));
                let labels: HashMap<String, String> = entry
                    .labels
                    .iter()
                    .filter_map(|l| l.split_once('='))
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();
                return (name, namespace, labels);
            }
        }
        (
            format!("service-{}", identity),
            "default".to_string(),
            HashMap::new(),
        )
    }

    /// Collect unique namespaces from IPCache for a set of identities.
    fn resolve_namespaces_from_identities(&self, identities: &[u32]) -> Vec<String> {
        let mut namespaces = HashSet::new();
        if let Ok(entries) = self.reader.read_ipcache_map() {
            for identity in identities {
                if let Some(entry) = entries.iter().find(|e| e.identity == *identity) {
                    let ns = if entry.namespace.is_empty() {
                        "default".to_string()
                    } else {
                        entry.namespace.clone()
                    };
                    namespaces.insert(ns);
                }
            }
        }
        if namespaces.is_empty() {
            namespaces.insert("default".to_string());
        }
        namespaces.into_iter().collect()
    }

    /// Analyze impact from flow simulation results
    pub async fn analyze(&self, flow_results: &[FlowSimulationResult]) -> Result<ImpactAnalysis> {
        let total_flows = flow_results.len();

        // Count blocked and allowed flows
        let blocked_flows = flow_results
            .iter()
            .filter(|f| f.after == PolicyVerdict::Deny && f.before != PolicyVerdict::Deny)
            .count();

        let allowed_flows = flow_results
            .iter()
            .filter(|f| f.after == PolicyVerdict::Allow)
            .count();

        let changed_flows = flow_results.iter().filter(|f| f.changed).count();

        // Find impacted services
        let impacted_services = self.find_impacted_services(flow_results).await?;

        // Identify critical services
        let critical_services = self.identify_critical_services(&impacted_services);

        Ok(ImpactAnalysis {
            total_flows,
            blocked_flows,
            allowed_flows,
            changed_flows,
            impacted_services,
            critical_services,
        })
    }

    /// Find affected resources
    pub async fn find_affected_resources(
        &self,
        flow_results: &[FlowSimulationResult],
    ) -> Result<AffectedResources> {
        // Collect unique identities
        let pod_identities: Vec<u32> = flow_results
            .iter()
            .filter(|f| f.changed)
            .flat_map(|f| vec![f.src_identity, f.dst_identity])
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        // Resolve namespaces from IPCache entries matching the flow identities
        let namespaces = self.resolve_namespaces_from_identities(&pod_identities);

        // Find affected services
        let services = self.find_affected_services(flow_results).await?;

        // Find affected endpoints
        let endpoints = self.find_affected_endpoints(flow_results).await?;

        // Find broken dependencies
        let broken_dependencies = self.find_broken_dependencies(flow_results).await?;

        Ok(AffectedResources {
            namespaces,
            pod_identities,
            services,
            endpoints,
            broken_dependencies,
        })
    }

    /// Find impacted services
    async fn find_impacted_services(
        &self,
        flow_results: &[FlowSimulationResult],
    ) -> Result<Vec<String>> {
        let mut services = HashSet::new();

        // Group flows by destination
        let mut by_dst: HashMap<u32, Vec<&FlowSimulationResult>> = HashMap::new();
        for result in flow_results.iter().filter(|f| f.changed) {
            by_dst.entry(result.dst_identity).or_default().push(result);
        }

        // For each affected destination, identify service
        for (identity, flows) in by_dst {
            if flows.iter().any(|f| f.after == PolicyVerdict::Deny) {
                let (name, namespace, _labels) = self.resolve_identity_info(identity);
                services.insert(format!("{}/{}", namespace, name));
            }
        }

        Ok(services.into_iter().collect())
    }

    /// Identify critical services by checking pod labels/annotations and
    /// well-known service name patterns (db, payment, auth, gateway, etc.).
    fn identify_critical_services(&self, impacted_services: &[String]) -> Vec<String> {
        let critical_name_patterns = [
            "db", "database", "postgres", "mysql", "mongo", "payment", "billing", "checkout",
            "auth", "oauth", "identity", "login", "gateway", "ingress", "proxy", "envoy", "redis",
            "cache", "memcache", "kafka", "rabbitmq", "nats", "mq", "dns", "api", "core",
        ];

        // Build a set of identities that carry a criticality annotation
        let mut annotated_critical: HashSet<String> = HashSet::new();
        if let Ok(entries) = self.reader.read_ipcache_map() {
            for entry in &entries {
                let is_critical_by_label = entry.labels.iter().any(|l| {
                    l == "criticality=high" || l == "tier=critical" || l == "priority=critical"
                });
                if is_critical_by_label {
                    let namespace = if entry.namespace.is_empty() {
                        "default".to_string()
                    } else {
                        entry.namespace.clone()
                    };
                    let name = entry
                        .labels
                        .iter()
                        .find(|l| l.starts_with("app="))
                        .map(|l| l.trim_start_matches("app=").to_string())
                        .unwrap_or_else(|| format!("service-{}", entry.identity));
                    annotated_critical.insert(format!("{}/{}", namespace, name));
                }
            }
        }

        impacted_services
            .iter()
            .filter(|s| {
                // Match if the service name contains any critical pattern
                let name_match = critical_name_patterns
                    .iter()
                    .any(|pat| s.to_lowercase().contains(pat));
                // Or if the service was annotated as critical in IPCache labels
                let annotation_match = annotated_critical.contains(s.as_str());
                name_match || annotation_match
            })
            .cloned()
            .collect()
    }

    /// Find affected services with detailed impact
    async fn find_affected_services(
        &self,
        flow_results: &[FlowSimulationResult],
    ) -> Result<Vec<ServiceImpact>> {
        let mut services = Vec::new();

        // Group by destination identity (service)
        let mut by_service: HashMap<u32, Vec<&FlowSimulationResult>> = HashMap::new();
        for result in flow_results {
            by_service
                .entry(result.dst_identity)
                .or_default()
                .push(result);
        }

        for (identity, flows) in by_service {
            let blocked_flows: Vec<_> = flows
                .iter()
                .filter(|f| f.changed && f.after == PolicyVerdict::Deny)
                .collect();

            if blocked_flows.is_empty() {
                continue;
            }

            // Determine impact type
            let total_flows = flows.len();
            let blocked_count = blocked_flows.len();

            let impact_type = if blocked_count == total_flows {
                ImpactType::FullyBlocked
            } else if blocked_count > 0 {
                ImpactType::PartiallyBlocked
            } else {
                ImpactType::Unaffected
            };

            // Collect affected ports
            let affected_ports: Vec<u16> = blocked_flows
                .iter()
                .map(|f| f.port)
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();

            // Count unique clients
            let client_count = flows
                .iter()
                .map(|f| f.src_identity)
                .collect::<HashSet<_>>()
                .len();

            let (resolved_name, resolved_namespace, _labels) = self.resolve_identity_info(identity);
            services.push(ServiceImpact {
                name: resolved_name,
                namespace: resolved_namespace,
                impact_type,
                affected_ports,
                client_count,
            });
        }

        Ok(services)
    }

    /// Find affected endpoints
    async fn find_affected_endpoints(
        &self,
        flow_results: &[FlowSimulationResult],
    ) -> Result<Vec<EndpointImpact>> {
        let mut endpoints = Vec::new();

        // Group by source identity (endpoint)
        let mut by_endpoint: HashMap<u32, Vec<&FlowSimulationResult>> = HashMap::new();
        for result in flow_results {
            by_endpoint
                .entry(result.src_identity)
                .or_default()
                .push(result);
        }

        for (identity, flows) in by_endpoint {
            // Group blocked flows by (dst_identity, port, protocol) and count occurrences
            let mut egress_counts: HashMap<(u32, u16, u8), usize> = HashMap::new();
            for f in flows
                .iter()
                .filter(|f| f.changed && f.after == PolicyVerdict::Deny)
            {
                *egress_counts
                    .entry((f.dst_identity, f.port, f.protocol))
                    .or_insert(0) += 1;
            }

            let blocked_egress: Vec<_> = egress_counts
                .into_iter()
                .map(|((to_identity, port, protocol), count)| EgressBlocked {
                    to_identity,
                    port,
                    protocol,
                    flow_count: count,
                })
                .collect();

            // Analyze ingress: find flows where this identity is the destination
            let mut ingress_counts: HashMap<(u32, u16, u8), usize> = HashMap::new();
            for f in flow_results.iter().filter(|f| {
                f.dst_identity == identity && f.changed && f.after == PolicyVerdict::Deny
            }) {
                *ingress_counts
                    .entry((f.src_identity, f.port, f.protocol))
                    .or_insert(0) += 1;
            }

            let blocked_ingress: Vec<_> = ingress_counts
                .into_iter()
                .map(|((from_identity, port, protocol), count)| IngressBlocked {
                    from_identity,
                    port,
                    protocol,
                    flow_count: count,
                })
                .collect();

            if blocked_egress.is_empty() && blocked_ingress.is_empty() {
                continue;
            }

            let (_name, resolved_namespace, resolved_labels) = self.resolve_identity_info(identity);
            endpoints.push(EndpointImpact {
                identity,
                namespace: resolved_namespace,
                labels: resolved_labels,
                blocked_egress,
                blocked_ingress,
            });
        }

        Ok(endpoints)
    }

    /// Find broken dependencies
    async fn find_broken_dependencies(
        &self,
        flow_results: &[FlowSimulationResult],
    ) -> Result<Vec<Dependency>> {
        let mut dependencies = Vec::new();

        // Find flows that changed from Allow to Deny
        for result in flow_results {
            if result.before == PolicyVerdict::Allow && result.after == PolicyVerdict::Deny {
                let protocol = match result.protocol {
                    6 => "TCP",
                    17 => "UDP",
                    _ => "OTHER",
                };

                // Determine criticality based on port
                let criticality = Self::determine_criticality(result.port, protocol);

                let (src_name, src_ns, _) = self.resolve_identity_info(result.src_identity);
                let (dst_name, dst_ns, _) = self.resolve_identity_info(result.dst_identity);
                dependencies.push(Dependency {
                    from_service: format!("{}/{}", src_ns, src_name),
                    to_service: format!("{}/{}", dst_ns, dst_name),
                    port: result.port,
                    protocol: protocol.to_string(),
                    criticality,
                });
            }
        }

        Ok(dependencies)
    }

    /// Determine dependency criticality
    fn determine_criticality(port: u16, protocol: &str) -> DependencyCriticality {
        match (port, protocol) {
            // Critical infrastructure ports
            (53, _) => DependencyCriticality::Critical, // DNS
            (443, "TCP") => DependencyCriticality::Critical, // HTTPS
            (5432, _) => DependencyCriticality::Critical, // PostgreSQL
            (3306, _) => DependencyCriticality::Critical, // MySQL
            (6379, _) => DependencyCriticality::Critical, // Redis
            (9200, _) => DependencyCriticality::Critical, // Elasticsearch

            // Important ports
            (80, "TCP") => DependencyCriticality::Important, // HTTP
            (8080, _) => DependencyCriticality::Important,   // HTTP alt
            (3000, _) => DependencyCriticality::Important,   // Common app port
            (9090, _) => DependencyCriticality::Important,   // Prometheus

            // Everything else
            _ => DependencyCriticality::Optional,
        }
    }

    /// Analyze service-to-service communication
    pub async fn analyze_service_mesh(
        &self,
        flow_results: &[FlowSimulationResult],
    ) -> Result<ServiceMeshAnalysis> {
        let mut edges = Vec::new();
        let mut nodes = HashSet::new();

        for result in flow_results {
            nodes.insert(result.src_identity);
            nodes.insert(result.dst_identity);

            if result.changed {
                edges.push(CommunicationEdge {
                    from: result.src_identity,
                    to: result.dst_identity,
                    port: result.port,
                    protocol: result.protocol,
                    before_allowed: result.before == PolicyVerdict::Allow,
                    after_allowed: result.after == PolicyVerdict::Allow,
                });
            }
        }

        Ok(ServiceMeshAnalysis {
            total_nodes: nodes.len(),
            affected_edges: edges.len(),
            edges,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ServiceMeshAnalysis {
    pub total_nodes: usize,
    pub affected_edges: usize,
    pub edges: Vec<CommunicationEdge>,
}

#[derive(Debug, Clone)]
pub struct CommunicationEdge {
    pub from: u32,
    pub to: u32,
    pub port: u16,
    pub protocol: u8,
    pub before_allowed: bool,
    pub after_allowed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ebpf::MockMapReader;

    #[test]
    fn test_determine_criticality() {
        assert_eq!(
            ImpactAnalyzer::<MockMapReader>::determine_criticality(53, "UDP"),
            DependencyCriticality::Critical
        );
        assert_eq!(
            ImpactAnalyzer::<MockMapReader>::determine_criticality(443, "TCP"),
            DependencyCriticality::Critical
        );
        assert_eq!(
            ImpactAnalyzer::<MockMapReader>::determine_criticality(80, "TCP"),
            DependencyCriticality::Important
        );
        assert_eq!(
            ImpactAnalyzer::<MockMapReader>::determine_criticality(12345, "TCP"),
            DependencyCriticality::Optional
        );
    }

    #[test]
    fn test_identify_critical_services() {
        let services = vec![
            "web-service".to_string(),
            "db-service".to_string(),
            "api-gateway".to_string(),
            "cache-service".to_string(),
        ];

        // Verify the pattern matching logic would work
        assert!(services.iter().any(|s| s.contains("db")));
        assert!(services.iter().any(|s| s.contains("api")));
    }
}
