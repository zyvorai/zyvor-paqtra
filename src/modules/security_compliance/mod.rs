// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Security & Compliance Module - Zero-trust, compliance frameworks, threat intelligence
// Experimental: Enterprise-grade security and compliance features

pub mod compliance;
pub mod posture;
pub mod threat_intel;
pub mod zero_trust;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Comprehensive security and compliance manager
pub struct SecurityComplianceManager {
    zero_trust: zero_trust::ZeroTrustEngine,
    compliance: compliance::ComplianceEngine,
    threat_intel: threat_intel::ThreatIntelligence,
    posture: posture::SecurityPosture,
}

impl SecurityComplianceManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            zero_trust: zero_trust::ZeroTrustEngine::new()?,
            compliance: compliance::ComplianceEngine::new()?,
            threat_intel: threat_intel::ThreatIntelligence::new()?,
            posture: posture::SecurityPosture::new()?,
        })
    }

    /// Generate zero-trust policies for a namespace
    pub async fn generate_zero_trust_policies(&mut self, namespace: &str) -> Result<Vec<String>> {
        self.zero_trust.generate_policies(namespace).await
    }

    /// Run compliance audit for a framework
    pub async fn run_compliance_audit(
        &mut self,
        framework: ComplianceFramework,
    ) -> Result<ComplianceReport> {
        self.compliance.audit(framework).await
    }

    /// Check threat intelligence for an IP or domain
    pub async fn check_threat_intel(&mut self, indicator: &str) -> Result<ThreatAssessment> {
        self.threat_intel.assess(indicator).await
    }

    /// Calculate security posture score
    pub async fn calculate_security_posture(&mut self) -> Result<SecurityScore> {
        self.posture.calculate_score().await
    }

    /// Get security recommendations
    pub async fn get_recommendations(&mut self) -> Result<Vec<SecurityRecommendation>> {
        let mut recommendations = Vec::new();

        // Collect from all engines
        recommendations.extend(self.zero_trust.get_recommendations().await?);
        recommendations.extend(self.compliance.get_recommendations().await?);
        recommendations.extend(self.threat_intel.get_recommendations().await?);
        recommendations.extend(self.posture.get_recommendations().await?);

        // Sort by priority
        recommendations.sort_by(|a, b| b.priority.cmp(&a.priority));

        Ok(recommendations)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ComplianceFramework {
    /// PCI DSS (Payment Card Industry Data Security Standard)
    PCIDSS,
    /// SOC 2 (Service Organization Control 2)
    SOC2,
    /// HIPAA (Health Insurance Portability and Accountability Act)
    HIPAA,
    /// GDPR (General Data Protection Regulation)
    GDPR,
    /// ISO 27001
    ISO27001,
    /// NIST Cybersecurity Framework
    NIST,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReport {
    pub framework: ComplianceFramework,
    pub timestamp: DateTime<Utc>,
    pub overall_score: f64,
    pub controls: Vec<ControlStatus>,
    pub violations: Vec<Violation>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlStatus {
    pub control_id: String,
    pub name: String,
    pub status: ControlState,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ControlState {
    Compliant,
    PartiallyCompliant,
    NonCompliant,
    NotApplicable,
    /// Check has not been implemented or executed yet
    NotChecked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    pub severity: ViolationSeverity,
    pub control_id: String,
    pub description: String,
    pub affected_resources: Vec<String>,
    pub remediation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ViolationSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreatAssessment {
    pub indicator: String,
    pub threat_level: ThreatLevel,
    pub categories: Vec<ThreatCategory>,
    pub sources: Vec<String>,
    pub first_seen: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThreatLevel {
    Clean,
    Suspicious,
    Malicious,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ThreatCategory {
    Malware,
    Phishing,
    C2Server,
    BotNet,
    Tor,
    Scanner,
    Spam,
    MiningPool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityScore {
    pub overall_score: f64, // 0-100
    pub dimensions: HashMap<String, DimensionScore>,
    pub timestamp: DateTime<Utc>,
    pub trend: ScoreTrend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionScore {
    pub score: f64,
    pub weight: f64,
    pub findings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ScoreTrend {
    Improving,
    Stable,
    Declining,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityRecommendation {
    pub priority: Priority,
    pub category: RecommendationCategory,
    pub title: String,
    pub description: String,
    pub impact: String,
    pub effort: Effort,
    pub auto_applicable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    P4Low,
    P3Medium,
    P2High,
    P1Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RecommendationCategory {
    ZeroTrust,
    Compliance,
    ThreatMitigation,
    BestPractice,
    Performance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Effort {
    Low,      // < 1 hour
    Medium,   // 1-4 hours
    High,     // 1-2 days
    VeryHigh, // > 2 days
}

impl Default for SecurityComplianceManager {
    fn default() -> Self {
        match Self::new() {
            Ok(manager) => manager,
            Err(e) => {
                tracing::error!("Failed to create SecurityComplianceManager: {}", e);
                // Return a minimal, non-functional instance rather than panicking
                Self {
                    zero_trust: zero_trust::ZeroTrustEngine::default(),
                    compliance: compliance::ComplianceEngine::default(),
                    threat_intel: threat_intel::ThreatIntelligence::default(),
                    posture: posture::SecurityPosture::default(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manager_creation() {
        let manager = SecurityComplianceManager::new();
        assert!(manager.is_ok());
    }

    #[test]
    fn test_manager_default() {
        let _manager = SecurityComplianceManager::default();
    }

    #[tokio::test]
    async fn test_security_posture_calculation() {
        let mut manager = SecurityComplianceManager::new().unwrap();
        let score = manager.calculate_security_posture().await.unwrap();
        assert!(score.overall_score >= 0.0 && score.overall_score <= 100.0);
    }

    #[tokio::test]
    async fn test_generate_zero_trust_policies() {
        let mut manager = SecurityComplianceManager::new().unwrap();
        let policies = manager
            .generate_zero_trust_policies("test-ns")
            .await
            .unwrap();
        assert!(!policies.is_empty(), "Should generate at least one policy");
        // All policies should be valid YAML-like strings containing the namespace
        for policy in &policies {
            assert!(
                policy.contains("test-ns"),
                "Policy should reference the namespace"
            );
        }
    }

    #[tokio::test]
    async fn test_compliance_audit_pci_dss() {
        let mut manager = SecurityComplianceManager::new().unwrap();
        let report = manager
            .run_compliance_audit(ComplianceFramework::PCIDSS)
            .await
            .unwrap();
        assert_eq!(report.framework, ComplianceFramework::PCIDSS);
        assert!(!report.controls.is_empty());
        // Score depends on cluster availability
        assert!(report.overall_score >= 0.0 && report.overall_score <= 100.0);
    }

    #[tokio::test]
    async fn test_check_threat_intel() {
        let mut manager = SecurityComplianceManager::new().unwrap();
        let assessment = manager.check_threat_intel("192.168.1.1").await.unwrap();
        assert_eq!(assessment.indicator, "192.168.1.1");
        assert_eq!(assessment.threat_level, ThreatLevel::Clean);
        // Private IP should have confidence > 0
        assert!(assessment.confidence > 0.0);
    }

    #[tokio::test]
    async fn test_get_recommendations() {
        let mut manager = SecurityComplianceManager::new().unwrap();
        let recommendations = manager.get_recommendations().await.unwrap();
        assert!(
            !recommendations.is_empty(),
            "Should return at least one recommendation"
        );
        // Recommendations should be sorted by priority (descending)
        for window in recommendations.windows(2) {
            assert!(window[0].priority >= window[1].priority);
        }
    }

    #[test]
    fn test_compliance_framework_equality() {
        assert_eq!(ComplianceFramework::PCIDSS, ComplianceFramework::PCIDSS);
        assert_ne!(ComplianceFramework::SOC2, ComplianceFramework::HIPAA);
    }

    #[test]
    fn test_threat_level_ordering() {
        assert!(ThreatLevel::Critical > ThreatLevel::Malicious);
        assert!(ThreatLevel::Malicious > ThreatLevel::Suspicious);
        assert!(ThreatLevel::Suspicious > ThreatLevel::Clean);
    }
}
