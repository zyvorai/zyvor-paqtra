// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Self-Healer Module
///
/// Automatically detects and fixes common network issues:
/// - DNS resolution failures
/// - MTU mismatches
/// - Load balancer timeouts
/// - Policy gaps
/// - Connection tracking issues
use anyhow::Result;
use std::collections::HashMap;

use crate::ebpf::{DropReason, DropReasonType, EnrichedMapReader, MapReader};
use crate::kubernetes::K8sClient;
use crate::modules::yaml_escape;
use crate::policies::PolicyManager;

const UNKNOWN: &str = "unknown";

pub mod dns;
pub mod mtu;
pub mod policy;

#[derive(Debug, Clone)]
pub struct HealerConfig {
    pub enabled: bool,
    pub auto_apply: bool,
    pub dry_run: bool,
    pub check_interval_secs: u64,
}

impl Default for HealerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_apply: false, // Safe default: suggest but don't apply
            dry_run: true,
            check_interval_secs: 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    DNSDrops {
        namespace: String,
        pod: String,
        count: u64,
    },
    MTUMismatch {
        namespace: String,
        pod: String,
        expected: u16,
        actual: u16,
    },
    PolicyGap {
        src_namespace: String,
        src_pod: String,
        dst_namespace: String,
        dst_pod: String,
        port: u16,
        protocol: String,
    },
    LoadBalancerTimeout {
        service: String,
        backend: String,
        timeout_count: u64,
    },
    ConntrackFull {
        node: String,
        utilization: f64,
    },
}

#[derive(Debug, Clone)]
pub struct Fix {
    pub problem: Problem,
    pub action: FixAction,
    pub applied: bool,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FixAction {
    CreateDNSPolicy {
        namespace: String,
    },
    AdjustMTU {
        namespace: String,
        pod: String,
        new_mtu: u16,
    },
    CreateAllowPolicy {
        src: String,
        dst: String,
        port: u16,
    },
    RebalanceBackend {
        service: String,
        backend: String,
    },
    TuneConntrack {
        node: String,
        new_timeout: u32,
    },
}

pub struct SelfHealer<M: MapReader> {
    config: HealerConfig,
    ebpf_reader: M,
    k8s_client: K8sClient,
    policy_manager: PolicyManager,
    detected_problems: Vec<Problem>,
    applied_fixes: Vec<Fix>,
}

impl<M: MapReader> SelfHealer<M> {
    pub fn new(config: HealerConfig, ebpf_reader: M, k8s_client: K8sClient) -> Self {
        let policy_manager = PolicyManager::new(k8s_client.clone());

        Self {
            config,
            ebpf_reader,
            k8s_client,
            policy_manager,
            detected_problems: Vec::new(),
            applied_fixes: Vec::new(),
        }
    }

    /// Main healing loop - detect and fix problems
    pub async fn run(&mut self) -> Result<HealerStats> {
        if !self.config.enabled {
            return Ok(HealerStats::default());
        }

        let mut stats = HealerStats::default();

        // Detect problems from eBPF data
        self.detect_problems().await?;
        stats.problems_detected = self.detected_problems.len();
        tracing::info!(
            problem_count = self.detected_problems.len(),
            "Healer problem detection completed"
        );

        // Generate fixes
        let fixes = self.generate_fixes().await?;
        stats.fixes_proposed = fixes.len();

        // Apply fixes (if auto_apply enabled and not dry_run)
        if self.config.auto_apply && !self.config.dry_run {
            for mut fix in fixes {
                if self.apply_fix(&fix).await.is_ok() {
                    fix.applied = true;
                    self.applied_fixes.push(fix);
                    stats.fixes_applied += 1;
                }
            }
        }

        Ok(stats)
    }

    /// Detect problems from eBPF drops
    async fn detect_problems(&mut self) -> Result<()> {
        self.detected_problems.clear();

        // Read drop reasons from eBPF
        let drops = self.ebpf_reader.read_drop_map()?;

        // Analyze drops for patterns
        let dns_drops = self.analyze_dns_drops(&drops);
        self.detected_problems.extend(dns_drops);

        let policy_gaps = self.analyze_policy_gaps(&drops);
        self.detected_problems.extend(policy_gaps);

        Ok(())
    }

