// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Risk Scorer
///
/// Calculates risk scores for policy changes
use super::*;

pub struct RiskScorer {
    min_confidence: f32,
}

impl RiskScorer {
    pub fn new(min_confidence: f32) -> Self {
        Self { min_confidence }
    }

    /// Assess risk of a policy change
    pub fn assess_risk(
        &self,
        impact: &ImpactAnalysis,
        affected: &AffectedResources,
        scenario: &SimulationScenario,
    ) -> RiskAssessment {
        let mut factors = Vec::new();
        let mut score: u16 = 0;

        // Factor 1: Service Availability Risk
        if !impact.critical_services.is_empty() {
            let severity = (impact.critical_services.len() as u8 * 2).min(10);
            score += u16::from(severity);

            factors.push(RiskFactor {
                category: RiskCategory::ServiceAvailability,
                description: format!(
                    "{} critical services would be affected",
                    impact.critical_services.len()
                ),
                severity,
            });
        }

        // Factor 2: Data Path Risk
        if impact.blocked_flows > 0 {
            let percentage =
                (impact.blocked_flows as f32 / impact.total_flows as f32 * 100.0) as u8;
            let severity: u8 = if percentage > 50 {
                8
            } else if percentage > 25 {
                5
            } else if percentage > 10 {
                3
            } else {
                1
            };

            score += u16::from(severity);

            factors.push(RiskFactor {
                category: RiskCategory::DataPath,
                description: format!(
                    "{}% of traffic would be blocked ({} flows)",
                    percentage, impact.blocked_flows
                ),
                severity,
            });
        }

        // Factor 3: Broken Dependencies
        let critical_deps = affected
            .broken_dependencies
            .iter()
            .filter(|d| d.criticality == DependencyCriticality::Critical)
            .count();

        if critical_deps > 0 {
            let severity = (critical_deps as u8 * 2).min(10);
            score += u16::from(severity);

            factors.push(RiskFactor {
                category: RiskCategory::DataPath,
                description: format!("{} critical dependencies would be broken", critical_deps),
                severity,
            });
        }

        // Factor 4: Security Risk (for allowing traffic)
        match scenario {
            SimulationScenario::AllowTraffic { .. } => {
                factors.push(RiskFactor {
                    category: RiskCategory::Security,
                    description: "Allowing new traffic increases attack surface".to_string(),
                    severity: 2,
                });
                score += 2;
            }
            SimulationScenario::BlockExternalIP { .. } => {
                // Blocking external is generally safer
                factors.push(RiskFactor {
                    category: RiskCategory::Security,
                    description: "Blocking external traffic improves security posture".to_string(),
                    severity: 0, // Positive change
                });
            }
            _ => {}
        }

        // Factor 5: Scope of Change
        let namespaces_affected = affected.namespaces.len();
        if namespaces_affected > 3 {
            let severity: u8 = 3;
            score += u16::from(severity);

            factors.push(RiskFactor {
                category: RiskCategory::DataPath,
                description: format!("{} namespaces would be affected", namespaces_affected),
                severity,
            });
        }

        // Cap score at 10 and narrow to u8
        let score = score.min(10) as u8;

        // Determine risk level
        let level = RiskLevel::from_score(score);

        // Determine if safe to apply
        let (safe_to_apply, unsafe_reasons) = self.determine_safety(score, impact, affected);

        RiskAssessment {
            level,
            score,
            factors,
            safe_to_apply,
            unsafe_reasons,
        }
    }

    /// Determine if change is safe to apply
    fn determine_safety(
        &self,
        score: u8,
        impact: &ImpactAnalysis,
        affected: &AffectedResources,
    ) -> (bool, Vec<String>) {
        let mut unsafe_reasons = Vec::new();

        // Critical risk score
        if score >= 8 {
            unsafe_reasons.push(format!("Risk score too high ({})", score));
        }

        // Critical services affected
        if !impact.critical_services.is_empty() {
            unsafe_reasons.push(format!(
                "Critical services affected: {}",
                impact.critical_services.join(", ")
            ));
        }

        // Too many flows blocked
        if impact.total_flows > 0 {
            let block_percentage =
                (impact.blocked_flows as f32 / impact.total_flows as f32) * 100.0;
            if block_percentage > 50.0 {
                unsafe_reasons.push(format!(
                    "More than 50% of flows would be blocked ({:.0}%)",
                    block_percentage
                ));
            }
        }

        // Critical dependencies broken
        let critical_deps = affected
            .broken_dependencies
            .iter()
            .filter(|d| d.criticality == DependencyCriticality::Critical)
            .count();

        if critical_deps > 0 {
            unsafe_reasons.push(format!(
                "{} critical dependencies would be broken",
                critical_deps
            ));
        }

        // Fully blocked services
        let fully_blocked = affected
            .services
            .iter()
            .filter(|s| s.impact_type == ImpactType::FullyBlocked)
            .count();

        if fully_blocked > 0 {
            unsafe_reasons.push(format!("{} services would be fully blocked", fully_blocked));
        }

        let safe = unsafe_reasons.is_empty() && score < 6;

        (safe, unsafe_reasons)
    }

    /// Calculate confidence adjustment based on data quality
    pub fn adjust_confidence(&self, base_confidence: f32, impact: &ImpactAnalysis) -> f32 {
        let mut adjusted = base_confidence;

        // Reduce confidence if very few flows
        if impact.total_flows < 100 {
            adjusted *= 0.8;
        }

        // Reduce confidence if impact is extreme
        if impact.blocked_flows > impact.total_flows / 2 {
            adjusted *= 0.9;
        }

        // Ensure within bounds
        adjusted.clamp(0.0, 1.0)
    }

