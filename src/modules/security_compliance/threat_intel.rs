// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Threat Intelligence Integration
use anyhow::Result;

use super::{
    Effort, Priority, RecommendationCategory, SecurityRecommendation, ThreatAssessment,
    ThreatCategory, ThreatLevel,
};

/// Integrates with threat intelligence feeds
#[derive(Default)]
pub struct ThreatIntelligence {
    feeds_enabled: bool,
}

impl ThreatIntelligence {
    pub fn new() -> Result<Self> {
        Ok(Self {
            feeds_enabled: false, // No feeds are actually configured
        })
    }

    pub async fn assess(&self, indicator: &str) -> Result<ThreatAssessment> {
        tracing::debug!("Assessing threat indicator: {}", indicator);

        let mut threat_level = ThreatLevel::Clean;
        let mut categories = Vec::new();
        let mut sources = Vec::new();
        let mut confidence;

        // Determine indicator type and run appropriate checks
        if let Ok(ip) = indicator.parse::<std::net::IpAddr>() {
            // IP address analysis
            let result = self.assess_ip(&ip);
            threat_level = result.0;
            categories = result.1;
            sources = result.2;
            confidence = result.3;
        } else if indicator.contains('.') && !indicator.contains('/') {
            // Domain analysis
            let result = self.assess_domain(indicator);
            threat_level = result.0;
            categories = result.1;
            sources = result.2;
            confidence = result.3;
        } else {
            sources.push("unknown indicator format".to_string());
            confidence = 0.1;
        }

        // If external feeds are configured, note that for higher confidence
        if self.feeds_enabled {
            sources.push("external feeds consulted".to_string());
            confidence = (confidence + 0.3).min(1.0);
        } else {
            sources.push("local heuristic analysis only (no external feeds)".to_string());
        }

        Ok(ThreatAssessment {
            indicator: indicator.to_string(),
            threat_level,
            categories,
            sources,
            first_seen: None,
            last_seen: None,
            confidence,
        })
    }

    /// Assess an IP address for known threat patterns
    fn assess_ip(
        &self,
        ip: &std::net::IpAddr,
    ) -> (ThreatLevel, Vec<ThreatCategory>, Vec<String>, f64) {
        let mut categories = Vec::new();
        let mut sources = Vec::new();
        let mut confidence = 0.5; // Base confidence for heuristic analysis

        match ip {
            std::net::IpAddr::V4(v4) => {
                let octets = v4.octets();

                // RFC1918 private addresses are clean
                if v4.is_private() || v4.is_loopback() || v4.is_link_local() {
                    sources.push("RFC1918/loopback/link-local address".to_string());
                    return (ThreatLevel::Clean, categories, sources, 0.9);
                }

                // Check for known bogon/reserved ranges
                if v4.is_broadcast() || v4.is_unspecified() || octets[0] == 0 || octets[0] == 127 {
                    sources.push("reserved/bogon address".to_string());
                    return (ThreatLevel::Suspicious, vec![], sources, 0.7);
                }

                // Known Tor exit node ranges (common patterns)
                // These are heuristic checks, not exhaustive
                if octets[0] == 185 && octets[1] == 220 {
                    categories.push(ThreatCategory::Tor);
                    sources.push("IP in range commonly associated with Tor exit nodes".to_string());
                    return (ThreatLevel::Suspicious, categories, sources, 0.4);
                }

                // Known mining pool port patterns (stratum)
                sources.push("public IP - no known threat patterns matched".to_string());
            }
            std::net::IpAddr::V6(v6) => {
                if v6.is_loopback() || v6.is_unspecified() {
                    sources.push("IPv6 loopback/unspecified".to_string());
                    return (ThreatLevel::Clean, categories, sources, 0.9);
                }
                sources.push("IPv6 address - heuristic analysis".to_string());
                confidence = 0.3;
            }
        }

        (ThreatLevel::Clean, categories, sources, confidence)
    }

