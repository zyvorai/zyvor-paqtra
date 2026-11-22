// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Packet Explainer - Interactive Packet Analysis
///
/// Provides detailed explanations of network packets including:
/// - Protocol breakdown
/// - Policy evaluation context
/// - Security implications
/// - Performance insights
/// - Troubleshooting suggestions
use anyhow::Result;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PacketExplanation {
    pub packet_id: String,
    pub timestamp: String,
    pub verdict: String,

    // Basic Info
    pub source: String,
    pub destination: String,
    pub protocol: String,
    pub ports: String,

    // Analysis
    pub what_happened: String,
    pub why_happened: String,
    pub policy_context: String,
    pub security_analysis: String,
    pub performance_notes: String,
    pub troubleshooting_tips: Vec<String>,

    // Metadata
    pub confidence: f32,
    pub severity: String,
}

const MAX_CACHE_SIZE: usize = 1000;

pub struct PacketExplainer {
    explanations_cache: HashMap<String, PacketExplanation>,
}

impl Default for PacketExplainer {
    fn default() -> Self {
        Self::new()
    }
}

impl PacketExplainer {
    pub fn new() -> Self {
        Self {
            explanations_cache: HashMap::new(),
        }
    }

    /// Explain a packet flow
    #[allow(clippy::too_many_arguments)]
    pub fn explain_packet(
        &mut self,
        source_ns: &str,
        source_pod: &str,
        dest_ns: &str,
        dest_pod: &str,
        port: u16,
        protocol: &str,
        verdict: &str,
    ) -> Result<PacketExplanation> {
        let packet_id = format!(
            "{}/{}→{}/{}:{}",
            source_ns, source_pod, dest_ns, dest_pod, port
        );

        // Check cache
        if let Some(cached) = self.explanations_cache.get(&packet_id) {
            return Ok(cached.clone());
        }

        // Generate explanation
        let explanation = self.generate_explanation(
            source_ns, source_pod, dest_ns, dest_pod, port, protocol, verdict,
        );

        // Evict all entries when cache is full (simple but deterministic)
        if self.explanations_cache.len() >= MAX_CACHE_SIZE {
            self.explanations_cache.clear();
        }

        // Cache it
        self.explanations_cache
            .insert(packet_id.clone(), explanation.clone());

        Ok(explanation)
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_explanation(
        &self,
        source_ns: &str,
        source_pod: &str,
        dest_ns: &str,
        dest_pod: &str,
        port: u16,
        protocol: &str,
        verdict: &str,
    ) -> PacketExplanation {
        let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();

        // Analyze what happened
        let what_happened = match verdict {
            "FORWARDED" => {
                format!(
                    "Traffic was ALLOWED from {}/{} to {}/{} on port {}",
                    source_ns, source_pod, dest_ns, dest_pod, port
                )
            }
            "DROPPED" => {
                format!(
                    "Traffic was BLOCKED from {}/{} to {}/{} on port {}",
                    source_ns, source_pod, dest_ns, dest_pod, port
                )
            }
            _ => format!("Traffic verdict: {}", verdict),
        };

        // Analyze why it happened
        let why_happened = self.analyze_reason(port, protocol, verdict, source_ns, dest_ns);

        // Policy context
        let policy_context = self.get_policy_context(source_ns, dest_ns, port, verdict);

        // Security analysis
        let security_analysis = self.analyze_security(port, protocol, verdict, dest_ns);

        // Performance notes
        let performance_notes = self.analyze_performance(protocol, port);

        // Troubleshooting tips
        let troubleshooting_tips = self.get_troubleshooting_tips(verdict, port, protocol);

        // Determine severity
        let severity = if verdict == "DROPPED" {
            if port == 80 || port == 443 || port == 8080 {
                "Critical".to_string()
            } else {
                "Medium".to_string()
            }
        } else {
            "Low".to_string()
        };

        PacketExplanation {
            packet_id: format!(
                "{}/{}→{}/{}:{}",
                source_ns, source_pod, dest_ns, dest_pod, port
            ),
            timestamp,
            verdict: verdict.to_string(),
            source: format!("{}/{}", source_ns, source_pod),
            destination: format!("{}/{}", dest_ns, dest_pod),
            protocol: protocol.to_string(),
            ports: format!(":{}", port),
            what_happened,
            why_happened,
            policy_context,
            security_analysis,
            performance_notes,
            troubleshooting_tips,
            confidence: 0.85,
            severity,
        }
    }

    fn analyze_reason(
        &self,
        port: u16,
        _protocol: &str,
        verdict: &str,
        _source_ns: &str,
        _dest_ns: &str,
    ) -> String {
        if verdict == "DROPPED" {
            match port {
                53 => "DNS traffic was blocked. This usually indicates a missing DNS egress policy or restrictive network policy preventing DNS resolution.".to_string(),
                80 | 443 | 8080 => format!("HTTP/HTTPS traffic on port {} was blocked. This typically means a CiliumNetworkPolicy denies this communication path.", port),
                3306 => "MySQL database traffic was blocked. Database access requires explicit network policy allowing this connection.".to_string(),
                5432 => "PostgreSQL database traffic was blocked. Database connections must be explicitly allowed in network policies.".to_string(),
                6379 => "Redis traffic was blocked. Cache access requires network policy permission.".to_string(),
                _ => format!("Traffic on port {} was blocked by network policy. This connection is not explicitly allowed.", port),
            }
        } else {
            match port {
                53 => "DNS query was allowed. This is normal for service discovery and name resolution.".to_string(),
                80 | 443 => format!("HTTP/HTTPS traffic on port {} was allowed by network policy.", port),
                _ => format!("Traffic on port {} was explicitly allowed by network policy or default-allow rules.", port),
            }
        }
    }

    fn get_policy_context(
        &self,
        source_ns: &str,
        dest_ns: &str,
        _port: u16,
        verdict: &str,
    ) -> String {
        if source_ns == dest_ns {
            format!(
                "Traffic within the same namespace ({}). {}",
                source_ns,
                if verdict == "DROPPED" {
                    "A restrictive CiliumNetworkPolicy is blocking intra-namespace communication."
                } else {
                    "Intra-namespace traffic is typically allowed by default."
                }
            )
        } else {
            format!(
                "Cross-namespace traffic from {} to {}. {}",
                source_ns,
                dest_ns,
                if verdict == "DROPPED" {
                    "Cross-namespace traffic requires explicit policy rules to allow it."
                } else {
                    "An egress policy in source namespace or ingress policy in destination namespace allows this."
                }
            )
        }
    }

    fn analyze_security(&self, port: u16, _protocol: &str, verdict: &str, dest_ns: &str) -> String {
        let mut analysis = Vec::new();

        // Check for sensitive ports
        if [22, 3389, 23].contains(&port) {
            analysis.push(format!(
                "⚠️ Port {} is a management/admin port ({}). This should be restricted.",
                port,
                match port {
                    22 => "SSH",
                    3389 => "RDP",
                    23 => "Telnet",
                    _ => "Unknown",
                }
            ));
        }

        // Check for unencrypted traffic
        if [80, 21, 23, 3306, 5432].contains(&port) && verdict == "FORWARDED" {
            analysis.push("⚠️ Unencrypted protocol in use. Consider using encrypted alternatives (HTTPS, SSH, TLS).".to_string());
        }

        // Check for production traffic
        if dest_ns == "production" || dest_ns == "prod" {
            if verdict == "DROPPED" {
                analysis.push("✅ Good: Traffic to production namespace is restricted. This follows zero-trust principles.".to_string());
            } else {
                analysis.push("ℹ️ Traffic to production allowed. Ensure this is intentional and from trusted sources.".to_string());
            }
        }

        if analysis.is_empty() {
            "No significant security concerns detected.".to_string()
        } else {
            analysis.join("\n")
        }
    }

    fn analyze_performance(&self, protocol: &str, port: u16) -> String {
        match protocol {
            "TCP" => {
                if [80, 443, 8080].contains(&port) {
                    "HTTP/HTTPS traffic. Consider enabling HTTP caching, compression, and keep-alive for better performance.".to_string()
                } else {
                    "TCP protocol provides reliable, ordered delivery but has higher latency than UDP.".to_string()
                }
            }
            "UDP" => "UDP protocol provides low-latency, connectionless communication. Good for DNS, streaming, and real-time apps.".to_string(),
            _ => format!("Protocol: {}", protocol),
        }
    }

    fn get_troubleshooting_tips(&self, verdict: &str, port: u16, _protocol: &str) -> Vec<String> {
        let mut tips = Vec::new();

        if verdict == "DROPPED" {
            tips.push(
                "1. Check CiliumNetworkPolicies for the source and destination namespaces"
                    .to_string(),
            );
            tips.push("2. Verify pod labels match the policy selectors".to_string());
            tips.push(format!(
                "3. Add an egress rule allowing traffic to port {}",
                port
            ));
            tips.push("4. Use 'cilium monitor' to see real-time policy verdicts".to_string());
            tips.push("5. Check RootCause tab for recommended policy fixes".to_string());
        } else {
            tips.push("Traffic is flowing normally".to_string());
            tips.push("No troubleshooting needed for this packet".to_string());
        }

        tips
    }

    /// Get statistics
    pub fn stats(&self) -> PacketExplainerStats {
        PacketExplainerStats {
            total_explanations: self.explanations_cache.len(),
            cache_size_kb: (self.explanations_cache.len()
                * std::mem::size_of::<PacketExplanation>())
                / 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PacketExplainerStats {
    pub total_explanations: usize,
    pub cache_size_kb: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_explainer_creation() {
        let explainer = PacketExplainer::new();
        assert_eq!(explainer.stats().total_explanations, 0);
    }

    #[test]
    fn test_explain_dropped_packet() {
        let mut explainer = PacketExplainer::new();
        let explanation = explainer
            .explain_packet(
                "frontend", "web-pod", "backend", "api-pod", 80, "TCP", "DROPPED",
            )
            .unwrap();

        assert_eq!(explanation.verdict, "DROPPED");
        assert!(explanation.what_happened.contains("BLOCKED"));
        assert_eq!(explanation.severity, "Critical"); // port 80 = critical
        assert!(!explanation.troubleshooting_tips.is_empty());
    }

    #[test]
    fn test_explain_forwarded_packet() {
        let mut explainer = PacketExplainer::new();
        let explanation = explainer
            .explain_packet(
                "frontend",
                "web-pod",
                "backend",
                "api-pod",
                443,
                "TCP",
                "FORWARDED",
            )
            .unwrap();

        assert_eq!(explanation.verdict, "FORWARDED");
        assert!(explanation.what_happened.contains("ALLOWED"));
        assert_eq!(explanation.severity, "Low");
    }

    #[test]
    fn test_explain_dns_dropped() {
        let mut explainer = PacketExplainer::new();
        let explanation = explainer
            .explain_packet(
                "app",
                "client",
                "kube-system",
                "coredns",
                53,
                "UDP",
                "DROPPED",
            )
            .unwrap();

        assert!(explanation.why_happened.contains("DNS"));
    }

    #[test]
    fn test_explain_database_dropped() {
        let mut explainer = PacketExplainer::new();
        let explanation = explainer
            .explain_packet("app", "api", "db", "postgres", 5432, "TCP", "DROPPED")
            .unwrap();

        assert!(explanation.why_happened.contains("PostgreSQL"));
    }

    #[test]
    fn test_cross_namespace_policy_context() {
        let mut explainer = PacketExplainer::new();
        let explanation = explainer
            .explain_packet("frontend", "web", "backend", "api", 8080, "TCP", "DROPPED")
            .unwrap();

        assert!(explanation.policy_context.contains("Cross-namespace"));
    }

    #[test]
    fn test_same_namespace_policy_context() {
        let mut explainer = PacketExplainer::new();
        let explanation = explainer
            .explain_packet("app", "client", "app", "server", 8080, "TCP", "FORWARDED")
            .unwrap();

        assert!(explanation.policy_context.contains("same namespace"));
    }

    #[test]
    fn test_security_analysis_sensitive_port() {
        let mut explainer = PacketExplainer::new();
        let explanation = explainer
            .explain_packet("app", "pod", "infra", "server", 22, "TCP", "FORWARDED")
            .unwrap();

        assert!(explanation.security_analysis.contains("SSH"));
    }

    #[test]
    fn test_caching() {
        let mut explainer = PacketExplainer::new();

        // First call generates explanation
        let _ = explainer
            .explain_packet("ns", "pod", "ns2", "pod2", 80, "TCP", "FORWARDED")
            .unwrap();
        assert_eq!(explainer.stats().total_explanations, 1);

        // Second call with same params uses cache
        let _ = explainer
            .explain_packet("ns", "pod", "ns2", "pod2", 80, "TCP", "FORWARDED")
            .unwrap();
        assert_eq!(explainer.stats().total_explanations, 1);

        // Different params creates new entry
        let _ = explainer
            .explain_packet("ns", "pod", "ns3", "pod3", 443, "TCP", "DROPPED")
            .unwrap();
        assert_eq!(explainer.stats().total_explanations, 2);
    }
}