    /// Generate risk summary
    pub fn summarize_risk(&self, risk: &RiskAssessment) -> String {
        let mut summary = format!("Risk Level: {} (Score: {}/10)\n", risk.level, risk.score);

        if risk.safe_to_apply {
            summary.push_str("✅ Safe to apply\n");
        } else {
            summary.push_str("⚠️  NOT SAFE to apply\n");
        }

        if !risk.unsafe_reasons.is_empty() {
            summary.push_str("\nReasons:\n");
            for reason in &risk.unsafe_reasons {
                summary.push_str(&format!("  • {}\n", reason));
            }
        }

        if !risk.factors.is_empty() {
            summary.push_str("\nRisk Factors:\n");
            for factor in &risk.factors {
                summary.push_str(&format!(
                    "  • {} (severity: {}): {}\n",
                    Self::category_name(&factor.category),
                    factor.severity,
                    factor.description
                ));
            }
        }

        summary
    }

    fn category_name(category: &RiskCategory) -> &'static str {
        match category {
            RiskCategory::ServiceAvailability => "Service Availability",
            RiskCategory::DataPath => "Data Path",
            RiskCategory::Security => "Security",
            RiskCategory::Compliance => "Compliance",
            RiskCategory::Performance => "Performance",
        }
    }

    /// Compare scenarios
    pub fn compare_scenarios(
        &self,
        scenarios: &[(SimulationScenario, RiskAssessment)],
    ) -> Vec<(usize, String)> {
        let mut rankings = Vec::new();

        for (idx, (scenario, risk)) in scenarios.iter().enumerate() {
            let description = format!("{:?} - Risk: {} ({})", scenario, risk.level, risk.score);
            rankings.push((idx, description));
        }

        // Sort by risk score (lower is better)
        rankings.sort_by_key(|(idx, _)| scenarios[*idx].1.score);

        rankings
    }

    /// Suggest mitigations
    pub fn suggest_mitigations(&self, risk: &RiskAssessment) -> Vec<String> {
        let mut mitigations = Vec::new();

        for factor in &risk.factors {
            match factor.category {
                RiskCategory::ServiceAvailability => {
                    mitigations
                        .push("Consider gradual rollout with canary deployments".to_string());
                    mitigations.push("Ensure monitoring and rollback plan is ready".to_string());
                }
                RiskCategory::DataPath => {
                    if factor.severity > 5 {
                        mitigations.push("Test in staging environment first".to_string());
                        mitigations.push("Review and validate all blocked flows".to_string());
                    }
                }
                RiskCategory::Security => {
                    mitigations.push("Enable audit mode first to observe behavior".to_string());
                }
                _ => {}
            }
        }

        // Remove duplicates
        mitigations.sort();
        mitigations.dedup();

        mitigations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_scorer_creation() {
        let scorer = RiskScorer::new(0.7);
        assert_eq!(scorer.min_confidence, 0.7);
    }

    #[test]
    fn test_assess_risk_low() {
        let scorer = RiskScorer::new(0.7);

        let impact = ImpactAnalysis {
            total_flows: 100,
            blocked_flows: 5,
            allowed_flows: 95,
            changed_flows: 5,
            impacted_services: vec![],
            critical_services: vec![],
        };

        let affected = AffectedResources {
            namespaces: vec!["default".to_string()],
            pod_identities: vec![],
            services: vec![],
            endpoints: vec![],
            broken_dependencies: vec![],
        };

        let scenario = SimulationScenario::DefaultDeny {
            namespace: "test".to_string(),
        };

        let risk = scorer.assess_risk(&impact, &affected, &scenario);

        assert!(risk.score < 5);
        assert_eq!(risk.level, RiskLevel::Low);
    }

    #[test]
    fn test_assess_risk_high() {
        let scorer = RiskScorer::new(0.7);

        let impact = ImpactAnalysis {
            total_flows: 100,
            blocked_flows: 80,
            allowed_flows: 20,
            changed_flows: 80,
            impacted_services: vec!["web".to_string(), "api".to_string()],
            critical_services: vec!["database".to_string(), "auth".to_string()],
        };

        let affected = AffectedResources {
            namespaces: vec!["prod".to_string()],
            pod_identities: vec![],
            services: vec![],
            endpoints: vec![],
            broken_dependencies: vec![Dependency {
                from_service: "web".to_string(),
                to_service: "database".to_string(),
                port: 5432,
                protocol: "TCP".to_string(),
                criticality: DependencyCriticality::Critical,
            }],
        };

        let scenario = SimulationScenario::BlockExternalIP {
            ip: "8.8.8.8".parse().unwrap(),
        };

        let risk = scorer.assess_risk(&impact, &affected, &scenario);

        assert!(risk.score >= 6);
        assert!(!risk.safe_to_apply);
    }

    #[test]
    fn test_adjust_confidence() {
        let scorer = RiskScorer::new(0.7);

        let impact_low = ImpactAnalysis {
            total_flows: 50,
            blocked_flows: 5,
            allowed_flows: 45,
            changed_flows: 5,
            impacted_services: vec![],
            critical_services: vec![],
        };

        let confidence = scorer.adjust_confidence(0.9, &impact_low);
        assert!(confidence < 0.9); // Should be reduced due to low flow count

        let impact_high = ImpactAnalysis {
            total_flows: 1000,
            blocked_flows: 10,
            allowed_flows: 990,
            changed_flows: 10,
            impacted_services: vec![],
            critical_services: vec![],
        };

        let confidence = scorer.adjust_confidence(0.9, &impact_high);
        assert!(confidence >= 0.85); // Should stay high
    }
}
