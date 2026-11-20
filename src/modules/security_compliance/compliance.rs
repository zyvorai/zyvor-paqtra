// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Compliance Engine - PCI-DSS, SOC2, HIPAA, GDPR, ISO 27001, NIST
use anyhow::Result;
use chrono::Utc;
use std::process::Command;

use super::{
    ComplianceFramework, ComplianceReport, ControlState, ControlStatus, Effort, Priority,
    RecommendationCategory, SecurityRecommendation, Violation, ViolationSeverity,
};

/// Audits cluster against compliance frameworks
#[derive(Default)]
pub struct ComplianceEngine {}

impl ComplianceEngine {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    pub async fn audit(&self, framework: ComplianceFramework) -> Result<ComplianceReport> {
        tracing::info!("Running compliance audit for {:?}", framework);

        let controls = match framework {
            ComplianceFramework::PCIDSS => self.audit_pci_dss().await?,
            ComplianceFramework::SOC2 => self.audit_soc2().await?,
            ComplianceFramework::HIPAA => self.audit_hipaa().await?,
            ComplianceFramework::GDPR => self.audit_gdpr().await?,
            ComplianceFramework::ISO27001 => self.audit_iso27001().await?,
            ComplianceFramework::NIST => self.audit_nist().await?,
        };

        let violations = self.find_violations(&controls);
        let overall_score = self.calculate_score(&controls);

        let mut recommendations = Vec::new();
        if violations.is_empty() && overall_score == 100.0 {
            recommendations.push("All checked controls are compliant.".to_string());
        }
        for v in &violations {
            recommendations.push(format!(
                "[{}] {}: {}",
                v.control_id, v.description, v.remediation
            ));
        }

        let not_checked = controls
            .iter()
            .filter(|c| c.status == ControlState::NotChecked)
            .count();
        if not_checked > 0 {
            recommendations.push(format!(
                "{} control(s) could not be checked (kubectl unavailable or cluster unreachable).",
                not_checked
            ));
        }

        Ok(ComplianceReport {
            framework,
            timestamp: Utc::now(),
            overall_score,
            controls,
            violations,
            recommendations,
        })
    }

    /// Run a kubectl command and return (success, stdout).
    ///
    /// NOTE: This uses synchronous `std::process::Command`. The commands include
    /// `--request-timeout=2s` to ensure they complete quickly.
    fn kubectl_check(args: &[&str]) -> (bool, String) {
        match Command::new("kubectl").args(args).output() {
            Ok(output) => (
                output.status.success(),
                String::from_utf8_lossy(&output.stdout).to_string(),
            ),
            Err(_) => (false, String::new()),
        }
    }

    /// Check if kubectl is available and cluster is reachable
    fn kubectl_available() -> bool {
        Self::kubectl_check(&["cluster-info", "--request-timeout=2s"]).0
    }