    /// Analyze DNS-related drops
    fn analyze_dns_drops(&self, drops: &[DropReason]) -> Vec<Problem> {
        let mut dns_problems = Vec::new();
        let mut dns_drop_count: HashMap<String, u64> = HashMap::new();

        for drop in drops {
            if drop.port == 53 && drop.reason == DropReasonType::PolicyDenied {
                *dns_drop_count.entry(drop.src_ip.clone()).or_insert(0) += 1;
            }
        }

        for (ip, count) in dns_drop_count {
            if count > 5 {
                // Threshold: more than 5 DNS drops
                dns_problems.push(Problem::DNSDrops {
                    namespace: UNKNOWN.to_string(), // Will be resolved if using EnrichedMapReader
                    pod: ip,
                    count,
                });
            }
        }

        dns_problems
    }

    /// Analyze policy-related drops
    fn analyze_policy_gaps(&self, drops: &[DropReason]) -> Vec<Problem> {
        let mut problems = Vec::new();

        for drop in drops {
            if drop.reason == DropReasonType::PolicyDenied {
                problems.push(Problem::PolicyGap {
                    src_namespace: UNKNOWN.to_string(),
                    src_pod: drop.src_ip.clone(),
                    dst_namespace: UNKNOWN.to_string(),
                    dst_pod: drop.dst_ip.clone(),
                    port: drop.port,
                    protocol: if drop.protocol == 6 {
                        "TCP".to_string()
                    } else {
                        "UDP".to_string()
                    },
                });
            }
        }

        problems
    }

    /// Generate fixes for detected problems
    async fn generate_fixes(&self) -> Result<Vec<Fix>> {
        let mut fixes = Vec::new();

        for problem in &self.detected_problems {
            match problem {
                Problem::DNSDrops { namespace, .. } => {
                    fixes.push(Fix {
                        problem: problem.clone(),
                        action: FixAction::CreateDNSPolicy {
                            namespace: namespace.clone(),
                        },
                        applied: false,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    });
                }
                Problem::PolicyGap {
                    src_namespace,
                    dst_pod,
                    port,
                    ..
                } => {
                    fixes.push(Fix {
                        problem: problem.clone(),
                        action: FixAction::CreateAllowPolicy {
                            src: src_namespace.clone(),
                            dst: dst_pod.clone(),
                            port: *port,
                        },
                        applied: false,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    });
                }
                _ => {}
            }
        }

        Ok(fixes)
    }

    /// Apply a fix
    async fn apply_fix(&mut self, fix: &Fix) -> Result<()> {
        match &fix.action {
            FixAction::CreateDNSPolicy { namespace } => {
                self.policy_manager.apply_dns_policy(namespace).await?;
                println!("✔ Applied DNS policy fix for namespace: {}", namespace);
            }
            FixAction::CreateAllowPolicy { src, dst, port } => {
                let policy_yaml = format!(
                    r#"
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: auto-allow-{port}
  namespace: {src_esc}
spec:
  endpointSelector: {{}}
  egress:
    - toEndpoints:
        - matchLabels:
            io.kubernetes.pod.ip: {dst_esc}
      toPorts:
        - ports:
            - port: "{port}"
              protocol: TCP
"#,
                    src_esc = yaml_escape(src),
                    dst_esc = yaml_escape(dst),
                    port = port,
                );
                self.k8s_client
                    .apply_custom_resource(Some(src), &policy_yaml)
                    .await?;
                println!("Applied allow policy: {} -> {}:{}", src, dst, port);
            }
            FixAction::AdjustMTU {
                namespace,
                pod,
                new_mtu,
            } => {
                // Cilium's MTU is one cluster-wide setting (`mtu` in cilium-config /
                // the Helm value), not a per-pod one, and Cilium reads no pod
                // annotation for it. Record the recommendation on the pod so it is
                // visible, and say plainly that nothing was changed.
                let annotation = format!("paqtra.io/recommended-mtu={}", new_mtu);
                self.k8s_client
                    .annotate_pod(namespace, pod, &annotation)
                    .await?;
                println!(
                    "✔ Recorded recommended MTU {} on pod {}/{} (advisory: set `mtu` in cilium-config to change it)",
                    new_mtu, namespace, pod
                );
            }
            FixAction::RebalanceBackend { service, backend } => {
                // Restart the backend pod to trigger rebalancing
                let parts: Vec<&str> = backend.splitn(2, '/').collect();
                let (ns, pod_prefix) = if parts.len() == 2 {
                    (parts[0], parts[1])
                } else {
                    ("default", backend.as_str())
                };

                // Scale down and up to force rebalance
                tracing::info!(
                    service = %service,
                    backend = %backend,
                    "Rebalancing backend by restarting pod"
                );
                self.k8s_client.restart_rollout(ns, pod_prefix).await?;
                println!("✔ Rebalanced backend {} for service {}", backend, service);
            }
            FixAction::TuneConntrack { node, new_timeout } => {
                // Apply conntrack tuning via a configmap update or sysctl
                tracing::info!(
                    node = %node,
                    new_timeout,
                    "Tuning conntrack timeout"
                );
                let configmap_patch = format!(
                    r#"{{"data":{{"ct-global-max-entries-per-node":"{}"}}}}"#,
                    new_timeout
                );
                self.k8s_client
                    .patch_configmap("kube-system", "cilium-config", &configmap_patch)
                    .await?;
                println!(
                    "✔ Tuned conntrack for node {} (timeout={})",
                    node, new_timeout
                );
            }
        }

        Ok(())
    }

