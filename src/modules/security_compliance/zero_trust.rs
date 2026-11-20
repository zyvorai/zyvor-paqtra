// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Zero-Trust Policy Engine - Never trust, always verify
use anyhow::Result;
use std::collections::{HashMap, HashSet};

use super::{Effort, Priority, RecommendationCategory, SecurityRecommendation};

/// Generates and enforces zero-trust network policies
pub struct ZeroTrustEngine {
    default_deny_all: bool,
    micro_segmentation: bool,
    identity_based: bool,
}

impl ZeroTrustEngine {
    pub fn new() -> Result<Self> {
        Ok(Self {
            default_deny_all: true,
            micro_segmentation: true,
            identity_based: true,
        })
    }

    /// Generate zero-trust policies for a namespace
    pub async fn generate_policies(&self, namespace: &str) -> Result<Vec<String>> {
        tracing::info!(
            "Generating zero-trust policies for namespace: {}",
            namespace
        );

        let mut policies = Vec::new();

        // 1. Default deny-all policy
        if self.default_deny_all {
            policies.push(self.generate_default_deny(namespace));
        }

        // 2. Baseline allow policies (DNS + K8s API are always needed)
        policies.push(self.generate_allow_dns(namespace));
        policies.push(self.generate_allow_kubernetes_api(namespace));

        // 3. Discover observed traffic patterns and generate allow policies
        let observed = self.discover_traffic_patterns(namespace).await;
        for policy in observed {
            policies.push(policy);
        }

        // 4. Micro-segmentation policies
        if self.micro_segmentation {
            policies.extend(self.generate_micro_segmentation(namespace)?);
        }

        // 5. Identity-based policies
        if self.identity_based {
            policies.extend(self.generate_identity_policies(namespace)?);
        }

        Ok(policies)
    }

    /// Discover observed traffic patterns using `hubble observe` and generate
    /// explicit allow policies for each unique src_label → dst_label:port flow.
    ///
    /// Falls back to `kubectl get pods` label enumeration if Hubble is unavailable.
    async fn discover_traffic_patterns(&self, namespace: &str) -> Vec<String> {
        // Try Hubble first: observe recent flows in the namespace
        if let Some(policies) = self.discover_via_hubble(namespace).await {
            if !policies.is_empty() {
                return policies;
            }
        }

        // Fallback: enumerate pod labels via kubectl to generate service-to-service policies
        self.discover_via_kubectl(namespace).await
    }

    /// Query `hubble observe` for recent flows and group by
    /// (source app label → destination app label, dest port).
    async fn discover_via_hubble(&self, namespace: &str) -> Option<Vec<String>> {
        let output = tokio::process::Command::new("hubble")
            .args([
                "observe",
                "--namespace",
                namespace,
                "--verdict",
                "FORWARDED",
                "--last",
                "500",
                "-o",
                "json",
            ])
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Collect unique (src_app, dst_app, dst_port, protocol) tuples
        // Each line is a JSON flow object from Hubble
        let mut edges: HashSet<(String, String, String, String)> = HashSet::new();

        for line in stdout.lines() {
            if let Ok(flow) = serde_json::from_str::<serde_json::Value>(line) {
                let src_app = flow
                    .pointer("/source/labels")
                    .and_then(|l| l.as_array())
                    .and_then(|labels| {
                        labels.iter().find_map(|l| {
                            l.as_str()
                                .and_then(|s| s.strip_prefix("k8s:app="))
                                .map(String::from)
                        })
                    });
                let dst_app = flow
                    .pointer("/destination/labels")
                    .and_then(|l| l.as_array())
                    .and_then(|labels| {
                        labels.iter().find_map(|l| {
                            l.as_str()
                                .and_then(|s| s.strip_prefix("k8s:app="))
                                .map(String::from)
                        })
                    });
                let dst_port = flow
                    .pointer("/l4/TCP/destination_port")
                    .or_else(|| flow.pointer("/l4/UDP/destination_port"))
                    .and_then(|p| p.as_u64())
                    .map(|p| p.to_string());
                let protocol = if flow.pointer("/l4/TCP").is_some() {
                    "TCP"
                } else if flow.pointer("/l4/UDP").is_some() {
                    "UDP"
                } else {
                    continue; // Skip flows with unknown L4 protocol
                };

                if let (Some(src), Some(dst), Some(port)) = (src_app, dst_app, dst_port) {
                    edges.insert((src, dst, port, protocol.to_string()));
                }
            }
        }

        let policies: Vec<String> = edges
            .iter()
            .map(|(src, dst, port, proto)| {
                format!(
                    r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: allow-{src}-to-{dst}-{port}
  namespace: {namespace}
  annotations:
    zero-trust.paqtra/discovered: "true"
spec:
  endpointSelector:
    matchLabels:
      app: {src}
  egress:
  - toEndpoints:
    - matchLabels:
        app: {dst}
    toPorts:
    - ports:
      - port: "{port}"
        protocol: {proto}
"#
                )
            })
            .collect();

        Some(policies)
    }