    /// Assess a domain for known threat patterns
    fn assess_domain(&self, domain: &str) -> (ThreatLevel, Vec<ThreatCategory>, Vec<String>, f64) {
        let mut categories = Vec::new();
        let mut sources = Vec::new();
        let lower = domain.to_lowercase();

        // Check for suspicious TLDs commonly used in attacks
        let suspicious_tlds = [".tk", ".ml", ".ga", ".cf", ".gq", ".xyz", ".top", ".buzz"];
        for tld in &suspicious_tlds {
            if lower.ends_with(tld) {
                sources.push(format!(
                    "Domain uses {} TLD (commonly abused for phishing/malware)",
                    tld
                ));
                categories.push(ThreatCategory::Phishing);
                return (ThreatLevel::Suspicious, categories, sources, 0.4);
            }
        }

        // Check for known malicious domain patterns
        if lower.contains("malware")
            || lower.contains("exploit")
            || lower.contains("phish")
            || lower.contains("c2-")
            || lower.contains("botnet")
        {
            categories.push(ThreatCategory::Malware);
            sources.push("Domain name contains known malicious keywords".to_string());
            return (ThreatLevel::Malicious, categories, sources, 0.6);
        }

        // Check for homograph attack patterns (mixed script)
        if !lower.is_ascii() {
            sources.push(
                "Domain contains non-ASCII characters (possible homograph attack)".to_string(),
            );
            categories.push(ThreatCategory::Phishing);
            return (ThreatLevel::Suspicious, categories, sources, 0.5);
        }

        // Check excessive subdomain depth (DNS tunneling indicator)
        let dot_count = lower.chars().filter(|c| *c == '.').count();
        if dot_count > 4 {
            sources.push(format!(
                "Excessive subdomain depth ({} levels) - possible DNS tunneling",
                dot_count
            ));
            categories.push(ThreatCategory::C2Server);
            return (ThreatLevel::Suspicious, categories, sources, 0.3);
        }

        // Check for very long labels (DNS tunneling indicator)
        for label in lower.split('.') {
            if label.len() > 40 {
                sources.push(
                    "Domain label exceeds 40 chars - possible DNS tunneling/exfiltration"
                        .to_string(),
                );
                categories.push(ThreatCategory::C2Server);
                return (ThreatLevel::Suspicious, categories, sources, 0.3);
            }
        }

        sources.push("No known threat patterns matched".to_string());
        (ThreatLevel::Clean, categories, sources, 0.5)
    }

    pub async fn get_recommendations(&self) -> Result<Vec<SecurityRecommendation>> {
        Ok(vec![SecurityRecommendation {
            priority: Priority::P3Medium,
            category: RecommendationCategory::ThreatMitigation,
            title: "Enable threat intelligence feeds".to_string(),
            description: "Integrate with external threat feeds for real-time protection"
                .to_string(),
            impact: "Proactive threat detection".to_string(),
            effort: Effort::Medium,
            auto_applicable: false,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_threat_intelligence_creation() {
        let ti = ThreatIntelligence::new();
        assert!(ti.is_ok());
    }

    #[test]
    fn test_threat_intelligence_default() {
        let ti = ThreatIntelligence::default();
        assert!(!ti.feeds_enabled);
    }

    #[tokio::test]
    async fn test_assess_returns_clean_without_feeds() {
        let ti = ThreatIntelligence::new().unwrap();
        let assessment = ti.assess("192.168.1.1").await.unwrap();
        assert_eq!(assessment.indicator, "192.168.1.1");
        assert_eq!(assessment.threat_level, ThreatLevel::Clean);
        // Private IP should have high confidence
        assert!(assessment.confidence > 0.0);
        assert!(assessment.categories.is_empty());
        assert!(assessment.first_seen.is_none());
        assert!(assessment.last_seen.is_none());
    }

    #[tokio::test]
    async fn test_assess_domain_with_malicious_keyword() {
        let ti = ThreatIntelligence::new().unwrap();
        let assessment = ti.assess("malware.example.com").await.unwrap();
        assert_eq!(assessment.indicator, "malware.example.com");
        assert_eq!(assessment.threat_level, ThreatLevel::Malicious);
        assert!(!assessment.categories.is_empty());
    }

    #[tokio::test]
    async fn test_assess_clean_domain() {
        let ti = ThreatIntelligence::new().unwrap();
        let assessment = ti.assess("example.com").await.unwrap();
        assert_eq!(assessment.indicator, "example.com");
        assert_eq!(assessment.threat_level, ThreatLevel::Clean);
    }

    #[tokio::test]
    async fn test_assess_sources_populated() {
        let ti = ThreatIntelligence::new().unwrap();
        let assessment = ti.assess("10.0.0.1").await.unwrap();
        assert!(!assessment.sources.is_empty());
        // Should mention local heuristic analysis
        assert!(
            assessment
                .sources
                .iter()
                .any(|s| s.contains("heuristic") || s.contains("RFC1918")),
            "Sources should describe analysis method, got: {:?}",
            assessment.sources
        );
    }

    #[tokio::test]
    async fn test_get_recommendations() {
        let ti = ThreatIntelligence::new().unwrap();
        let recs = ti.get_recommendations().await.unwrap();
        assert!(!recs.is_empty());
        assert_eq!(recs[0].category, RecommendationCategory::ThreatMitigation);
        assert_eq!(recs[0].priority, Priority::P3Medium);
    }
}