    /// Get statistics
    pub fn stats(&self) -> HealerStats {
        HealerStats {
            problems_detected: self.detected_problems.len(),
            fixes_proposed: self.applied_fixes.len(),
            fixes_applied: self.applied_fixes.iter().filter(|f| f.applied).count(),
        }
    }

    /// Get detected problems
    pub fn problems(&self) -> &[Problem] {
        &self.detected_problems
    }

    /// Get applied fixes
    pub fn fixes(&self) -> &[Fix] {
        &self.applied_fixes
    }
}

/// Enhanced implementation for EnrichedMapReader
impl SelfHealer<EnrichedMapReader> {
    /// Detect problems with full pod context
    pub async fn detect_problems_enriched(&mut self) -> Result<()> {
        self.detected_problems.clear();

        // Read enriched drops from eBPF
        let enriched_drops = self.ebpf_reader.read_enriched_drops()?;

        // Analyze DNS drops with pod context
        let dns_drops = self.analyze_dns_drops_enriched(&enriched_drops);
        self.detected_problems.extend(dns_drops);

        // Analyze policy gaps with pod context
        let policy_gaps = self.analyze_policy_gaps_enriched(&enriched_drops);
        self.detected_problems.extend(policy_gaps);

        Ok(())
    }

    /// Analyze DNS-related drops with enriched pod information
    fn analyze_dns_drops_enriched(
        &self,
        drops: &[(DropReason, crate::ebpf::EnrichedDropInfo)],
    ) -> Vec<Problem> {
        let mut dns_problems = Vec::new();
        let mut dns_drop_count: HashMap<(String, String), u64> = HashMap::new();

        for (drop, info) in drops {
            if drop.port == 53 && drop.reason == DropReasonType::PolicyDenied {
                let key = (
                    info.src_namespace
                        .clone()
                        .unwrap_or_else(|| UNKNOWN.to_string()),
                    info.src_pod.clone().unwrap_or_else(|| drop.src_ip.clone()),
                );
                *dns_drop_count.entry(key).or_insert(0) += 1;
            }
        }

        for ((namespace, pod), count) in dns_drop_count {
            if count > 5 {
                dns_problems.push(Problem::DNSDrops {
                    namespace,
                    pod,
                    count,
                });
            }
        }

        dns_problems
    }

    /// Analyze policy-related drops with enriched pod information
    fn analyze_policy_gaps_enriched(
        &self,
        drops: &[(DropReason, crate::ebpf::EnrichedDropInfo)],
    ) -> Vec<Problem> {
        let mut problems = Vec::new();

        for (drop, info) in drops {
            if drop.reason == DropReasonType::PolicyDenied {
                problems.push(Problem::PolicyGap {
                    src_namespace: info
                        .src_namespace
                        .clone()
                        .unwrap_or_else(|| UNKNOWN.to_string()),
                    src_pod: info.src_pod.clone().unwrap_or_else(|| drop.src_ip.clone()),
                    dst_namespace: info
                        .dst_namespace
                        .clone()
                        .unwrap_or_else(|| UNKNOWN.to_string()),
                    dst_pod: info.dst_pod.clone().unwrap_or_else(|| drop.dst_ip.clone()),
                    port: drop.port,
                    protocol: if drop.protocol == 6 {
                        "TCP".to_string()
                    } else {
                        "UDP".to_string()
                    },
                });
            }
        }

        problems
    }