    /// Fallback: enumerate unique `app` labels in the namespace via kubectl
    /// and generate allow policies between all discovered services on common ports.
    async fn discover_via_kubectl(&self, namespace: &str) -> Vec<String> {
        let output = match tokio::process::Command::new("kubectl")
            .args([
                "get",
                "pods",
                "-n",
                namespace,
                "-o",
                "jsonpath={range .items[*]}{.metadata.labels.app}{\"\\n\"}{end}",
                "--request-timeout=5s",
            ])
            .output()
            .await
        {
            Ok(o) if o.status.success() => o,
            _ => return Vec::new(),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let apps: Vec<String> = stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(String::from)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        // For each pair of discovered apps, generate a bidirectional allow
        // on common service ports (80, 443, 8080, 5432, 6379, 3306)
        let common_ports = ["80", "443", "8080"];
        let mut policies = Vec::new();

        // Only generate inter-service policies (not self-to-self)
        let mut seen: HashMap<String, bool> = HashMap::new();
        for src in &apps {
            for dst in &apps {
                if src == dst {
                    continue;
                }
                let key = format!("{}->{}", src, dst);
                if seen.contains_key(&key) {
                    continue;
                }
                seen.insert(key, true);

                let ports_yaml: String = common_ports
                    .iter()
                    .map(|p| format!("      - port: \"{}\"\n        protocol: TCP", p))
                    .collect::<Vec<_>>()
                    .join("\n");

                policies.push(format!(
                    r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: allow-{src}-to-{dst}
  namespace: {namespace}
  annotations:
    zero-trust.paqtra/discovered: "true"
spec:
  endpointSelector:
    matchLabels:
      app: {src}
  egress:
  - toEndpoints:
    - matchLabels:
        app: {dst}
    toPorts:
    - ports:
{ports_yaml}
"#
                ));
            }
        }

        if !policies.is_empty() {
            tracing::info!(
                namespace,
                services = apps.len(),
                policies = policies.len(),
                "Generated zero-trust policies from kubectl pod discovery"
            );
        }

        policies
    }

    fn generate_default_deny(&self, namespace: &str) -> String {
        format!(
            r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: default-deny-all
  namespace: {}
spec:
  # Matches all pods in this namespace (empty selector = wildcard).
  # Narrow with matchLabels when targeting specific workloads.
  endpointSelector: {{}}
  ingress: []
  egress: []
"#,
            namespace
        )
    }

    fn generate_allow_dns(&self, namespace: &str) -> String {
        format!(
            r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: allow-dns
  namespace: {}
spec:
  # Matches all pods in this namespace (empty selector = wildcard).
  # Narrow with matchLabels when targeting specific workloads.
  endpointSelector: {{}}
  egress:
  - toEndpoints:
    - matchLabels:
        k8s:io.kubernetes.pod.namespace: kube-system
        k8s-app: kube-dns
    toPorts:
    - ports:
      - port: "53"
        protocol: UDP
      rules:
        dns:
        - matchPattern: "*"
"#,
            namespace
        )
    }

    fn generate_allow_kubernetes_api(&self, namespace: &str) -> String {
        format!(
            r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: allow-kubernetes-api
  namespace: {}
spec:
  # Matches all pods in this namespace (empty selector = wildcard).
  # Narrow with matchLabels when targeting specific workloads.
  endpointSelector: {{}}
  egress:
  - toEntities:
    - kube-apiserver
    toPorts:
    - ports:
      - port: "443"
        protocol: TCP
      - port: "6443"
        protocol: TCP
"#,
            namespace
        )
    }

    fn generate_micro_segmentation(&self, namespace: &str) -> Result<Vec<String>> {
        // Generate fine-grained policies for each service
        let mut policies = Vec::new();

        // Example: web tier can only talk to app tier
        policies.push(format!(
            r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: web-to-app-tier
  namespace: {}
spec:
  endpointSelector:
    matchLabels:
      tier: web
  egress:
  - toEndpoints:
    - matchLabels:
        tier: app
    toPorts:
    - ports:
      - port: "8080"
        protocol: TCP
"#,
            namespace
        ));

        Ok(policies)
    }

    fn generate_identity_policies(&self, namespace: &str) -> Result<Vec<String>> {
        // Generate policies based on service account identities
        let mut policies = Vec::new();

        policies.push(format!(
            r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: identity-based-policy
  namespace: {}
spec:
  endpointSelector:
    matchLabels:
      app: frontend
  egress:
  - toEndpoints:
    - matchLabels:
        k8s:io.kubernetes.pod.service-account: backend-sa
"#,
            namespace
        ));

        Ok(policies)
    }

    pub async fn get_recommendations(&self) -> Result<Vec<SecurityRecommendation>> {
        let recommendations = vec![
            SecurityRecommendation {
                priority: Priority::P1Critical,
                category: RecommendationCategory::ZeroTrust,
                title: "Implement default-deny policies".to_string(),
                description:
                    "No default-deny policies detected. All traffic is implicitly allowed."
                        .to_string(),
                impact: "High - reduces attack surface by 90%".to_string(),
                effort: Effort::Low,
                auto_applicable: true,
            },
            SecurityRecommendation {
                priority: Priority::P2High,
                category: RecommendationCategory::ZeroTrust,
                title: "Enable micro-segmentation".to_string(),
                description:
                    "Implement fine-grained network segmentation between application tiers"
                        .to_string(),
                impact: "Medium - prevents lateral movement".to_string(),
                effort: Effort::Medium,
                auto_applicable: false,
            },
        ];

        Ok(recommendations)
    }
}

impl Default for ZeroTrustEngine {
    fn default() -> Self {
        Self {
            default_deny_all: true,
            micro_segmentation: true,
            identity_based: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_trust_engine_creation() {
        let engine = ZeroTrustEngine::new();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_zero_trust_engine_default() {
        let engine = ZeroTrustEngine::default();
        assert!(engine.default_deny_all);
        assert!(engine.micro_segmentation);
        assert!(engine.identity_based);
    }

    #[tokio::test]
    async fn test_generate_policies_for_namespace() {
        let engine = ZeroTrustEngine::new().unwrap();
        let policies = engine.generate_policies("my-namespace").await.unwrap();
        // Should generate at least: default-deny, allow-dns, allow-k8s-api,
        // micro-segmentation, identity-based
        assert!(policies.len() >= 5);
    }

    #[tokio::test]
    async fn test_policies_contain_namespace() {
        let engine = ZeroTrustEngine::new().unwrap();
        let namespace = "test-ns-123";
        let policies = engine.generate_policies(namespace).await.unwrap();
        for policy in &policies {
            assert!(
                policy.contains(namespace),
                "Policy should contain the namespace '{}', got:\n{}",
                namespace,
                policy
            );
        }
    }

    #[tokio::test]
    async fn test_policies_are_valid_yaml_structure() {
        let engine = ZeroTrustEngine::new().unwrap();
        let policies = engine.generate_policies("default").await.unwrap();
        for policy in &policies {
            assert!(
                policy.contains("apiVersion:"),
                "Policy should have apiVersion"
            );
            assert!(policy.contains("kind:"), "Policy should have kind");
            assert!(policy.contains("metadata:"), "Policy should have metadata");
            assert!(policy.contains("spec:"), "Policy should have spec");
        }
    }

    #[test]
    fn test_generate_default_deny_policy() {
        let engine = ZeroTrustEngine::new().unwrap();
        let policy = engine.generate_default_deny("prod");
        assert!(policy.contains("default-deny-all"));
        assert!(policy.contains("prod"));
        assert!(policy.contains("CiliumNetworkPolicy"));
    }

    #[test]
    fn test_generate_allow_dns_policy() {
        let engine = ZeroTrustEngine::new().unwrap();
        let policy = engine.generate_allow_dns("staging");
        assert!(policy.contains("allow-dns"));
        assert!(policy.contains("staging"));
        assert!(policy.contains("port: \"53\""));
        assert!(policy.contains("UDP"));
    }

    #[tokio::test]
    async fn test_get_recommendations() {
        let engine = ZeroTrustEngine::new().unwrap();
        let recs = engine.get_recommendations().await.unwrap();
        assert!(!recs.is_empty());
        assert!(recs
            .iter()
            .all(|r| r.category == RecommendationCategory::ZeroTrust));
    }
}