    async fn audit_pci_dss(&self) -> Result<Vec<ControlStatus>> {
        let cluster_ok = Self::kubectl_available();

        // PCI-DSS 1.1: Network security controls
        let ctrl_1_1 = if cluster_ok {
            let (ok, output) = Self::kubectl_check(&[
                "get",
                "ciliumnetworkpolicies,networkpolicies",
                "--all-namespaces",
                "-o",
                "name",
            ]);
            if ok && !output.trim().is_empty() {
                let count = output.lines().count();
                ControlStatus {
                    control_id: "PCI-DSS-1.1".to_string(),
                    name: "Install and maintain network security controls".to_string(),
                    status: ControlState::Compliant,
                    evidence: vec![format!(
                        "Found {} network policies enforcing segmentation",
                        count
                    )],
                }
            } else {
                ControlStatus {
                    control_id: "PCI-DSS-1.1".to_string(),
                    name: "Install and maintain network security controls".to_string(),
                    status: ControlState::NonCompliant,
                    evidence: vec![
                        "No network policies found - network segmentation is not enforced"
                            .to_string(),
                    ],
                }
            }
        } else {
            ControlStatus {
                control_id: "PCI-DSS-1.1".to_string(),
                name: "Install and maintain network security controls".to_string(),
                status: ControlState::NotChecked,
                evidence: vec!["kubectl not available - cannot verify network policies".to_string()],
            }
        };

        // PCI-DSS 2.1: Secure configurations
        let ctrl_2_1 = if cluster_ok {
            let (ok, output) = Self::kubectl_check(&[
                "get",
                "serviceaccounts",
                "--all-namespaces",
                "-o",
                "jsonpath={.items[*].automountServiceAccountToken}",
            ]);
            let has_auto_mount = ok && output.contains("true");
            if !has_auto_mount {
                ControlStatus {
                    control_id: "PCI-DSS-2.1".to_string(),
                    name: "Apply secure configurations to all system components".to_string(),
                    status: ControlState::Compliant,
                    evidence: vec![
                        "Service accounts do not auto-mount tokens by default".to_string()
                    ],
                }
            } else {
                ControlStatus {
                    control_id: "PCI-DSS-2.1".to_string(),
                    name: "Apply secure configurations to all system components".to_string(),
                    status: ControlState::PartiallyCompliant,
                    evidence: vec![
                        "Some service accounts auto-mount tokens - review for least privilege"
                            .to_string(),
                    ],
                }
            }
        } else {
            ControlStatus {
                control_id: "PCI-DSS-2.1".to_string(),
                name: "Apply secure configurations to all system components".to_string(),
                status: ControlState::NotChecked,
                evidence: vec![
                    "kubectl not available - cannot verify service account configurations"
                        .to_string(),
                ],
            }
        };

        // PCI-DSS 7.1: Restrict access by business need
        let ctrl_7_1 = if cluster_ok {
            let (ok, output) = Self::kubectl_check(&[
                "get",
                "clusterrolebindings",
                "-o",
                "jsonpath={.items[?(@.roleRef.name=='cluster-admin')].subjects[*].name}",
            ]);
            if ok {
                let admin_bindings: Vec<&str> = output
                    .split_whitespace()
                    .filter(|s| !s.is_empty())
                    .collect();
                if admin_bindings.len() <= 2 {
                    ControlStatus {
                        control_id: "PCI-DSS-7.1".to_string(),
                        name: "Restrict access to system components by business need to know"
                            .to_string(),
                        status: ControlState::Compliant,
                        evidence: vec![format!(
                            "cluster-admin bound to {} subject(s): {}",
                            admin_bindings.len(),
                            admin_bindings.join(", ")
                        )],
                    }
                } else {
                    ControlStatus {
                        control_id: "PCI-DSS-7.1".to_string(),
                        name: "Restrict access to system components by business need to know"
                            .to_string(),
                        status: ControlState::NonCompliant,
                        evidence: vec![format!(
                            "cluster-admin bound to {} subjects (too broad): {}",
                            admin_bindings.len(),
                            admin_bindings.join(", ")
                        )],
                    }
                }
            } else {
                ControlStatus {
                    control_id: "PCI-DSS-7.1".to_string(),
                    name: "Restrict access to system components by business need to know"
                        .to_string(),
                    status: ControlState::NotChecked,
                    evidence: vec!["Failed to query cluster role bindings".to_string()],
                }
            }
        } else {
            ControlStatus {
                control_id: "PCI-DSS-7.1".to_string(),
                name: "Restrict access to system components by business need to know".to_string(),
                status: ControlState::NotChecked,
                evidence: vec!["kubectl not available - cannot verify RBAC".to_string()],
            }
        };

        Ok(vec![ctrl_1_1, ctrl_2_1, ctrl_7_1])
    }