    /// Enhanced run loop with pod context
    pub async fn run_enriched(&mut self) -> Result<HealerStats> {
        if !self.config.enabled {
            return Ok(HealerStats::default());
        }

        let mut stats = HealerStats::default();

        // Detect problems from enriched eBPF data
        self.detect_problems_enriched().await?;
        stats.problems_detected = self.detected_problems.len();

        // Generate fixes
        let fixes = self.generate_fixes().await?;
        stats.fixes_proposed = fixes.len();

        // Apply fixes (if auto_apply enabled and not dry_run)
        if self.config.auto_apply && !self.config.dry_run {
            for mut fix in fixes {
                if self.apply_fix(&fix).await.is_ok() {
                    fix.applied = true;
                    self.applied_fixes.push(fix);
                    stats.fixes_applied += 1;
                }
            }
        }

        Ok(stats)
    }
}

#[derive(Debug, Clone, Default)]
pub struct HealerStats {
    pub problems_detected: usize,
    pub fixes_proposed: usize,
    pub fixes_applied: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ebpf::MockMapReader;

    #[tokio::test]
    #[ignore] // Requires a live Kubernetes cluster
    async fn test_healer_creation() {
        let config = HealerConfig::default();
        let reader = MockMapReader;
        let k8s_client = K8sClient::new().await.unwrap();

        let healer = SelfHealer::new(config, reader, k8s_client);
        assert!(!healer.config.auto_apply); // Safe default
    }

    #[test]
    fn test_healer_config_defaults() {
        let config = HealerConfig::default();
        assert!(config.enabled);
        assert!(!config.auto_apply);
        assert!(config.dry_run);
        assert_eq!(config.check_interval_secs, 30);
    }

    #[tokio::test]
    async fn test_analyze_dns_drops_empty() {
        let config = HealerConfig::default();
        let reader = MockMapReader;
        let k8s_client = K8sClient::mock();
        let healer = SelfHealer::new(config, reader, k8s_client);

        let drops: Vec<DropReason> = vec![];
        let problems = healer.analyze_dns_drops(&drops);
        assert!(problems.is_empty());
    }

    #[tokio::test]
    async fn test_analyze_dns_drops_threshold() {
        let config = HealerConfig::default();
        let reader = MockMapReader;
        let k8s_client = K8sClient::mock();
        let healer = SelfHealer::new(config, reader, k8s_client);

        // 5 DNS drops from same IP - at threshold (> 5 needed)
        let drops: Vec<DropReason> = (0..5)
            .map(|_| DropReason {
                src_ip: "10.0.0.1".to_string(),
                dst_ip: "10.96.0.10".to_string(),
                port: 53,
                protocol: 17,
                reason: DropReasonType::PolicyDenied,
                timestamp: 0,
            })
            .collect();

        let problems = healer.analyze_dns_drops(&drops);
        assert!(problems.is_empty()); // 5 is not > 5

        // 6 DNS drops - above threshold
        let drops: Vec<DropReason> = (0..6)
            .map(|_| DropReason {
                src_ip: "10.0.0.1".to_string(),
                dst_ip: "10.96.0.10".to_string(),
                port: 53,
                protocol: 17,
                reason: DropReasonType::PolicyDenied,
                timestamp: 0,
            })
            .collect();

        let problems = healer.analyze_dns_drops(&drops);
        assert_eq!(problems.len(), 1);
    }

    #[tokio::test]
    async fn test_analyze_dns_drops_non_dns_ignored() {
        let config = HealerConfig::default();
        let reader = MockMapReader;
        let k8s_client = K8sClient::mock();
        let healer = SelfHealer::new(config, reader, k8s_client);

        // Drops on port 80, not DNS
        let drops: Vec<DropReason> = (0..10)
            .map(|_| DropReason {
                src_ip: "10.0.0.1".to_string(),
                dst_ip: "10.0.0.2".to_string(),
                port: 80,
                protocol: 6,
                reason: DropReasonType::PolicyDenied,
                timestamp: 0,
            })
            .collect();

        let problems = healer.analyze_dns_drops(&drops);
        assert!(problems.is_empty());
    }

