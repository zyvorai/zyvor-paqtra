// Security Posture Assessment
use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use super::{
    DimensionScore, Effort, Priority, RecommendationCategory, ScoreTrend, SecurityRecommendation,
    SecurityScore,
};

/// File where the last posture score is persisted for trend comparison.
const SCORE_HISTORY_FILE: &str = ".paqtra-posture-score";

/// System namespaces excluded from network-policy coverage calculations.
const SYSTEM_NAMESPACES: &[&str] = &["kube-system", "kube-public", "kube-node-lease"];

/// Calculates overall security posture from four equally-weighted components:
///
/// 1. **Encryption** (25%) - WireGuard / IPsec via Cilium config
/// 2. **RBAC coverage** (25%) - ServiceAccount RBAC bindings per namespace
/// 3. **Network policy coverage** (25%) - CiliumNetworkPolicy per non-system namespace
/// 4. **Hubble observability** (25%) - Hubble relay health
#[derive(Default)]
pub struct SecurityPosture {}

impl SecurityPosture {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    // -----------------------------------------------------------------------
    // kubectl helpers
    // -----------------------------------------------------------------------

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

    fn kubectl_available() -> bool {
        Self::kubectl_check(&["cluster-info", "--request-timeout=2s"]).0
    }

    // -----------------------------------------------------------------------
    // Component 1: Encryption (25%)
    // -----------------------------------------------------------------------

    /// Score 100 if WireGuard **or** IPsec is enabled in the cilium-config
    /// ConfigMap, 0 otherwise.
    fn assess_encryption(&self) -> DimensionScore {
        if !Self::kubectl_available() {
            return DimensionScore {
                score: 0.0,
                weight: 0.25,
                findings: vec!["kubectl not available - cannot assess encryption".to_string()],
            };
        }

        let mut findings = Vec::new();

        // Check WireGuard
        let (wg_ok, wg_out) = Self::kubectl_check(&[
            "get",
            "configmap",
            "cilium-config",
            "-n",
            "kube-system",
            "-o",
            "jsonpath={.data.enable-wireguard}",
            "--request-timeout=2s",
        ]);
        let wireguard_enabled = wg_ok && wg_out.trim() == "true";

        // Check IPsec
        let (ipsec_ok, ipsec_out) = Self::kubectl_check(&[
            "get",
            "configmap",
            "cilium-config",
            "-n",
            "kube-system",
            "-o",
            "jsonpath={.data.encrypt-node}",
            "--request-timeout=2s",
        ]);
        let ipsec_enabled = ipsec_ok && ipsec_out.trim() == "true";

        let score = if wireguard_enabled || ipsec_enabled {
            if wireguard_enabled {
                findings.push("WireGuard encryption enabled for pod-to-pod traffic".to_string());
            }
            if ipsec_enabled {
                findings.push("IPsec node-level encryption enabled".to_string());
            }
            100.0
        } else {
            findings.push(
                "No encryption detected - enable WireGuard or IPsec in Cilium config".to_string(),
            );
            0.0
        };

        DimensionScore {
            score,
            weight: 0.25,
            findings,
        }
    }

    // -----------------------------------------------------------------------
    // Component 2: RBAC coverage (25%)
    // -----------------------------------------------------------------------

    /// Score based on the percentage of non-system namespaces that have at
    /// least one RBAC Role or RoleBinding.
    fn assess_rbac_coverage(&self) -> DimensionScore {
        if !Self::kubectl_available() {
            return DimensionScore {
                score: 0.0,
                weight: 0.25,
                findings: vec!["kubectl not available - cannot assess RBAC".to_string()],
            };
        }

        let mut findings = Vec::new();

        // List all namespaces
        let (ns_ok, ns_out) =
            Self::kubectl_check(&["get", "namespaces", "-o", "name", "--request-timeout=2s"]);
        let all_namespaces: Vec<String> = if ns_ok {
            ns_out
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.trim_start_matches("namespace/").to_string())
                .collect()
        } else {
            findings.push("Failed to list namespaces".to_string());
            return DimensionScore {
                score: 0.0,
                weight: 0.25,
                findings,
            };
        };

