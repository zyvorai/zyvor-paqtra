// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Request Replay - Replay recorded traffic for debugging
use anyhow::Result;

use super::{ReplayConfig, ReplayResults};

/// Replays recorded HTTP requests
#[derive(Default)]
pub struct RequestReplay {}

impl RequestReplay {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    #[allow(dead_code)]
    pub async fn replay(&self, config: ReplayConfig) -> Result<ReplayResults> {
        if config.recording_file.is_empty() {
            anyhow::bail!("recording_file path cannot be empty");
        }
        if config.target_service.is_empty() {
            anyhow::bail!("target_service cannot be empty");
        }

        tracing::info!(
            recording_file = %config.recording_file,
            target_service = %config.target_service,
            speed = config.speed_multiplier,
            "Starting request replay"
        );

        // Load the recording file
        let recording_path = std::path::Path::new(&config.recording_file);
        if !recording_path.exists() {
            anyhow::bail!("Recording file not found: {}", config.recording_file);
        }

        let content = tokio::fs::read_to_string(&config.recording_file).await?;

        // Parse HAR format
        let har: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse recording file as JSON: {}", e))?;

        // Extract entries from HAR log
        let entries = har
            .get("log")
            .and_then(|log| log.get("entries"))
            .and_then(|e| e.as_array())
            .cloned()
            .unwrap_or_default();

        if entries.is_empty() {
            tracing::warn!("No entries found in recording file");
            return Ok(ReplayResults {
                total_requests: 0,
                successful: 0,
                failed: 0,
                timing_comparison: None,
                differences: vec![],
            });
        }

        let total_requests = entries.len() as u64;
        let mut successful = 0u64;
        let mut failed = 0u64;
        let mut differences = Vec::new();
        let mut original_total_ms = 0.0f64;
        let mut replay_total_ms = 0.0f64;

        // Resolve target service address
        let target_base =
            Self::resolve_service_url(&config.target_service, &config.namespace).await;

        for (idx, entry) in entries.iter().enumerate() {
            let request = match entry.get("request") {
                Some(r) => r,
                None => continue,
            };

            let method = request
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("GET");
            let url = request.get("url").and_then(|u| u.as_str()).unwrap_or("");

            // Extract path from original URL and build target URL
            let path = url
                .find("://")
                .and_then(|i| url[i + 3..].find('/'))
                .map(|i| &url[url.find("://").unwrap() + 3 + i..])
                .unwrap_or("/");

            let target_url = format!("{}{}", target_base, path);

            // Record original timing
            let orig_time = entry.get("time").and_then(|t| t.as_f64()).unwrap_or(0.0);
            original_total_ms += orig_time;

            // Apply speed multiplier delay
            if config.speed_multiplier > 0.0 && config.speed_multiplier < 100.0 && idx > 0 {
                let delay_ms = (orig_time / config.speed_multiplier) as u64;
                if delay_ms > 0 && delay_ms < 10000 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                }
            }

            // Replay the request
            let start = std::time::Instant::now();
            let replay_result = tokio::process::Command::new("curl")
                .args([
                    "-s",
                    "-o",
                    "/dev/null",
                    "-w",
                    "%{http_code}",
                    "-X",
                    method,
                    &target_url,
                ])
                .output()
                .await;

            let elapsed_ms = start.elapsed().as_millis() as f64;
            replay_total_ms += elapsed_ms;

            match replay_result {
                Ok(output) if output.status.success() => {
                    let status_code = String::from_utf8_lossy(&output.stdout);
                    let status: u16 = status_code.trim().parse().unwrap_or(0);

                    if (200..400).contains(&status) {
                        successful += 1;
                    } else {
                        failed += 1;
                    }

                    // Compare with original response if enabled
                    if config.compare_with_original {
                        let orig_status = entry
                            .get("response")
                            .and_then(|r| r.get("status"))
                            .and_then(|s| s.as_u64())
                            .unwrap_or(0) as u16;

                        if status != orig_status {
                            differences.push(super::Difference {
                                request_id: format!("req-{}", idx),
                                field: "status_code".to_string(),
                                expected: orig_status.to_string(),
                                actual: status.to_string(),
                            });
                        }
                    }
                }
                _ => {
                    failed += 1;
                }
            }
        }