    #[tokio::test]
    async fn test_analyze_policy_gaps() {
        let config = HealerConfig::default();
        let reader = MockMapReader;
        let k8s_client = K8sClient::mock();
        let healer = SelfHealer::new(config, reader, k8s_client);

        let drops = vec![
            DropReason {
                src_ip: "10.0.0.1".to_string(),
                dst_ip: "10.0.0.2".to_string(),
                port: 80,
                protocol: 6,
                reason: DropReasonType::PolicyDenied,
                timestamp: 0,
            },
            DropReason {
                src_ip: "10.0.0.3".to_string(),
                dst_ip: "10.0.0.4".to_string(),
                port: 443,
                protocol: 6,
                reason: DropReasonType::PolicyDenied,
                timestamp: 0,
            },
        ];

        let problems = healer.analyze_policy_gaps(&drops);
        assert_eq!(problems.len(), 2);

        match &problems[0] {
            Problem::PolicyGap {
                src_pod,
                dst_pod,
                port,
                protocol,
                ..
            } => {
                assert_eq!(src_pod, "10.0.0.1");
                assert_eq!(dst_pod, "10.0.0.2");
                assert_eq!(*port, 80);
                assert_eq!(protocol, "TCP");
            }
            _ => panic!("Expected PolicyGap"),
        }
    }

    #[tokio::test]
    async fn test_analyze_policy_gaps_non_policy_ignored() {
        let config = HealerConfig::default();
        let reader = MockMapReader;
        let k8s_client = K8sClient::mock();
        let healer = SelfHealer::new(config, reader, k8s_client);

        let drops = vec![DropReason {
            src_ip: "10.0.0.1".to_string(),
            dst_ip: "10.0.0.2".to_string(),
            port: 80,
            protocol: 6,
            reason: DropReasonType::FragmentationNeeded,
            timestamp: 0,
        }];

        let problems = healer.analyze_policy_gaps(&drops);
        assert!(problems.is_empty());
    }

    #[tokio::test]
    async fn test_analyze_policy_gaps_udp_protocol() {
        let config = HealerConfig::default();
        let reader = MockMapReader;
        let k8s_client = K8sClient::mock();
        let healer = SelfHealer::new(config, reader, k8s_client);

        let drops = vec![DropReason {
            src_ip: "10.0.0.1".to_string(),
            dst_ip: "10.0.0.2".to_string(),
            port: 53,
            protocol: 17,
            reason: DropReasonType::PolicyDenied,
            timestamp: 0,
        }];

        let problems = healer.analyze_policy_gaps(&drops);
        assert_eq!(problems.len(), 1);
        match &problems[0] {
            Problem::PolicyGap { protocol, .. } => {
                assert_eq!(protocol, "UDP");
            }
            _ => panic!("Expected PolicyGap"),
        }
    }

    #[tokio::test]
    async fn test_healer_stats_initial() {
        let config = HealerConfig::default();
        let reader = MockMapReader;
        let k8s_client = K8sClient::mock();
        let healer = SelfHealer::new(config, reader, k8s_client);

        let stats = healer.stats();
        assert_eq!(stats.problems_detected, 0);
        assert_eq!(stats.fixes_proposed, 0);
        assert_eq!(stats.fixes_applied, 0);
    }

    #[tokio::test]
    async fn test_healer_problems_empty() {
        let config = HealerConfig::default();
        let reader = MockMapReader;
        let k8s_client = K8sClient::mock();
        let healer = SelfHealer::new(config, reader, k8s_client);

        assert!(healer.problems().is_empty());
        assert!(healer.fixes().is_empty());
    }

    #[test]
    fn test_problem_equality() {
        let p1 = Problem::DNSDrops {
            namespace: "default".to_string(),
            pod: "web".to_string(),
            count: 10,
        };
        let p2 = Problem::DNSDrops {
            namespace: "default".to_string(),
            pod: "web".to_string(),
            count: 10,
        };
        assert_eq!(p1, p2);
    }

    #[test]
    fn test_fix_action_equality() {
        let a1 = FixAction::CreateDNSPolicy {
            namespace: "default".to_string(),
        };
        let a2 = FixAction::CreateDNSPolicy {
            namespace: "default".to_string(),
        };
        assert_eq!(a1, a2);

        let a3 = FixAction::AdjustMTU {
            namespace: "prod".to_string(),
            pod: "web".to_string(),
            new_mtu: 1450,
        };
        let a4 = FixAction::CreateDNSPolicy {
            namespace: "prod".to_string(),
        };
        assert_ne!(a3, a4);
    }

    #[test]
    fn test_healer_stats_default() {
        let stats = HealerStats::default();
        assert_eq!(stats.problems_detected, 0);
        assert_eq!(stats.fixes_proposed, 0);
        assert_eq!(stats.fixes_applied, 0);
    }
}