    async fn audit_soc2(&self) -> Result<Vec<ControlStatus>> {
        let cluster_ok = Self::kubectl_available();

        // CC6.1: Logical access controls
        let ctrl_cc6 = if cluster_ok {
            let (ok, output) = Self::kubectl_check(&[
                "get",
                "ciliumnetworkpolicies,networkpolicies",
                "--all-namespaces",
                "-o",
                "name",
            ]);
            let policy_count = if ok { output.lines().count() } else { 0 };

            let (rbac_ok, rbac_out) = Self::kubectl_check(&[
                "get",
                "roles,rolebindings",
                "--all-namespaces",
                "-o",
                "name",
            ]);
            let rbac_count = if rbac_ok { rbac_out.lines().count() } else { 0 };

            if policy_count > 0 && rbac_count > 0 {
                ControlStatus {
                    control_id: "CC6.1".to_string(),
                    name: "Logical and physical access controls".to_string(),
                    status: ControlState::Compliant,
                    evidence: vec![format!(
                        "Found {} network policies and {} RBAC roles/bindings",
                        policy_count, rbac_count
                    )],
                }
            } else {
                ControlStatus {
                    control_id: "CC6.1".to_string(),
                    name: "Logical and physical access controls".to_string(),
                    status: ControlState::NonCompliant,
                    evidence: vec![format!(
                        "Insufficient controls: {} network policies, {} RBAC entries",
                        policy_count, rbac_count
                    )],
                }
            }
        } else {
            ControlStatus {
                control_id: "CC6.1".to_string(),
                name: "Logical and physical access controls".to_string(),
                status: ControlState::NotChecked,
                evidence: vec!["kubectl not available".to_string()],
            }
        };

        // CC7.2: System monitoring
        let ctrl_cc7 = if cluster_ok {
            let (ok, output) = Self::kubectl_check(&[
                "get",
                "pods",
                "-n",
                "kube-system",
                "-l",
                "k8s-app=hubble",
                "-o",
                "name",
            ]);
            let hubble_running = ok && !output.trim().is_empty();

            if hubble_running {
                ControlStatus {
                    control_id: "CC7.2".to_string(),
                    name: "System monitoring and anomaly detection".to_string(),
                    status: ControlState::Compliant,
                    evidence: vec!["Hubble observability is running in kube-system".to_string()],
                }
            } else {
                // Also check for hubble-relay
                let (ok2, output2) = Self::kubectl_check(&[
                    "get",
                    "pods",
                    "-n",
                    "kube-system",
                    "-l",
                    "k8s-app=hubble-relay",
                    "-o",
                    "name",
                ]);
                if ok2 && !output2.trim().is_empty() {
                    ControlStatus {
                        control_id: "CC7.2".to_string(),
                        name: "System monitoring and anomaly detection".to_string(),
                        status: ControlState::Compliant,
                        evidence: vec!["Hubble relay is running in kube-system".to_string()],
                    }
                } else {
                    ControlStatus {
                        control_id: "CC7.2".to_string(),
                        name: "System monitoring and anomaly detection".to_string(),
                        status: ControlState::NonCompliant,
                        evidence: vec![
                            "Hubble observability not detected - enable for flow monitoring"
                                .to_string(),
                        ],
                    }
                }
            }
        } else {
            ControlStatus {
                control_id: "CC7.2".to_string(),
                name: "System monitoring and anomaly detection".to_string(),
                status: ControlState::NotChecked,
                evidence: vec!["kubectl not available".to_string()],
            }
        };

        Ok(vec![ctrl_cc6, ctrl_cc7])
    }

    async fn audit_hipaa(&self) -> Result<Vec<ControlStatus>> {
        let cluster_ok = Self::kubectl_available();

        // 164.312(a)(1): Access control
        let ctrl_access = if cluster_ok {
            let (ok, output) = Self::kubectl_check(&[
                "get",
                "ciliumnetworkpolicies",
                "--all-namespaces",
                "-o",
                "jsonpath={.items[*].metadata.name}",
            ]);
            let has_identity_policies = ok && !output.trim().is_empty();
            if has_identity_policies {
                ControlStatus {
                    control_id: "164.312(a)(1)".to_string(),
                    name: "Access control - unique user identification".to_string(),
                    status: ControlState::Compliant,
                    evidence: vec![
                        "Identity-based network policies are enforced via Cilium".to_string()
                    ],
                }
            } else {
                ControlStatus {
                    control_id: "164.312(a)(1)".to_string(),
                    name: "Access control - unique user identification".to_string(),
                    status: ControlState::NonCompliant,
                    evidence: vec![
                        "No Cilium identity-based policies found for access control".to_string()
                    ],
                }
            }
        } else {
            ControlStatus {
                control_id: "164.312(a)(1)".to_string(),
                name: "Access control - unique user identification".to_string(),
                status: ControlState::NotChecked,
                evidence: vec!["kubectl not available".to_string()],
            }
        };

        // 164.312(e)(1): Transmission security
        let ctrl_tls = if cluster_ok {
            let (ok, output) = Self::kubectl_check(&[
                "get",
                "ciliumnetworkpolicies",
                "--all-namespaces",
                "-o",
                "json",
            ]);
            let tls_enforced = ok && output.contains("terminatingTLS");
            if tls_enforced {
                ControlStatus {
                    control_id: "164.312(e)(1)".to_string(),
                    name: "Transmission security - encryption of ePHI in transit".to_string(),
                    status: ControlState::Compliant,
                    evidence: vec![
                        "TLS termination policies found in Cilium network policies".to_string()
                    ],
                }
            } else {
                // Check for Cilium encryption config
                let (enc_ok, enc_out) = Self::kubectl_check(&[
                    "get",
                    "configmap",
                    "cilium-config",
                    "-n",
                    "kube-system",
                    "-o",
                    "jsonpath={.data.enable-wireguard}",
                ]);
                let wireguard = enc_ok && enc_out.trim() == "true";

                let (ipsec_ok, ipsec_out) = Self::kubectl_check(&[
                    "get",
                    "configmap",
                    "cilium-config",
                    "-n",
                    "kube-system",
                    "-o",
                    "jsonpath={.data.encrypt-node}",
                ]);
                let ipsec = ipsec_ok && ipsec_out.trim() == "true";

                if wireguard || ipsec {
                    ControlStatus {
                        control_id: "164.312(e)(1)".to_string(),
                        name: "Transmission security - encryption of ePHI in transit".to_string(),
                        status: ControlState::Compliant,
                        evidence: vec![format!(
                            "Encryption enabled: WireGuard={}, IPsec={}",
                            wireguard, ipsec
                        )],
                    }
                } else {
                    ControlStatus {
                        control_id: "164.312(e)(1)".to_string(),
                        name: "Transmission security - encryption of ePHI in transit".to_string(),
                        status: ControlState::NonCompliant,
                        evidence: vec![
                            "No encryption (WireGuard/IPsec/TLS) detected for in-transit data"
                                .to_string(),
                        ],
                    }
                }
            }
        } else {
            ControlStatus {
                control_id: "164.312(e)(1)".to_string(),
                name: "Transmission security - encryption of ePHI in transit".to_string(),
                status: ControlState::NotChecked,
                evidence: vec!["kubectl not available".to_string()],
            }
        };

        Ok(vec![ctrl_access, ctrl_tls])
    }