        let user_namespaces: Vec<&String> = all_namespaces
            .iter()
            .filter(|ns| !SYSTEM_NAMESPACES.contains(&ns.as_str()))
            .collect();

        if user_namespaces.is_empty() {
            findings.push("No non-system namespaces found".to_string());
            return DimensionScore {
                score: 0.0,
                weight: 0.25,
                findings,
            };
        }

        // Collect namespaces that have roles or rolebindings
        let (rb_ok, rb_out) = Self::kubectl_check(&[
            "get",
            "roles,rolebindings",
            "--all-namespaces",
            "-o",
            "custom-columns=NAMESPACE:.metadata.namespace",
            "--no-headers",
            "--request-timeout=2s",
        ]);
        let ns_with_rbac: std::collections::HashSet<String> = if rb_ok {
            rb_out
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.trim().to_string())
                .collect()
        } else {
            std::collections::HashSet::new()
        };

        let covered = user_namespaces
            .iter()
            .filter(|ns| ns_with_rbac.contains(ns.as_str()))
            .count();
        let total = user_namespaces.len();
        let pct = if total > 0 {
            (covered as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        findings.push(format!(
            "{}/{} non-system namespaces have RBAC roles/bindings ({:.0}%)",
            covered, total, pct
        ));

        if pct < 100.0 {
            let missing: Vec<String> = user_namespaces
                .iter()
                .filter(|ns| !ns_with_rbac.contains(ns.as_str()))
                .map(|ns| ns.to_string())
                .collect();
            if !missing.is_empty() {
                findings.push(format!("Namespaces without RBAC: {}", missing.join(", ")));
            }
        }

        DimensionScore {
            score: pct,
            weight: 0.25,
            findings,
        }
    }

    // -----------------------------------------------------------------------
    // Component 3: Network policy coverage (25%)
    // -----------------------------------------------------------------------

    /// Score based on the percentage of non-system namespaces that have at
    /// least one CiliumNetworkPolicy.
    fn assess_network_policy_coverage(&self) -> DimensionScore {
        if !Self::kubectl_available() {
            return DimensionScore {
                score: 0.0,
                weight: 0.25,
                findings: vec!["kubectl not available - cannot assess network policies".to_string()],
            };
        }

        let mut findings = Vec::new();

        // List all namespaces
        let (ns_ok, ns_out) =
            Self::kubectl_check(&["get", "namespaces", "-o", "name", "--request-timeout=2s"]);
        let all_namespaces: Vec<String> = if ns_ok {
            ns_out
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.trim_start_matches("namespace/").to_string())
                .collect()
        } else {
            findings.push("Failed to list namespaces".to_string());
            return DimensionScore {
                score: 0.0,
                weight: 0.25,
                findings,
            };
        };

        let user_namespaces: Vec<&String> = all_namespaces
            .iter()
            .filter(|ns| !SYSTEM_NAMESPACES.contains(&ns.as_str()))
            .collect();

        if user_namespaces.is_empty() {
            findings.push("No non-system namespaces found".to_string());
            return DimensionScore {
                score: 0.0,
                weight: 0.25,
                findings,
            };
        }

        // Collect namespaces that have CiliumNetworkPolicies
        let (cnp_ok, cnp_out) = Self::kubectl_check(&[
            "get",
            "ciliumnetworkpolicies",
            "--all-namespaces",
            "-o",
            "custom-columns=NAMESPACE:.metadata.namespace",
            "--no-headers",
            "--request-timeout=2s",
        ]);
        let ns_with_policies: std::collections::HashSet<String> = if cnp_ok {
            cnp_out
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.trim().to_string())
                .collect()
        } else {
            std::collections::HashSet::new()
        };

        let covered = user_namespaces
            .iter()
            .filter(|ns| ns_with_policies.contains(ns.as_str()))
            .count();
        let total = user_namespaces.len();
        let pct = if total > 0 {
            (covered as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        findings.push(format!(
            "{}/{} non-system namespaces have CiliumNetworkPolicy ({:.0}%)",
            covered, total, pct
        ));

        if pct < 100.0 {
            let missing: Vec<String> = user_namespaces
                .iter()
                .filter(|ns| !ns_with_policies.contains(ns.as_str()))
                .map(|ns| ns.to_string())
                .collect();
            if !missing.is_empty() {
                findings.push(format!(
                    "Namespaces without CiliumNetworkPolicy: {}",
                    missing.join(", ")
                ));
            }
        }

        DimensionScore {
            score: pct,
            weight: 0.25,
            findings,
        }
    }

    // -----------------------------------------------------------------------
    // Component 4: Hubble observability (25%)
    // -----------------------------------------------------------------------

    /// Score 100 if Hubble relay pods are Running, 50 if they exist but are
    /// not in Running phase, 0 if not installed at all.
    fn assess_hubble_observability(&self) -> DimensionScore {
        if !Self::kubectl_available() {
            return DimensionScore {
                score: 0.0,
                weight: 0.25,
                findings: vec![
                    "kubectl not available - cannot assess Hubble observability".to_string()
                ],
            };
        }

        let mut findings = Vec::new();

        // Check for Running hubble-relay pods
        let (running_ok, running_out) = Self::kubectl_check(&[
            "get",
            "pods",
            "-n",
            "kube-system",
            "-l",
            "k8s-app=hubble-relay",
            "--field-selector=status.phase=Running",
            "-o",
            "name",
            "--request-timeout=2s",
        ]);
        let running_count = if running_ok {
            running_out.lines().filter(|l| !l.is_empty()).count()
        } else {
            0
        };

        if running_count > 0 {
            findings.push(format!(
                "Hubble relay is healthy ({} Running pod(s))",
                running_count
            ));
            return DimensionScore {
                score: 100.0,
                weight: 0.25,
                findings,
            };
        }

        // Relay not Running — check if pods exist at all (any phase)
        let (exists_ok, exists_out) = Self::kubectl_check(&[
            "get",
            "pods",
            "-n",
            "kube-system",
            "-l",
            "k8s-app=hubble-relay",
            "-o",
            "name",
            "--request-timeout=2s",
        ]);
        let exists_count = if exists_ok {
            exists_out.lines().filter(|l| !l.is_empty()).count()
        } else {
            0
        };

        if exists_count > 0 {
            findings.push(format!(
                "Hubble relay pods exist ({}) but none are Running",
                exists_count
            ));
            return DimensionScore {
                score: 50.0,
                weight: 0.25,
                findings,
            };
        }

        findings.push("Hubble relay is not installed".to_string());
        DimensionScore {
            score: 0.0,
            weight: 0.25,
            findings,
        }
    }

    // -----------------------------------------------------------------------
    // Trend persistence
    // -----------------------------------------------------------------------

    /// Return the path used for persisting the last score. Uses
    /// `$PAQTRA_POSTURE_SCORE_FILE` when set, otherwise
    /// `$HOME/.paqtra-posture-score`, falling back to `/tmp`.
    fn score_history_path() -> PathBuf {
        if let Ok(p) = std::env::var("PAQTRA_POSTURE_SCORE_FILE") {
            if !p.is_empty() {
                return PathBuf::from(p);
            }
        }
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"))
            .join(SCORE_HISTORY_FILE)
    }

    /// Load the previously stored score (if any).
    fn load_previous_score() -> Option<f64> {
        let path = Self::score_history_path();
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok())
    }

    /// Persist the current score so the next run can compute a trend.
    fn save_current_score(score: f64) {
        let path = Self::score_history_path();
        if let Err(e) = std::fs::write(&path, format!("{:.2}", score)) {
            tracing::warn!("Failed to persist posture score to {:?}: {}", path, e);
        }
    }

    /// Compare `current` against an optional `previous` score.
    /// A difference of more than 5 points is considered meaningful.
    fn compare_scores(current: f64, previous: Option<f64>) -> ScoreTrend {
        match previous {
            Some(prev) => {
                let delta = current - prev;
                if delta > 5.0 {
                    ScoreTrend::Improving
                } else if delta < -5.0 {
                    ScoreTrend::Declining
                } else {
                    ScoreTrend::Stable
                }
            }
            // No history available — assume stable
            None => ScoreTrend::Stable,
        }
    }

    /// Determine the trend by comparing the current score to the last stored
    /// score on disk.
    fn determine_trend(current: f64) -> ScoreTrend {
        Self::compare_scores(current, Self::load_previous_score())
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    pub async fn calculate_score(&self) -> Result<SecurityScore> {
        let mut dimensions = HashMap::new();

        dimensions.insert("Encryption".to_string(), self.assess_encryption());
        dimensions.insert("RBAC Coverage".to_string(), self.assess_rbac_coverage());
        dimensions.insert(
            "Network Policy Coverage".to_string(),
            self.assess_network_policy_coverage(),
        );
        dimensions.insert(
            "Hubble Observability".to_string(),
            self.assess_hubble_observability(),
        );

        let overall_score = dimensions.values().map(|d| d.score * d.weight).sum::<f64>();

        // Compare with the last stored score to determine the trend
        let trend = Self::determine_trend(overall_score);

        // Persist the new score for next comparison
        Self::save_current_score(overall_score);

        tracing::info!("Security posture score: {:.1}/100", overall_score);

        Ok(SecurityScore {
            overall_score,
            dimensions,
            timestamp: Utc::now(),
            trend,
        })
    }

    pub async fn get_recommendations(&self) -> Result<Vec<SecurityRecommendation>> {
        let mut recs = Vec::new();

        let cluster_ok = Self::kubectl_available();

        // ---- Encryption recommendations ----
        if cluster_ok {
            let (wg_ok, wg_out) = Self::kubectl_check(&[
                "get",
                "configmap",
                "cilium-config",
                "-n",
                "kube-system",
                "-o",
                "jsonpath={.data.enable-wireguard}",
                "--request-timeout=2s",
            ]);
            let wireguard = wg_ok && wg_out.trim() == "true";

            let (ipsec_ok, ipsec_out) = Self::kubectl_check(&[
                "get",
                "configmap",
                "cilium-config",
                "-n",
                "kube-system",
                "-o",
                "jsonpath={.data.encrypt-node}",
                "--request-timeout=2s",
            ]);
            let ipsec = ipsec_ok && ipsec_out.trim() == "true";

            if !wireguard && !ipsec {
                recs.push(SecurityRecommendation {
                    priority: Priority::P1Critical,
                    category: RecommendationCategory::BestPractice,
                    title: "Enable WireGuard or IPsec encryption".to_string(),
                    description: "No in-transit encryption is configured. Enable WireGuard \
                                  (recommended) or IPsec in the Cilium Helm values to encrypt \
                                  pod-to-pod traffic."
                        .to_string(),
                    impact: "Protects all pod-to-pod data in transit within the cluster"
                        .to_string(),
                    effort: Effort::Low,
                    auto_applicable: false,
                });
            }
        }

        // ---- RBAC recommendations ----
        if cluster_ok {
            let rbac_dim = self.assess_rbac_coverage();
            if rbac_dim.score < 100.0 {
                recs.push(SecurityRecommendation {
                    priority: Priority::P2High,
                    category: RecommendationCategory::BestPractice,
                    title: "Improve RBAC coverage across namespaces".to_string(),
                    description: format!(
                        "Only {:.0}% of non-system namespaces have RBAC roles/bindings. \
                         Create Role and RoleBinding resources in uncovered namespaces to \
                         enforce least-privilege access.",
                        rbac_dim.score
                    ),
                    impact: "Reduces blast radius of compromised service accounts".to_string(),
                    effort: Effort::Medium,
                    auto_applicable: false,
                });
            }
        }

        // ---- Network policy recommendations ----
        if cluster_ok {
            let np_dim = self.assess_network_policy_coverage();
            if np_dim.score < 100.0 {
                recs.push(SecurityRecommendation {
                    priority: Priority::P1Critical,
                    category: RecommendationCategory::BestPractice,
                    title: "Add CiliumNetworkPolicy to all namespaces".to_string(),
                    description: format!(
                        "Only {:.0}% of non-system namespaces have a CiliumNetworkPolicy. \
                         Deploy at least a default-deny policy in every namespace to enforce \
                         micro-segmentation.",
                        np_dim.score
                    ),
                    impact: "Prevents lateral movement and limits blast radius of compromised pods"
                        .to_string(),
                    effort: Effort::Low,
                    auto_applicable: true,
                });
            }
        }

        // ---- Hubble observability recommendations ----
        if cluster_ok {
            let hubble_dim = self.assess_hubble_observability();
            if hubble_dim.score < 100.0 {
                let (title, description) = if hubble_dim.score == 0.0 {
                    (
                        "Install Hubble relay for flow observability".to_string(),
                        "Hubble is not installed. Enable it via the Cilium Helm chart \
                         (hubble.enabled=true, hubble.relay.enabled=true) to gain full \
                         network flow visibility."
                            .to_string(),
                    )
                } else {
                    (
                        "Fix unhealthy Hubble relay pods".to_string(),
                        "Hubble relay pods exist but are not Running. Check pod events \
                         and logs to restore observability."
                            .to_string(),
                    )
                };
                recs.push(SecurityRecommendation {
                    priority: Priority::P2High,
                    category: RecommendationCategory::BestPractice,
                    title,
                    description,
                    impact: "Enables real-time network flow monitoring and anomaly detection"
                        .to_string(),
                    effort: Effort::Low,
                    auto_applicable: false,
                });
            }
        }

        // ---- Generic recommendation when kubectl is not available ----
        if !cluster_ok {
            recs.push(SecurityRecommendation {
                priority: Priority::P2High,
                category: RecommendationCategory::BestPractice,
                title: "Connect to a Kubernetes cluster".to_string(),
                description: "kubectl is not available or the cluster is unreachable. \
                              Ensure KUBECONFIG is set and the cluster is accessible for \
                              a complete posture assessment."
                    .to_string(),
                impact: "Required for any security posture assessment".to_string(),
                effort: Effort::Low,
                auto_applicable: false,
            });
        }

        Ok(recs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_posture_creation() {
        let posture = SecurityPosture::new();
        assert!(posture.is_ok());
    }

    #[test]
    fn test_security_posture_default() {
        let _posture = SecurityPosture::default();
    }

    #[tokio::test]
    async fn test_calculate_score_returns_valid_score() {
        let posture = SecurityPosture::new().unwrap();
        let score = posture.calculate_score().await.unwrap();
        assert!(score.overall_score >= 0.0 && score.overall_score <= 100.0);
    }

    #[tokio::test]
    async fn test_score_dimensions_present() {
        let posture = SecurityPosture::new().unwrap();
        let score = posture.calculate_score().await.unwrap();
        assert!(score.dimensions.contains_key("Encryption"));
        assert!(score.dimensions.contains_key("RBAC Coverage"));
        assert!(score.dimensions.contains_key("Network Policy Coverage"));
        assert!(score.dimensions.contains_key("Hubble Observability"));
    }

    #[tokio::test]
    async fn test_dimension_weights_sum_to_one() {
        let posture = SecurityPosture::new().unwrap();
        let score = posture.calculate_score().await.unwrap();
        let total_weight: f64 = score.dimensions.values().map(|d| d.weight).sum();
        assert!(
            (total_weight - 1.0).abs() < 0.001,
            "Dimension weights should sum to 1.0, got {}",
            total_weight
        );
    }

    #[tokio::test]
    async fn test_all_dimensions_scored() {
        let posture = SecurityPosture::new().unwrap();
        let score = posture.calculate_score().await.unwrap();
        assert_eq!(
            score.dimensions.len(),
            4,
            "Should have exactly 4 dimensions"
        );
        // Each dimension should have a score between 0 and 100
        for (name, dim) in &score.dimensions {
            assert!(
                dim.score >= 0.0 && dim.score <= 100.0,
                "Dimension '{}' score {} out of range",
                name,
                dim.score
            );
            // Each dimension should have at least one finding
            assert!(
                !dim.findings.is_empty(),
                "Dimension '{}' should have findings",
                name
            );
        }
    }

    #[tokio::test]
    async fn test_score_trend_determined() {
        let posture = SecurityPosture::new().unwrap();
        let score = posture.calculate_score().await.unwrap();
        // Trend should be one of the valid values
        assert!(
            score.trend == ScoreTrend::Stable
                || score.trend == ScoreTrend::Improving
                || score.trend == ScoreTrend::Declining
        );
    }

    #[tokio::test]
    async fn test_get_recommendations() {
        let posture = SecurityPosture::new().unwrap();
        let recs = posture.get_recommendations().await.unwrap();
        assert!(!recs.is_empty());
        assert_eq!(recs[0].category, RecommendationCategory::BestPractice);
    }

    #[test]
    fn test_score_history_path_is_deterministic() {
        let path1 = SecurityPosture::score_history_path();
        let path2 = SecurityPosture::score_history_path();
        assert_eq!(path1, path2);
        assert!(
            path1.to_string_lossy().contains(SCORE_HISTORY_FILE),
            "Path should contain the score file name"
        );
    }

    #[test]
    fn test_save_and_load_score() {
        // Isolate from parallel calculate_score tests that write the shared HOME file.
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = std::env::temp_dir().join(format!(
            ".paqtra-posture-score-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::env::set_var("PAQTRA_POSTURE_SCORE_FILE", &path);
        SecurityPosture::save_current_score(72.5);
        let loaded = SecurityPosture::load_previous_score();
        let _ = std::fs::remove_file(&path);
        std::env::remove_var("PAQTRA_POSTURE_SCORE_FILE");
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert!(
            (loaded - 72.5).abs() < 0.1,
            "Loaded score should be ~72.5, got {}",
            loaded
        );
    }

    #[test]
    fn test_compare_scores_improving() {
        let trend = SecurityPosture::compare_scores(60.0, Some(40.0));
        assert_eq!(trend, ScoreTrend::Improving);
    }

    #[test]
    fn test_compare_scores_declining() {
        let trend = SecurityPosture::compare_scores(50.0, Some(80.0));
        assert_eq!(trend, ScoreTrend::Declining);
    }

    #[test]
    fn test_compare_scores_stable() {
        let trend = SecurityPosture::compare_scores(77.0, Some(75.0));
        assert_eq!(trend, ScoreTrend::Stable);
    }

    #[test]
    fn test_compare_scores_no_history() {
        let trend = SecurityPosture::compare_scores(50.0, None);
        assert_eq!(trend, ScoreTrend::Stable);
    }

    #[test]
    fn test_compare_scores_boundary_not_improving() {
        // Exactly 5 points difference is NOT enough to trigger Improving
        let trend = SecurityPosture::compare_scores(55.0, Some(50.0));
        assert_eq!(trend, ScoreTrend::Stable);
    }

    #[test]
    fn test_compare_scores_boundary_not_declining() {
        // Exactly -5 points difference is NOT enough to trigger Declining
        let trend = SecurityPosture::compare_scores(50.0, Some(55.0));
        assert_eq!(trend, ScoreTrend::Stable);
    }

    #[test]
    fn test_all_weights_equal() {
        let posture = SecurityPosture::new().unwrap();
        let dims = vec![
            posture.assess_encryption(),
            posture.assess_rbac_coverage(),
            posture.assess_network_policy_coverage(),
            posture.assess_hubble_observability(),
        ];
        for dim in &dims {
            assert!(
                (dim.weight - 0.25).abs() < f64::EPSILON,
                "Each dimension weight should be 0.25, got {}",
                dim.weight
            );
        }
    }
}
