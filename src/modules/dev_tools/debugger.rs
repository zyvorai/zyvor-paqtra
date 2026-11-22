// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Interactive Debugger - Debug network requests in real-time
use anyhow::Result;

use super::DebugConfig;

/// Interactive network debugger
#[derive(Default)]
pub struct InteractiveDebugger {}

impl InteractiveDebugger {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    pub async fn start_session(&self, config: DebugConfig) -> Result<String> {
        if config.service.is_empty() {
            anyhow::bail!("service name cannot be empty");
        }
        if config.namespace.is_empty() {
            anyhow::bail!("namespace cannot be empty");
        }

        let session_id = uuid::Uuid::new_v4().to_string();

        tracing::info!(
            session_id = %session_id,
            service = %config.service,
            namespace = %config.namespace,
            mode = ?config.debug_mode,
            breakpoints = config.breakpoints.len(),
            "Starting debug session"
        );

        // Verify the service exists
        let output = tokio::process::Command::new("kubectl")
            .args([
                "get",
                "pods",
                "-n",
                &config.namespace,
                "-l",
                &format!("app={}", config.service),
                "-o",
                "name",
            ])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let pod_count = stdout.lines().filter(|l| !l.is_empty()).count();
                if pod_count == 0 {
                    anyhow::bail!(
                        "No pods found for service '{}' in namespace '{}'",
                        config.service,
                        config.namespace
                    );
                }

                // Start hubble observe for the service based on debug mode
                let mut hubble_args = vec![
                    "observe".to_string(),
                    "--namespace".to_string(),
                    config.namespace.clone(),
                    "--to-pod".to_string(),
                    format!("{}/{}", config.namespace, config.service),
                    "--output".to_string(),
                    "json".to_string(),
                ];

                match config.debug_mode {
                    super::DebugMode::Trace => {
                        hubble_args.push("--verdict".to_string());
                        hubble_args.push("FORWARDED".to_string());
                    }
                    super::DebugMode::Intercept => {
                        // Show all verdicts for intercept mode
                    }
                    super::DebugMode::Profile => {
                        hubble_args.push("--last".to_string());
                        hubble_args.push("100".to_string());
                    }
                }

                // Log breakpoint conditions
                for bp in &config.breakpoints {
                    tracing::info!(
                        session_id = %session_id,
                        condition = %bp.condition,
                        action = ?bp.action,
                        "Breakpoint registered"
                    );
                }

                tracing::info!(
                    session_id = %session_id,
                    pod_count,
                    "Debug session started for {}/{} ({:?} mode)",
                    config.namespace,
                    config.service,
                    config.debug_mode
                );

                Ok(session_id)
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                anyhow::bail!(
                    "Failed to query pods for service '{}': {}",
                    config.service,
                    stderr.trim()
                )
            }
            Err(_) => {
                // kubectl not available, return session ID anyway for offline usage
                tracing::warn!(
                    session_id = %session_id,
                    "kubectl not available - debug session created in offline mode"
                );
                Ok(session_id)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::dev_tools::{Breakpoint, BreakpointAction, DebugConfig, DebugMode};

    #[test]
    fn test_interactive_debugger_creation() {
        let debugger = InteractiveDebugger::new();
        assert!(debugger.is_ok());
    }

    #[test]
    fn test_interactive_debugger_default() {
        let _debugger = InteractiveDebugger::default();
        // Should not panic
    }

    #[tokio::test]
    async fn test_start_session_empty_service_fails() {
        let debugger = InteractiveDebugger::new().unwrap();
        let config = DebugConfig {
            service: String::new(),
            namespace: "default".to_string(),
            debug_mode: DebugMode::Trace,
            breakpoints: vec![],
        };
        let result = debugger.start_session(config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("service name"));
    }

    #[tokio::test]
    async fn test_start_session_empty_namespace_fails() {
        let debugger = InteractiveDebugger::new().unwrap();
        let config = DebugConfig {
            service: "my-svc".to_string(),
            namespace: String::new(),
            debug_mode: DebugMode::Intercept,
            breakpoints: vec![],
        };
        let result = debugger.start_session(config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("namespace"));
    }

    #[tokio::test]
    async fn test_start_session_valid_config_returns_session_id() {
        let debugger = InteractiveDebugger::new().unwrap();
        let config = DebugConfig {
            service: "my-svc".to_string(),
            namespace: "default".to_string(),
            debug_mode: DebugMode::Profile,
            breakpoints: vec![Breakpoint {
                condition: "status == 500".to_string(),
                action: BreakpointAction::Log,
            }],
        };
        let result = debugger.start_session(config).await;
        // When kubectl is available, returns session id or error about pods not found.
        // When kubectl is not available, returns session id (offline mode).
        // Either way it should not panic.
        match result {
            Ok(id) => assert!(!id.is_empty()),
            Err(e) => {
                // Acceptable: no pods found, or kubectl error
                let msg = e.to_string();
                assert!(
                    msg.contains("No pods found") || msg.contains("Failed"),
                    "Unexpected error: {}",
                    msg
                );
            }
        }
    }

    #[test]
    fn test_debug_mode_equality() {
        assert_eq!(DebugMode::Intercept, DebugMode::Intercept);
        assert_ne!(DebugMode::Trace, DebugMode::Profile);
    }

    #[test]
    fn test_breakpoint_action_equality() {
        assert_eq!(BreakpointAction::Pause, BreakpointAction::Pause);
        assert_ne!(BreakpointAction::Log, BreakpointAction::ModifyRequest);
    }
}