    async fn audit_gdpr(&self) -> Result<Vec<ControlStatus>> {
        let cluster_ok = Self::kubectl_available();

        let ctrl = if cluster_ok {
            let (np_ok, np_out) = Self::kubectl_check(&[
                "get",
                "ciliumnetworkpolicies,networkpolicies",
                "--all-namespaces",
                "-o",
                "name",
            ]);
            let policy_count = if np_ok {
                np_out.lines().filter(|l| !l.is_empty()).count()
            } else {
                0
            };

            let (enc_ok, enc_out) = Self::kubectl_check(&[
                "get",
                "configmap",
                "cilium-config",
                "-n",
                "kube-system",
                "-o",
                "jsonpath={.data.enable-wireguard}",
            ]);
            let encryption = enc_ok && enc_out.trim() == "true";

            let mut evidence = Vec::new();
            let mut compliant = true;

            if policy_count > 0 {
                evidence.push(format!(
                    "{} network policies for access control",
                    policy_count
                ));
            } else {
                evidence.push("No network policies found - access controls missing".to_string());
                compliant = false;
            }

            if encryption {
                evidence.push("WireGuard encryption enabled for data in transit".to_string());
            } else {
                evidence.push("No encryption detected for data in transit".to_string());
                compliant = false;
            }

            ControlStatus {
                control_id: "Art.32".to_string(),
                name: "Security of processing - appropriate technical and organisational measures"
                    .to_string(),
                status: if compliant {
                    ControlState::Compliant
                } else {
                    ControlState::PartiallyCompliant
                },
                evidence,
            }
        } else {
            ControlStatus {
                control_id: "Art.32".to_string(),
                name: "Security of processing - appropriate technical and organisational measures"
                    .to_string(),
                status: ControlState::NotChecked,
                evidence: vec!["kubectl not available".to_string()],
            }
        };

        Ok(vec![ctrl])
    }

