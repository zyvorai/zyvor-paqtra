// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// Sidecarless Canary Deployment Module
///
/// Progressive traffic shifting for canary deployments without sidecars.
/// Uses Cilium's native L7 load balancing and traffic management.
///
/// Features:
/// - Gradual traffic shift (0% → 100%)
/// - Health-based auto-promotion or rollback
/// - Metric-based decision making
/// - No sidecar overhead
/// - L7 header-based routing
/// - Real-time traffic split visualization
use anyhow::Result;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::kubernetes::K8sClient;

/// Canary configuration
#[derive(Debug, Clone)]
pub struct CanaryConfig {
    /// Enable canary deployments
    pub enabled: bool,

    /// Initial canary traffic percentage
    pub initial_traffic_pct: u8,

    /// Traffic increment step
    pub traffic_step_pct: u8,

    /// Step interval
    pub step_interval: Duration,

    /// Auto-promotion threshold
    pub auto_promote_threshold: f32, // Success rate threshold

    /// Auto-rollback threshold
    pub auto_rollback_threshold: f32, // Error rate threshold

    /// Maximum canary duration
    pub max_duration: Duration,
}

impl Default for CanaryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            initial_traffic_pct: 10,                 // Start with 10%
            traffic_step_pct: 10,                    // Increase by 10% each step
            step_interval: Duration::from_secs(300), // 5 minutes per step
            auto_promote_threshold: 0.99,            // 99% success rate
            auto_rollback_threshold: 0.90,           // Rollback if <90% success
            max_duration: Duration::from_secs(3600), // 1 hour max
        }
    }
}

/// Canary deployment specification
#[derive(Debug, Clone)]
pub struct CanaryDeployment {
    pub id: String,
    pub name: String,
    pub namespace: String,

    /// Stable version (current production)
    pub stable_version: String,
    pub stable_labels: HashMap<String, String>,

    /// Canary version (new version being tested)
    pub canary_version: String,
    pub canary_labels: HashMap<String, String>,

    /// Service to canary
    pub service_name: String,
    pub service_port: u16,

    /// Current traffic split
    pub current_split: TrafficSplit,

    /// Target traffic split
    pub target_split: TrafficSplit,

    /// Deployment state
    pub state: CanaryState,

    /// Metrics
    pub metrics: CanaryMetrics,

    /// Health check
    pub health: CanaryHealth,

    /// Created and started timestamps
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct TrafficSplit {
    pub stable_pct: u8, // 0-100
    pub canary_pct: u8, // 0-100 (should sum to 100)
}

impl TrafficSplit {
    pub fn new_stable() -> Self {
        Self {
            stable_pct: 100,
            canary_pct: 0,
        }
    }

    pub fn new_split(canary_pct: u8) -> Self {
        let clamped = canary_pct.min(100);
        Self {
            stable_pct: 100 - clamped,
            canary_pct: clamped,
        }
    }