        let timing_comparison = if original_total_ms > 0.0 {
            Some(super::TimingComparison {
                original_duration_ms: original_total_ms,
                replay_duration_ms: replay_total_ms,
                difference_percentage: ((replay_total_ms - original_total_ms) / original_total_ms)
                    * 100.0,
            })
        } else {
            None
        };

        tracing::info!(
            total = total_requests,
            successful,
            failed,
            "Request replay completed"
        );

        Ok(ReplayResults {
            total_requests,
            successful,
            failed,
            timing_comparison,
            differences,
        })
    }

    /// Resolve a Kubernetes service to a URL
    async fn resolve_service_url(service: &str, namespace: &str) -> String {
        // Try kubectl to get the service ClusterIP
        if let Ok(output) = tokio::process::Command::new("kubectl")
            .args([
                "get",
                "service",
                service,
                "-n",
                namespace,
                "-o",
                "jsonpath={.spec.clusterIP}:{.spec.ports[0].port}",
            ])
            .output()
            .await
        {
            if output.status.success() {
                let addr = String::from_utf8_lossy(&output.stdout);
                let addr = addr.trim();
                if !addr.is_empty() && !addr.starts_with(':') {
                    return format!("http://{}", addr);
                }
            }
        }

        // Fallback to DNS-based service discovery
        format!("http://{}.{}.svc.cluster.local", service, namespace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::dev_tools::ReplayConfig;

    #[test]
    fn test_request_replay_creation() {
        let replay = RequestReplay::new();
        assert!(replay.is_ok());
    }

    #[test]
    fn test_request_replay_default() {
        let _replay = RequestReplay::default();
        // Should not panic
    }

    #[tokio::test]
    async fn test_replay_empty_recording_file_fails() {
        let replay = RequestReplay::new().unwrap();
        let config = ReplayConfig {
            recording_file: String::new(),
            target_service: "svc-a".to_string(),
            namespace: "default".to_string(),
            speed_multiplier: 1.0,
            compare_with_original: false,
        };
        let result = replay.replay(config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("recording_file"));
    }

    #[tokio::test]
    async fn test_replay_empty_target_service_fails() {
        let replay = RequestReplay::new().unwrap();
        let config = ReplayConfig {
            recording_file: "/tmp/recording.har".to_string(),
            target_service: String::new(),
            namespace: "default".to_string(),
            speed_multiplier: 1.0,
            compare_with_original: false,
        };
        let result = replay.replay(config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("target_service"));
    }

    #[tokio::test]
    async fn test_replay_nonexistent_file_fails() {
        let replay = RequestReplay::new().unwrap();
        let config = ReplayConfig {
            recording_file: "/tmp/nonexistent-recording.har".to_string(),
            target_service: "svc-a".to_string(),
            namespace: "default".to_string(),
            speed_multiplier: 2.0,
            compare_with_original: true,
        };
        let result = replay.replay(config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_replay_valid_har_file() {
        let replay = RequestReplay::new().unwrap();

        // Create a minimal HAR file for testing
        let har = serde_json::json!({
            "log": {
                "entries": []
            }
        });
        let tmp_path = "/tmp/test-replay-empty.har";
        std::fs::write(tmp_path, serde_json::to_string(&har).unwrap()).unwrap();

        let config = ReplayConfig {
            recording_file: tmp_path.to_string(),
            target_service: "svc-a".to_string(),
            namespace: "default".to_string(),
            speed_multiplier: 1.0,
            compare_with_original: false,
        };
        let result = replay.replay(config).await;
        assert!(result.is_ok());
        let results = result.unwrap();
        assert_eq!(results.total_requests, 0);

        let _ = std::fs::remove_file(tmp_path);
    }
}