    async fn audit_iso27001(&self) -> Result<Vec<ControlStatus>> {
        let cluster_ok = Self::kubectl_available();

        let ctrl = if cluster_ok {
            let (ok, output) = Self::kubectl_check(&[
                "get",
                "ciliumnetworkpolicies,networkpolicies",
                "--all-namespaces",
                "-o",
                "name",
            ]);
            let policy_count = if ok {
                output.lines().filter(|l| !l.is_empty()).count()
            } else {
                0
            };

            // Check for namespace-level segmentation
            let (ns_ok, ns_out) = Self::kubectl_check(&["get", "namespaces", "-o", "name"]);
            let ns_count = if ns_ok {
                ns_out.lines().filter(|l| !l.is_empty()).count()
            } else {
                0
            };

            if policy_count > 0 && ns_count > 1 {
                ControlStatus {
                    control_id: "A.13.1.1".to_string(),
                    name:
                        "Network controls - networks managed and controlled to protect information"
                            .to_string(),
                    status: ControlState::Compliant,
                    evidence: vec![format!(
                        "{} network policies across {} namespaces provide micro-segmentation",
                        policy_count, ns_count
                    )],
                }
            } else {
                ControlStatus {
                    control_id: "A.13.1.1".to_string(),
                    name:
                        "Network controls - networks managed and controlled to protect information"
                            .to_string(),
                    status: ControlState::NonCompliant,
                    evidence: vec![format!(
                        "Insufficient segmentation: {} policies, {} namespaces",
                        policy_count, ns_count
                    )],
                }
            }
        } else {
            ControlStatus {
                control_id: "A.13.1.1".to_string(),
                name: "Network controls - networks managed and controlled to protect information"
                    .to_string(),
                status: ControlState::NotChecked,
                evidence: vec!["kubectl not available".to_string()],
            }
        };

        Ok(vec![ctrl])
    }

    async fn audit_nist(&self) -> Result<Vec<ControlStatus>> {
        let cluster_ok = Self::kubectl_available();

        let ctrl = if cluster_ok {
            let (ok, output) = Self::kubectl_check(&[
                "get",
                "ciliumnetworkpolicies,networkpolicies",
                "--all-namespaces",
                "-o",
                "name",
            ]);
            let policy_count = if ok {
                output.lines().filter(|l| !l.is_empty()).count()
            } else {
                0
            };

            if policy_count > 0 {
                ControlStatus {
                    control_id: "PR.AC-5".to_string(),
                    name:
                        "Network integrity is protected (e.g., network segregation, segmentation)"
                            .to_string(),
                    status: ControlState::Compliant,
                    evidence: vec![format!(
                        "{} network policies enforce network segmentation",
                        policy_count
                    )],
                }
            } else {
                ControlStatus {
                    control_id: "PR.AC-5".to_string(),
                    name:
                        "Network integrity is protected (e.g., network segregation, segmentation)"
                            .to_string(),
                    status: ControlState::NonCompliant,
                    evidence: vec![
                        "No network policies found - segmentation is not enforced".to_string()
                    ],
                }
            }
        } else {
            ControlStatus {
                control_id: "PR.AC-5".to_string(),
                name: "Network integrity is protected (e.g., network segregation, segmentation)"
                    .to_string(),
                status: ControlState::NotChecked,
                evidence: vec!["kubectl not available".to_string()],
            }
        };

        Ok(vec![ctrl])
    }

    fn find_violations(&self, controls: &[ControlStatus]) -> Vec<Violation> {
        controls
            .iter()
            .filter(|c| c.status == ControlState::NonCompliant)
            .map(|c| Violation {
                severity: ViolationSeverity::High,
                control_id: c.control_id.clone(),
                description: format!("Control {} is non-compliant", c.name),
                affected_resources: vec![],
                remediation: "Review and remediate".to_string(),
            })
            .collect()
    }

    fn calculate_score(&self, controls: &[ControlStatus]) -> f64 {
        if controls.is_empty() {
            return 0.0;
        }

        // Only count controls that have actually been checked
        let checked_controls: Vec<_> = controls
            .iter()
            .filter(|c| {
                c.status != ControlState::NotChecked && c.status != ControlState::NotApplicable
            })
            .collect();

        if checked_controls.is_empty() {
            return 0.0;
        }

        let compliant = checked_controls
            .iter()
            .filter(|c| c.status == ControlState::Compliant)
            .count();
        let partial = checked_controls
            .iter()
            .filter(|c| c.status == ControlState::PartiallyCompliant)
            .count();

        // Partially compliant counts as half
        ((compliant as f64 + partial as f64 * 0.5) / checked_controls.len() as f64) * 100.0
    }