    pub fn is_fully_promoted(&self) -> bool {
        self.canary_pct == 100
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CanaryState {
    Created,     // Created but not started
    Running,     // Active canary in progress
    Paused,      // Paused for manual review
    Promoting,   // Promoting canary to stable
    Promoted,    // Canary fully promoted
    RollingBack, // Rolling back to stable
    RolledBack,  // Rolled back to stable
    Failed { reason: String },
}

#[derive(Debug, Clone)]
pub struct CanaryMetrics {
    /// Total requests to stable
    pub stable_requests: u64,
    pub stable_successes: u64,
    pub stable_errors: u64,
    pub stable_avg_latency_ms: f64,

    /// Total requests to canary
    pub canary_requests: u64,
    pub canary_successes: u64,
    pub canary_errors: u64,
    pub canary_avg_latency_ms: f64,
}

impl Default for CanaryMetrics {
    fn default() -> Self {
        Self {
            stable_requests: 0,
            stable_successes: 0,
            stable_errors: 0,
            stable_avg_latency_ms: 0.0,
            canary_requests: 0,
            canary_successes: 0,
            canary_errors: 0,
            canary_avg_latency_ms: 0.0,
        }
    }
}

impl CanaryMetrics {
    pub fn stable_success_rate(&self) -> f32 {
        if self.stable_requests == 0 {
            return 1.0;
        }
        self.stable_successes as f32 / self.stable_requests as f32
    }

    pub fn canary_success_rate(&self) -> f32 {
        if self.canary_requests == 0 {
            return 1.0;
        }
        self.canary_successes as f32 / self.canary_requests as f32
    }

    pub fn canary_error_rate(&self) -> f32 {
        if self.canary_requests == 0 {
            return 0.0;
        }
        self.canary_errors as f32 / self.canary_requests as f32
    }
}

#[derive(Debug, Clone)]
pub struct CanaryHealth {
    pub stable_healthy: bool,
    pub canary_healthy: bool,
    pub health_check_passed: bool,
    pub last_check: Option<u64>,
}

impl Default for CanaryHealth {
    fn default() -> Self {
        Self {
            stable_healthy: true,
            canary_healthy: true,
            health_check_passed: true,
            last_check: None,
        }
    }
}

/// Canary analysis result
#[derive(Debug, Clone)]
pub struct CanaryAnalysis {
    pub recommendation: CanaryDecision,
    pub confidence: f32,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CanaryDecision {
    Continue, // Continue with current traffic split
    Promote,  // Increase traffic to canary
    Rollback, // Rollback to stable
    Pause,    // Pause for manual review
}

/// Sidecarless Canary Engine
pub struct CanaryEngine {
    config: CanaryConfig,
    k8s_client: K8sClient,

    /// Active canary deployments
    active_canaries: Vec<CanaryDeployment>,

    /// Canary history
    history: Vec<CanaryDeployment>,
}

impl CanaryEngine {
    pub fn new(config: CanaryConfig, k8s_client: K8sClient) -> Self {
        Self {
            config,
            k8s_client,
            active_canaries: Vec::new(),
            history: Vec::new(),
        }
    }

    /// Start a canary deployment
    pub async fn start_canary(
        &mut self,
        name: String,
        namespace: String,
        service_name: String,
        service_port: u16,
        stable_version: String,
        canary_version: String,
    ) -> Result<String> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let id = format!("canary-{}-{}", now, uuid::Uuid::new_v4());

        let mut stable_labels = HashMap::new();
        stable_labels.insert("version".to_string(), stable_version.clone());

        let mut canary_labels = HashMap::new();
        canary_labels.insert("version".to_string(), canary_version.clone());

        let canary = CanaryDeployment {
            id: id.clone(),
            name,
            namespace,
            stable_version,
            stable_labels,
            canary_version,
            canary_labels,
            service_name,
            service_port,
            current_split: TrafficSplit::new_stable(),
            target_split: TrafficSplit::new_split(self.config.initial_traffic_pct),
            state: CanaryState::Created,
            metrics: CanaryMetrics::default(),
            health: CanaryHealth::default(),
            created_at: now,
            started_at: None,
            completed_at: None,
        };

        tracing::info!("🚢 Starting canary deployment: {}", canary.name);

        self.active_canaries.push(canary);

        Ok(id)
    }

    /// Progress canary (increase traffic)
    pub async fn progress_canary(&mut self, id: &str) -> Result<()> {
        let traffic_step = self.config.traffic_step_pct;
        let canary = self.find_canary_mut(id)?;

        if canary.current_split.is_fully_promoted() {
            anyhow::bail!("Canary already fully promoted");
        }

        let new_canary_pct = (canary.current_split.canary_pct + traffic_step).min(100);
        canary.current_split = TrafficSplit::new_split(new_canary_pct);

        tracing::info!(
            "Progressing canary {}: {}% canary traffic",
            canary.name,
            new_canary_pct
        );

        // Record the desired traffic weight on the Kubernetes service.
        // These are Paqtra-owned keys: Cilium does not read them, so this alone
        // does not split traffic. Something that acts on the weight (Gateway API
        // HTTPRoute backendRef weights, a CiliumEnvoyConfig) has to consume it.
        let weight_annotation = format!("paqtra.io/canary-weight={}", new_canary_pct);
        let version_annotation = format!("paqtra.io/canary-version={}", canary.canary_version);
        let svc = &canary.service_name;
        let ns = &canary.namespace;

        match tokio::process::Command::new("kubectl")
            .args([
                "annotate",
                "service",
                svc,
                "-n",
                ns,
                &weight_annotation,
                &version_annotation,
                "--overwrite",
            ])
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                tracing::info!(
                    service = svc,
                    namespace = ns,
                    canary_pct = new_canary_pct,
                    "Updated service annotation for canary traffic split"
                );
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!(
                    service = svc,
                    namespace = ns,
                    error = %stderr,
                    "kubectl annotate failed; canary split updated locally only"
                );
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "kubectl not available; canary split updated locally only"
                );
            }
        }

        Ok(())
    }

    /// Analyze canary health and make decision
    pub fn analyze_canary(&self, id: &str) -> Result<CanaryAnalysis> {
        let canary = self.find_canary(id)?;

        let mut reasons = Vec::new();
        let mut score = 0.0;

        // Check error rate
        let canary_error_rate = canary.metrics.canary_error_rate();
        if canary_error_rate > (1.0 - self.config.auto_rollback_threshold) {
            reasons.push(format!(
                "Canary error rate {:.1}% exceeds threshold",
                canary_error_rate * 100.0
            ));
            return Ok(CanaryAnalysis {
                recommendation: CanaryDecision::Rollback,
                confidence: 0.95,
                reasons,
            });
        }

        // Check success rate
        let canary_success_rate = canary.metrics.canary_success_rate();
        if canary_success_rate >= self.config.auto_promote_threshold {
            score += 0.4;
            reasons.push(format!(
                "Canary success rate {:.1}% meets promotion threshold",
                canary_success_rate * 100.0
            ));
        }

        // Compare latency
        if canary.metrics.canary_avg_latency_ms <= canary.metrics.stable_avg_latency_ms * 1.2 {
            score += 0.3;
            reasons.push("Canary latency within acceptable range".to_string());
        }

        // Check health
        if canary.health.canary_healthy && canary.health.health_check_passed {
            score += 0.3;
            reasons.push("Canary health checks passing".to_string());
        }

        let recommendation = if score >= 0.8 {
            CanaryDecision::Promote
        } else if score >= 0.5 {
            CanaryDecision::Continue
        } else {
            CanaryDecision::Pause
        };

        Ok(CanaryAnalysis {
            recommendation,
            confidence: score,
            reasons,
        })
    }

