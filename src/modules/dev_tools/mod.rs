// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Developer Experience Tools - Traffic shadowing, replay, debugging
// Experimental: Developer-focused productivity features

pub mod debugger;
pub mod mirror;
pub mod replay;
pub mod shadowing;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Developer tools for testing and debugging
pub struct DevToolsManager {
    shadowing: shadowing::TrafficShadowing,
    replay: replay::RequestReplay,
    mirror: mirror::EnvironmentMirror,
    debugger: debugger::InteractiveDebugger,
}

impl DevToolsManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            shadowing: shadowing::TrafficShadowing::new()?,
            replay: replay::RequestReplay::new()?,
            mirror: mirror::EnvironmentMirror::new()?,
            debugger: debugger::InteractiveDebugger::new()?,
        })
    }

    /// Start shadowing traffic from one service to another
    pub async fn start_shadow(&mut self, config: ShadowConfig) -> Result<String> {
        self.shadowing.start(config).await
    }

    /// Stop traffic shadowing
    pub async fn stop_shadow(&mut self, shadow_id: &str) -> Result<()> {
        self.shadowing.stop(shadow_id).await
    }

    /// Replay recorded requests for debugging
    pub async fn replay_requests(&mut self, config: ReplayConfig) -> Result<ReplayResults> {
        self.replay.replay(config).await
    }

    /// Mirror production environment to local/staging
    pub async fn mirror_environment(&mut self, config: MirrorConfig) -> Result<String> {
        self.mirror.create_mirror(config).await
    }

    /// Start interactive debugging session
    pub async fn start_debug_session(&mut self, config: DebugConfig) -> Result<String> {
        self.debugger.start_session(config).await
    }

    /// Get shadow traffic statistics
    pub async fn get_shadow_stats(&self, shadow_id: &str) -> Result<ShadowStats> {
        self.shadowing.get_stats(shadow_id).await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowConfig {
    pub name: String,
    pub source_service: String,
    pub target_service: String,
    pub namespace: String,
    pub sampling_rate: f64, // 0.0 - 1.0
    pub filters: Vec<TrafficFilter>,
    pub compare_responses: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficFilter {
    pub filter_type: FilterType,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FilterType {
    Path,
    Method,
    Header,
    StatusCode,
    Latency,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowStats {
    pub total_requests: u64,
    pub shadowed_requests: u64,
    pub success_rate: f64,
    pub error_rate: f64,
    pub latency_diff_ms: f64,
    pub discrepancies: Vec<Discrepancy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discrepancy {
    pub request_id: String,
    pub discrepancy_type: DiscrepancyType,
    pub source_value: String,
    pub target_value: String,
    pub impact: Impact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DiscrepancyType {
    StatusCode,
    ResponseBody,
    Headers,
    Latency,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Impact {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayConfig {
    pub recording_file: String,
    pub target_service: String,
    pub namespace: String,
    pub speed_multiplier: f64, // 1.0 = real-time, 2.0 = 2x speed
    pub compare_with_original: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayResults {
    pub total_requests: u64,
    pub successful: u64,
    pub failed: u64,
    pub timing_comparison: Option<TimingComparison>,
    pub differences: Vec<Difference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingComparison {
    pub original_duration_ms: f64,
    pub replay_duration_ms: f64,
    pub difference_percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Difference {
    pub request_id: String,
    pub field: String,
    pub expected: String,
    pub actual: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorConfig {
    pub name: String,
    pub source_namespace: String,
    pub target_namespace: String,
    pub services: Vec<String>,
    pub mirror_type: MirrorType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MirrorType {
    /// Full environment clone
    Full,
    /// Specific services only
    Selective,
    /// Local development mirror
    LocalDev,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugConfig {
    pub service: String,
    pub namespace: String,
    pub debug_mode: DebugMode,
    pub breakpoints: Vec<Breakpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DebugMode {
    /// Intercept and inspect requests
    Intercept,
    /// Trace execution
    Trace,
    /// Performance profiling
    Profile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breakpoint {
    pub condition: String,
    pub action: BreakpointAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BreakpointAction {
    Pause,
    Log,
    ModifyRequest,
    ModifyResponse,
}

impl Default for DevToolsManager {
    fn default() -> Self {
        match Self::new() {
            Ok(manager) => manager,
            Err(e) => {
                tracing::error!("Failed to create DevToolsManager: {}", e);
                // Return a minimal, non-functional instance rather than panicking
                Self {
                    shadowing: shadowing::TrafficShadowing::default(),
                    replay: replay::RequestReplay::default(),
                    mirror: mirror::EnvironmentMirror::default(),
                    debugger: debugger::InteractiveDebugger::default(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_tools_manager_creation() {
        let manager = DevToolsManager::new();
        assert!(manager.is_ok(), "DevToolsManager::new() should succeed");
    }

    #[test]
    fn test_dev_tools_manager_default() {
        let manager = DevToolsManager::default();
        // Default should produce a valid instance without panicking
        let _ = &manager;
    }

    #[tokio::test]
    async fn test_shadow_start_stop_lifecycle() {
        let mut manager = DevToolsManager::new().unwrap();

        let config = ShadowConfig {
            name: "test-shadow".to_string(),
            source_service: "prod-api".to_string(),
            target_service: "canary-api".to_string(),
            namespace: "default".to_string(),
            sampling_rate: 0.1,
            filters: vec![],
            compare_responses: true,
        };

        let shadow_id = manager.start_shadow(config).await.unwrap();
        assert!(!shadow_id.is_empty());

        // Stop should succeed for a valid shadow_id
        let stop_result = manager.stop_shadow(&shadow_id).await;
        assert!(
            stop_result.is_ok(),
            "Stopping a valid shadow should succeed"
        );

        // Stopping again should fail (already removed)
        let stop_again = manager.stop_shadow(&shadow_id).await;
        assert!(
            stop_again.is_err(),
            "Stopping a non-existent shadow should fail"
        );
    }

    #[tokio::test]
    async fn test_shadow_stats_retrieval() {
        let mut manager = DevToolsManager::new().unwrap();

        let config = ShadowConfig {
            name: "stats-test".to_string(),
            source_service: "svc-a".to_string(),
            target_service: "svc-b".to_string(),
            namespace: "default".to_string(),
            sampling_rate: 1.0,
            filters: vec![],
            compare_responses: false,
        };

        let shadow_id = manager.start_shadow(config).await.unwrap();
        let stats = manager.get_shadow_stats(&shadow_id).await.unwrap();
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.shadowed_requests, 0);
    }

    #[test]
    fn test_shadow_config_serialization() {
        let config = ShadowConfig {
            name: "ser-test".to_string(),
            source_service: "src".to_string(),
            target_service: "tgt".to_string(),
            namespace: "ns".to_string(),
            sampling_rate: 0.5,
            filters: vec![TrafficFilter {
                filter_type: FilterType::Path,
                pattern: "/api/*".to_string(),
            }],
            compare_responses: true,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ShadowConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "ser-test");
        assert_eq!(deserialized.filters.len(), 1);
        assert_eq!(deserialized.filters[0].filter_type, FilterType::Path);
    }

    #[test]
    fn test_filter_type_variants() {
        assert_eq!(FilterType::Path, FilterType::Path);
        assert_ne!(FilterType::Path, FilterType::Method);
        assert_ne!(FilterType::Header, FilterType::StatusCode);
    }
}
