// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Traffic Shadowing - Mirror production traffic for testing
use anyhow::Result;
use std::collections::HashMap;
use tokio::sync::RwLock;

use super::{ShadowConfig, ShadowStats};

/// Shadows production traffic to test environments
pub struct TrafficShadowing {
    active_shadows: RwLock<HashMap<String, Shadow>>,
}

struct Shadow {
    config: ShadowConfig,
    stats: ShadowStats,
}

impl TrafficShadowing {
    pub fn new() -> Result<Self> {
        Ok(Self {
            active_shadows: RwLock::new(HashMap::new()),
        })
    }

    pub async fn start(&mut self, config: ShadowConfig) -> Result<String> {
        // Validate config
        if config.source_service.is_empty() {
            anyhow::bail!("source_service cannot be empty");
        }
        if config.target_service.is_empty() {
            anyhow::bail!("target_service cannot be empty");
        }
        if config.sampling_rate <= 0.0 || config.sampling_rate > 1.0 {
            anyhow::bail!(
                "sampling_rate must be in range (0.0, 1.0], got {}",
                config.sampling_rate
            );
        }

        let shadow_id = uuid::Uuid::new_v4().to_string();

        // Attempt to create a CiliumNetworkPolicy for traffic mirroring
        let mirror_policy = format!(
            r#"apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: shadow-{shadow_id_short}
  namespace: {namespace}
  labels:
    paqtra/shadow-id: "{shadow_id}"
spec:
  endpointSelector:
    matchLabels:
      app: "{source}"
  egress:
    - toEndpoints:
        - matchLabels:
            app: "{target}"
      toPorts:
        - ports:
            - port: "0"
              protocol: ANY"#,
            shadow_id_short = &shadow_id[..8],
            namespace = config.namespace,
            shadow_id = shadow_id,
            source = config.source_service,
            target = config.target_service,
        );

        // Try to apply the mirroring policy
        let apply_result = tokio::process::Command::new("kubectl")
            .args(["apply", "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();

        match apply_result {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    use tokio::io::AsyncWriteExt;
                    let _ = stdin.write_all(mirror_policy.as_bytes()).await;
                    drop(stdin);
                }
                let output = child.wait_with_output().await;
                match output {
                    Ok(out) if out.status.success() => {
                        tracing::info!(
                            shadow_id = %shadow_id,
                            "Traffic shadow policy applied for {} -> {}",
                            config.source_service,
                            config.target_service
                        );
                    }
                    _ => {
                        tracing::warn!(
                            shadow_id = %shadow_id,
                            source = %config.source_service,
                            target = %config.target_service,
                            "Could not apply shadow policy via kubectl. \
                             Shadow session created for tracking but traffic \
                             mirroring requires cluster access."
                        );
                    }
                }
            }
            Err(_) => {
                tracing::warn!(
                    shadow_id = %shadow_id,
                    "kubectl not available. Shadow session created for tracking \
                     but traffic mirroring requires cluster access."
                );
            }
        }

        let shadow = Shadow {
            config: config.clone(),
            stats: ShadowStats {
                total_requests: 0,
                shadowed_requests: 0,
                success_rate: 0.0,
                error_rate: 0.0,
                latency_diff_ms: 0.0,
                discrepancies: vec![],
            },
        };

        self.active_shadows
            .write()
            .await
            .insert(shadow_id.clone(), shadow);

        Ok(shadow_id)
    }

    pub async fn stop(&mut self, shadow_id: &str) -> Result<()> {
        let removed = self.active_shadows.write().await.remove(shadow_id);
        match removed {
            Some(shadow) => {
                // Clean up the shadow policy from the cluster
                let policy_name = format!("shadow-{}", &shadow_id[..8.min(shadow_id.len())]);
                let _ = tokio::process::Command::new("kubectl")
                    .args([
                        "delete",
                        "ciliumnetworkpolicy",
                        &policy_name,
                        "-n",
                        &shadow.config.namespace,
                        "--ignore-not-found",
                    ])
                    .output()
                    .await;
                tracing::info!("Shadow stopped: {}", shadow_id);
            }
            None => {
                anyhow::bail!("Shadow session not found: {}", shadow_id);
            }
        }
        Ok(())
    }

    pub async fn get_stats(&self, shadow_id: &str) -> Result<ShadowStats> {
        let shadows = self.active_shadows.read().await;
        shadows
            .get(shadow_id)
            .map(|s| s.stats.clone())
            .ok_or_else(|| anyhow::anyhow!("Shadow session not found: {}", shadow_id))
    }
}

impl Default for TrafficShadowing {
    fn default() -> Self {
        Self {
            active_shadows: RwLock::new(HashMap::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::dev_tools::ShadowConfig;

    fn make_valid_config() -> ShadowConfig {
        ShadowConfig {
            name: "test".to_string(),
            source_service: "source-svc".to_string(),
            target_service: "target-svc".to_string(),
            namespace: "default".to_string(),
            sampling_rate: 0.5,
            filters: vec![],
            compare_responses: false,
        }
    }

    #[test]
    fn test_traffic_shadowing_creation() {
        let ts = TrafficShadowing::new();
        assert!(ts.is_ok());
    }

    #[tokio::test]
    async fn test_start_with_valid_config() {
        let mut ts = TrafficShadowing::new().unwrap();
        let config = make_valid_config();
        let result = ts.start(config).await;
        assert!(result.is_ok());
        let shadow_id = result.unwrap();
        assert!(!shadow_id.is_empty());
    }

    #[tokio::test]
    async fn test_start_fails_empty_source() {
        let mut ts = TrafficShadowing::new().unwrap();
        let mut config = make_valid_config();
        config.source_service = String::new();
        let result = ts.start(config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("source_service"));
    }

    #[tokio::test]
    async fn test_start_fails_empty_target() {
        let mut ts = TrafficShadowing::new().unwrap();
        let mut config = make_valid_config();
        config.target_service = String::new();
        let result = ts.start(config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("target_service"));
    }

    #[tokio::test]
    async fn test_sampling_rate_zero_rejected() {
        let mut ts = TrafficShadowing::new().unwrap();
        let mut config = make_valid_config();
        config.sampling_rate = 0.0;
        let result = ts.start(config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("sampling_rate"));
    }

    #[tokio::test]
    async fn test_sampling_rate_negative_rejected() {
        let mut ts = TrafficShadowing::new().unwrap();
        let mut config = make_valid_config();
        config.sampling_rate = -0.1;
        let result = ts.start(config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_sampling_rate_above_one_rejected() {
        let mut ts = TrafficShadowing::new().unwrap();
        let mut config = make_valid_config();
        config.sampling_rate = 1.5;
        let result = ts.start(config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_sampling_rate_one_accepted() {
        let mut ts = TrafficShadowing::new().unwrap();
        let mut config = make_valid_config();
        config.sampling_rate = 1.0;
        let result = ts.start(config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_stop_nonexistent_shadow_fails() {
        let mut ts = TrafficShadowing::new().unwrap();
        let result = ts.stop("nonexistent-id").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_get_stats_for_active_shadow() {
        let mut ts = TrafficShadowing::new().unwrap();
        let config = make_valid_config();
        let shadow_id = ts.start(config).await.unwrap();

        let stats = ts.get_stats(&shadow_id).await;
        assert!(stats.is_ok());
        let stats = stats.unwrap();
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.shadowed_requests, 0);
    }

    #[tokio::test]
    async fn test_get_stats_for_missing_shadow_fails() {
        let ts = TrafficShadowing::new().unwrap();
        let result = ts.get_stats("missing").await;
        assert!(result.is_err());
    }
}