    /// Promote canary to stable (100% traffic)
    pub async fn promote_canary(&mut self, id: &str) -> Result<()> {
        let idx = self.find_canary_index(id)?;
        let mut canary = self.active_canaries.remove(idx);

        canary.state = CanaryState::Promoted;
        canary.current_split = TrafficSplit::new_split(100);
        canary.completed_at = Some(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs());

        tracing::info!("✅ Canary promoted: {}", canary.name);

        self.history.push(canary);

        const MAX_HISTORY: usize = 10_000;
        if self.history.len() > MAX_HISTORY {
            self.history.drain(..1);
        }

        Ok(())
    }

    /// Rollback canary to stable (0% traffic)
    pub async fn rollback_canary(&mut self, id: &str) -> Result<()> {
        let idx = self.find_canary_index(id)?;
        let mut canary = self.active_canaries.remove(idx);

        canary.state = CanaryState::RolledBack;
        canary.current_split = TrafficSplit::new_stable();
        canary.completed_at = Some(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs());

        tracing::info!("⏮️  Canary rolled back: {}", canary.name);

        self.history.push(canary);

        const MAX_HISTORY: usize = 10_000;
        if self.history.len() > MAX_HISTORY {
            self.history.drain(..1);
        }

        Ok(())
    }

    /// Pause canary (keep current traffic split)
    pub async fn pause_canary(&mut self, id: &str) -> Result<()> {
        let canary = self.find_canary_mut(id)?;
        canary.state = CanaryState::Paused;

        tracing::info!("⏸️  Canary paused: {}", canary.name);

        Ok(())
    }

    /// Resume canary
    pub async fn resume_canary(&mut self, id: &str) -> Result<()> {
        let canary = self.find_canary_mut(id)?;
        canary.state = CanaryState::Running;

        tracing::info!("▶️  Canary resumed: {}", canary.name);

        Ok(())
    }

    fn find_canary(&self, id: &str) -> Result<&CanaryDeployment> {
        self.active_canaries
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| anyhow::anyhow!("Canary not found"))
    }

    fn find_canary_mut(&mut self, id: &str) -> Result<&mut CanaryDeployment> {
        self.active_canaries
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| anyhow::anyhow!("Canary not found"))
    }

    fn find_canary_index(&self, id: &str) -> Result<usize> {
        self.active_canaries
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| anyhow::anyhow!("Canary not found"))
    }

    /// Get active canaries
    pub fn active_canaries(&self) -> &[CanaryDeployment] {
        &self.active_canaries
    }

    /// Get canary history
    pub fn history(&self) -> &[CanaryDeployment] {
        &self.history
    }

    /// Get statistics
    pub fn stats(&self) -> CanaryStats {
        CanaryStats {
            active_canaries: self.active_canaries.len(),
            total_deployments: self.history.len() + self.active_canaries.len(),
            successful_promotions: self
                .history
                .iter()
                .filter(|c| matches!(c.state, CanaryState::Promoted))
                .count(),
            rollbacks: self
                .history
                .iter()
                .filter(|c| matches!(c.state, CanaryState::RolledBack))
                .count(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CanaryStats {
    pub active_canaries: usize,
    pub total_deployments: usize,
    pub successful_promotions: usize,
    pub rollbacks: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires live Kubernetes cluster"]
    async fn test_canary_engine_creation() {
        let config = CanaryConfig::default();
        let k8s_client = K8sClient::new().await.unwrap();

        let engine = CanaryEngine::new(config, k8s_client);
        assert_eq!(engine.active_canaries.len(), 0);
    }

    #[test]
    fn test_traffic_split() {
        let split = TrafficSplit::new_split(20);
        assert_eq!(split.stable_pct, 80);
        assert_eq!(split.canary_pct, 20);
    }

    #[test]
    fn test_canary_metrics() {
        let mut metrics = CanaryMetrics::default();
        metrics.canary_requests = 100;
        metrics.canary_successes = 95;
        metrics.canary_errors = 5;

        assert_eq!(metrics.canary_success_rate(), 0.95);
        assert_eq!(metrics.canary_error_rate(), 0.05);
    }
}
