// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Environment Mirroring - Clone environments for testing
use anyhow::Result;

use super::MirrorConfig;

/// Mirrors Kubernetes environments
#[derive(Default)]
pub struct EnvironmentMirror {}

impl EnvironmentMirror {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    pub async fn create_mirror(&self, config: MirrorConfig) -> Result<String> {
        if config.source_namespace.is_empty() {
            anyhow::bail!("source_namespace cannot be empty");
        }
        if config.target_namespace.is_empty() {
            anyhow::bail!("target_namespace cannot be empty");
        }
        if config.source_namespace == config.target_namespace {
            anyhow::bail!(
                "source_namespace and target_namespace must differ, both are '{}'",
                config.source_namespace
            );
        }

        let mirror_id = uuid::Uuid::new_v4().to_string();

        tracing::info!(
            mirror_id = %mirror_id,
            source = %config.source_namespace,
            target = %config.target_namespace,
            mode = ?config.mirror_type,
            "Creating environment mirror"
        );

        // Create target namespace if it doesn't exist
        let ns_output = tokio::process::Command::new("kubectl")
            .args(["create", "namespace", &config.target_namespace])
            .output()
            .await;

        match ns_output {
            Ok(out) => {
                if out.status.success() {
                    tracing::info!("Created target namespace: {}", config.target_namespace);
                } else {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    if !stderr.contains("already exists") {
                        anyhow::bail!(
                            "Failed to create target namespace '{}': {}",
                            config.target_namespace,
                            stderr.trim()
                        );
                    }
                }
            }
            Err(_) => anyhow::bail!("kubectl not available - cannot create mirror"),
        }

        // Determine which resources to mirror
        let resource_types = match config.mirror_type {
            super::MirrorType::Full => vec!["deployments", "services", "configmaps"],
            super::MirrorType::Selective => vec!["deployments", "services"],
            super::MirrorType::LocalDev => vec!["configmaps", "services"],
        };

        for resource_type in &resource_types {
            // Get resources from source namespace
            let mut get_args = vec![
                "get".to_string(),
                resource_type.to_string(),
                "-n".to_string(),
                config.source_namespace.clone(),
                "-o".to_string(),
                "json".to_string(),
            ];

            // Filter by service names if selective
            if !config.services.is_empty() && *resource_type != "configmaps" {
                let label_selector = config
                    .services
                    .iter()
                    .map(|s| format!("app={}", s))
                    .collect::<Vec<_>>()
                    .join(",");
                get_args.push("-l".to_string());
                get_args.push(label_selector);
            }

            let output = tokio::process::Command::new("kubectl")
                .args(&get_args)
                .output()
                .await?;

            if output.status.success() {
                let json_str = String::from_utf8_lossy(&output.stdout);
                if let Ok(mut resources) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    // Modify namespace in each resource and apply to target
                    if let Some(items) = resources.get_mut("items").and_then(|i| i.as_array_mut()) {
                        for item in items {
                            // Update namespace
                            if let Some(metadata) = item.get_mut("metadata") {
                                metadata["namespace"] =
                                    serde_json::Value::String(config.target_namespace.clone());
                                // Remove resource-specific fields that shouldn't be copied
                                if let Some(obj) = metadata.as_object_mut() {
                                    obj.remove("resourceVersion");
                                    obj.remove("uid");
                                    obj.remove("creationTimestamp");
                                }
                            }
                            // Remove status
                            if let Some(obj) = item.as_object_mut() {
                                obj.remove("status");
                            }

                            // Apply to target namespace
                            let item_json = serde_json::to_string(&item)?;
                            let apply_output = tokio::process::Command::new("kubectl")
                                .args(["apply", "-f", "-", "-n", &config.target_namespace])
                                .stdin(std::process::Stdio::piped())
                                .stdout(std::process::Stdio::piped())
                                .stderr(std::process::Stdio::piped())
                                .spawn();

                            if let Ok(mut child) = apply_output {
                                if let Some(mut stdin) = child.stdin.take() {
                                    use tokio::io::AsyncWriteExt;
                                    let _ = stdin.write_all(item_json.as_bytes()).await;
                                    drop(stdin);
                                }
                                let _ = child.wait().await;
                            }
                        }
                    }
                }
            }

            tracing::info!(
                "Mirrored {} from {} to {}",
                resource_type,
                config.source_namespace,
                config.target_namespace
            );
        }

        tracing::info!(
            mirror_id = %mirror_id,
            "Environment mirror created: {} -> {}",
            config.source_namespace,
            config.target_namespace
        );

        Ok(mirror_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::dev_tools::{MirrorConfig, MirrorType};

    #[test]
    fn test_environment_mirror_creation() {
        let mirror = EnvironmentMirror::new();
        assert!(mirror.is_ok());
    }

    #[test]
    fn test_environment_mirror_default() {
        let _mirror = EnvironmentMirror::default();
        // Should not panic
    }

    #[tokio::test]
    async fn test_create_mirror_empty_source_namespace() {
        let mirror = EnvironmentMirror::new().unwrap();
        let config = MirrorConfig {
            name: "test".to_string(),
            source_namespace: String::new(),
            target_namespace: "staging".to_string(),
            services: vec![],
            mirror_type: MirrorType::Full,
        };
        let result = mirror.create_mirror(config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("source_namespace"));
    }

    #[tokio::test]
    async fn test_create_mirror_empty_target_namespace() {
        let mirror = EnvironmentMirror::new().unwrap();
        let config = MirrorConfig {
            name: "test".to_string(),
            source_namespace: "production".to_string(),
            target_namespace: String::new(),
            services: vec![],
            mirror_type: MirrorType::Full,
        };
        let result = mirror.create_mirror(config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("target_namespace"));
    }

    #[tokio::test]
    async fn test_create_mirror_same_namespaces_rejected() {
        let mirror = EnvironmentMirror::new().unwrap();
        let config = MirrorConfig {
            name: "test".to_string(),
            source_namespace: "production".to_string(),
            target_namespace: "production".to_string(),
            services: vec![],
            mirror_type: MirrorType::Full,
        };
        let result = mirror.create_mirror(config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must differ"));
    }

    #[ignore = "needs a Kubernetes cluster"]
    #[tokio::test]
    async fn test_create_mirror_valid_config() {
        let mirror = EnvironmentMirror::new().unwrap();
        let config = MirrorConfig {
            name: "test".to_string(),
            source_namespace: "production".to_string(),
            target_namespace: "staging".to_string(),
            services: vec!["svc-a".to_string()],
            mirror_type: MirrorType::Selective,
        };
        let result = mirror.create_mirror(config).await;
        // When kubectl is available, returns mirror id.
        // When kubectl is not available, returns error about kubectl.
        match result {
            Ok(id) => assert!(!id.is_empty()),
            Err(e) => assert!(e.to_string().contains("kubectl"), "Unexpected error: {}", e),
        }
    }

    #[test]
    fn test_mirror_type_equality() {
        assert_eq!(MirrorType::Full, MirrorType::Full);
        assert_ne!(MirrorType::Full, MirrorType::Selective);
        assert_ne!(MirrorType::Selective, MirrorType::LocalDev);
    }
}
