// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Traffic Pattern Analyzer
///
/// Analyzes learned traffic patterns to provide insights
use super::*;

pub struct TrafficAnalyzer;

impl TrafficAnalyzer {
    /// Analyze communication graph
    pub fn build_graph(
        observations: &HashMap<TrafficPattern, TrafficObservation>,
    ) -> CommunicationGraph {
        let mut graph = CommunicationGraph {
            nodes: HashMap::new(),
            edges: Vec::new(),
        };

        for obs in observations.values() {
            // Add source node
            let src_id = format!(
                "{}/{}",
                obs.pattern.src_namespace,
                obs.pattern
                    .src_labels
                    .get("app")
                    .unwrap_or(&"unknown".to_string())
            );
            graph.nodes.entry(src_id.clone()).or_insert_with(|| Node {
                id: src_id.clone(),
                namespace: obs.pattern.src_namespace.clone(),
                labels: obs.pattern.src_labels.clone(),
            });

            // Add destination node
            let dst_id = format!(
                "{}/{}",
                obs.pattern.dst_namespace,
                obs.pattern
                    .dst_labels
                    .get("app")
                    .unwrap_or(&"unknown".to_string())
            );
            graph.nodes.entry(dst_id.clone()).or_insert_with(|| Node {
                id: dst_id.clone(),
                namespace: obs.pattern.dst_namespace.clone(),
                labels: obs.pattern.dst_labels.clone(),
            });

            // Add edge
            graph.edges.push(Edge {
                from: src_id,
                to: dst_id,
                port: obs.pattern.port,
                protocol: obs.pattern.protocol,
                count: obs.count,
                bytes: obs.bytes_transferred,
            });
        }

        graph
    }

    /// Find isolated services (no outbound traffic)
    pub fn find_isolated_services(
        observations: &HashMap<TrafficPattern, TrafficObservation>,
    ) -> Vec<String> {
        let graph = Self::build_graph(observations);
        let mut isolated = Vec::new();

        for node in graph.nodes.values() {
            let has_outbound = graph.edges.iter().any(|e| e.from == node.id);
            if !has_outbound {
                isolated.push(node.id.clone());
            }
        }

        isolated
    }

    /// Find most connected services
    pub fn find_hubs(
        observations: &HashMap<TrafficPattern, TrafficObservation>,
        limit: usize,
    ) -> Vec<(String, usize)> {
        let graph = Self::build_graph(observations);
        let mut connections: HashMap<String, usize> = HashMap::new();

        for edge in &graph.edges {
            *connections.entry(edge.from.clone()).or_insert(0) += 1;
            *connections.entry(edge.to.clone()).or_insert(0) += 1;
        }

        let mut sorted: Vec<_> = connections.into_iter().collect();
        sorted.sort_by_key(|a| std::cmp::Reverse(a.1));
        sorted.into_iter().take(limit).collect()
    }

    /// Detect potential security issues
    pub fn detect_security_issues(
        observations: &HashMap<TrafficPattern, TrafficObservation>,
    ) -> Vec<SecurityIssue> {
        let mut issues = Vec::new();

        for obs in observations.values() {
            // Check for cross-namespace traffic
            if obs.pattern.src_namespace != obs.pattern.dst_namespace {
                issues.push(SecurityIssue::CrossNamespace {
                    from: obs.pattern.src_namespace.clone(),
                    to: obs.pattern.dst_namespace.clone(),
                    port: obs.pattern.port,
                });
            }

            // Check for privileged ports
            if obs.pattern.port < 1024 {
                issues.push(SecurityIssue::PrivilegedPort {
                    namespace: obs.pattern.src_namespace.clone(),
                    port: obs.pattern.port,
                });
            }

            // Check for common attack ports
            if Self::is_suspicious_port(obs.pattern.port) {
                issues.push(SecurityIssue::SuspiciousPort {
                    namespace: obs.pattern.src_namespace.clone(),
                    port: obs.pattern.port,
                });
            }
        }

        issues
    }

    fn is_suspicious_port(port: u16) -> bool {
        // Common attack/scan ports
        matches!(port, 22 | 23 | 3389 | 4444 | 6667 | 31337)
    }

    /// Calculate policy complexity score
    pub fn calculate_complexity(
        observations: &HashMap<TrafficPattern, TrafficObservation>,
    ) -> ComplexityScore {
        let unique_sources = observations
            .values()
            .map(|o| &o.pattern.src_namespace)
            .collect::<HashSet<_>>()
            .len();

        let unique_destinations = observations
            .values()
            .map(|o| &o.pattern.dst_namespace)
            .collect::<HashSet<_>>()
            .len();

        let unique_ports = observations
            .values()
            .map(|o| o.pattern.port)
            .collect::<HashSet<_>>()
            .len();

        let score = if unique_sources * unique_destinations * unique_ports > 1000 {
            ComplexityLevel::High
        } else if unique_sources * unique_destinations * unique_ports > 100 {
            ComplexityLevel::Medium
        } else {
            ComplexityLevel::Low
        };

        ComplexityScore {
            level: score,
            unique_sources,
            unique_destinations,
            unique_ports,
            total_patterns: observations.len(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommunicationGraph {
    pub nodes: HashMap<String, Node>,
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub namespace: String,
    pub labels: LabelSet,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub port: u16,
    pub protocol: Protocol,
    pub count: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub enum SecurityIssue {
    CrossNamespace { from: String, to: String, port: u16 },
    PrivilegedPort { namespace: String, port: u16 },
    SuspiciousPort { namespace: String, port: u16 },
}

#[derive(Debug, Clone)]
pub enum ComplexityLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone)]
pub struct ComplexityScore {
    pub level: ComplexityLevel,
    pub unique_sources: usize,
    pub unique_destinations: usize,
    pub unique_ports: usize,
    pub total_patterns: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_suspicious_port_detection() {
        assert!(TrafficAnalyzer::is_suspicious_port(22)); // SSH
        assert!(TrafficAnalyzer::is_suspicious_port(3389)); // RDP
        assert!(!TrafficAnalyzer::is_suspicious_port(80)); // HTTP
        assert!(!TrafficAnalyzer::is_suspicious_port(443)); // HTTPS
    }
}