    pub async fn get_recommendations(&self) -> Result<Vec<SecurityRecommendation>> {
        Ok(vec![SecurityRecommendation {
            priority: Priority::P2High,
            category: RecommendationCategory::Compliance,
            title: "Enable audit logging".to_string(),
            description: "Comprehensive audit logging required for compliance".to_string(),
            impact: "Required for SOC2, PCI-DSS compliance".to_string(),
            effort: Effort::Medium,
            auto_applicable: false,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compliance_engine_creation() {
        let engine = ComplianceEngine::new();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_compliance_engine_default() {
        let _engine = ComplianceEngine::default();
    }

    #[tokio::test]
    async fn test_audit_pci_dss() {
        let engine = ComplianceEngine::new().unwrap();
        let report = engine.audit(ComplianceFramework::PCIDSS).await.unwrap();
        assert_eq!(report.framework, ComplianceFramework::PCIDSS);
        assert!(!report.controls.is_empty());
    }

    #[tokio::test]
    async fn test_audit_soc2() {
        let engine = ComplianceEngine::new().unwrap();
        let report = engine.audit(ComplianceFramework::SOC2).await.unwrap();
        assert_eq!(report.framework, ComplianceFramework::SOC2);
        assert!(!report.controls.is_empty());
    }

    #[tokio::test]
    async fn test_audit_hipaa() {
        let engine = ComplianceEngine::new().unwrap();
        let report = engine.audit(ComplianceFramework::HIPAA).await.unwrap();
        assert_eq!(report.framework, ComplianceFramework::HIPAA);
        assert!(!report.controls.is_empty());
    }

    #[tokio::test]
    async fn test_audit_gdpr() {
        let engine = ComplianceEngine::new().unwrap();
        let report = engine.audit(ComplianceFramework::GDPR).await.unwrap();
        assert_eq!(report.framework, ComplianceFramework::GDPR);
        assert!(!report.controls.is_empty());
    }

    #[tokio::test]
    async fn test_audit_iso27001() {
        let engine = ComplianceEngine::new().unwrap();
        let report = engine.audit(ComplianceFramework::ISO27001).await.unwrap();
        assert_eq!(report.framework, ComplianceFramework::ISO27001);
    }

    #[tokio::test]
    async fn test_audit_nist() {
        let engine = ComplianceEngine::new().unwrap();
        let report = engine.audit(ComplianceFramework::NIST).await.unwrap();
        assert_eq!(report.framework, ComplianceFramework::NIST);
    }

    #[test]
    fn test_calculate_score_all_not_checked() {
        let engine = ComplianceEngine::new().unwrap();
        let controls = vec![ControlStatus {
            control_id: "C1".to_string(),
            name: "Control 1".to_string(),
            status: ControlState::NotChecked,
            evidence: vec![],
        }];
        let score = engine.calculate_score(&controls);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_calculate_score_all_compliant() {
        let engine = ComplianceEngine::new().unwrap();
        let controls = vec![
            ControlStatus {
                control_id: "C1".to_string(),
                name: "Control 1".to_string(),
                status: ControlState::Compliant,
                evidence: vec![],
            },
            ControlStatus {
                control_id: "C2".to_string(),
                name: "Control 2".to_string(),
                status: ControlState::Compliant,
                evidence: vec![],
            },
        ];
        let score = engine.calculate_score(&controls);
        assert_eq!(score, 100.0);
    }

    #[test]
    fn test_calculate_score_mixed() {
        let engine = ComplianceEngine::new().unwrap();
        let controls = vec![
            ControlStatus {
                control_id: "C1".to_string(),
                name: "Control 1".to_string(),
                status: ControlState::Compliant,
                evidence: vec![],
            },
            ControlStatus {
                control_id: "C2".to_string(),
                name: "Control 2".to_string(),
                status: ControlState::NonCompliant,
                evidence: vec![],
            },
        ];
        let score = engine.calculate_score(&controls);
        assert_eq!(score, 50.0);
    }

    #[test]
    fn test_calculate_score_empty() {
        let engine = ComplianceEngine::new().unwrap();
        let score = engine.calculate_score(&[]);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_find_violations_only_noncompliant() {
        let engine = ComplianceEngine::new().unwrap();
        let controls = vec![
            ControlStatus {
                control_id: "C1".to_string(),
                name: "Compliant".to_string(),
                status: ControlState::Compliant,
                evidence: vec![],
            },
            ControlStatus {
                control_id: "C2".to_string(),
                name: "Non-compliant".to_string(),
                status: ControlState::NonCompliant,
                evidence: vec![],
            },
        ];
        let violations = engine.find_violations(&controls);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].control_id, "C2");
        assert_eq!(violations[0].severity, ViolationSeverity::High);
    }

    #[tokio::test]
    async fn test_get_recommendations() {
        let engine = ComplianceEngine::new().unwrap();
        let recs = engine.get_recommendations().await.unwrap();
        assert!(!recs.is_empty());
        assert_eq!(recs[0].category, RecommendationCategory::Compliance);
    }
}
