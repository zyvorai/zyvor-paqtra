// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Configuration management
use serde::Deserialize;
use std::env;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub jwt_secret: String,
    pub hubble_address: String,
    /// How flows are fetched: `auto` (gRPC, CLI fallback), `grpc`, or `cli`.
    pub hubble_mode: HubbleMode,
    pub k8s_context: Option<String>,
    /// When true, authentication is completely disabled (dev/demo mode only).
    /// Read once at startup from AUTH_DISABLED env var.
    pub auth_disabled: bool,
    /// Console login username (default: admin).
    pub admin_username: String,
    /// Console login password (required when auth is enabled; min 8 chars).
    pub admin_password: String,
    /// Optional API key; when set, accepted as an alternate password for admin.
    pub api_key: String,
    /// Directory containing the built web UI static files (index.html, assets/, etc.)
    pub ui_dist_dir: Option<String>,
    /// Optional Prometheus server URL for querying real metrics (e.g. latency percentiles).
    pub prometheus_url: Option<String>,
    /// List of (cluster_name, hubble_address) pairs for multi-cluster aggregation.
    /// Parsed from HUBBLE_ADDRESSES env var as comma-separated `name=host:port` pairs.
    /// Falls back to a single "local" entry derived from `hubble_address`.
    pub hubble_addresses: Vec<(String, String)>,
    /// Optional OTLP HTTP endpoint for exporting trace spans (e.g. "http://localhost:4318").
    /// Read from OTEL_EXPORTER_ENDPOINT env var. When unset, tracing export is disabled.
    pub otel_endpoint: Option<String>,
    /// Service name reported in exported spans. Defaults to "paqtra-api".
    /// Read from OTEL_SERVICE_NAME env var.
    pub otel_service_name: String,
    /// Path to TLS certificate file (PEM format). When set with tls_key_path, enables HTTPS.
    pub tls_cert_path: Option<String>,
    /// Path to TLS private key file (PEM format).
    pub tls_key_path: Option<String>,
    /// Port for HTTPS when TLS is enabled. Defaults to 9443.
    pub tls_port: u16,
    /// Directory for the SQLite database that persists alert rules, audit log,
    /// exports, etc. across restarts. Read from PAQTRA_DATA_DIR. When unset,
    /// state is memory-only and lost on restart.
    pub data_dir: Option<String>,
    /// Seconds a still-firing alert waits before it is recorded and notified
    /// again. Read from ALERT_COOLDOWN_SECS (default 900).
    pub alert_cooldown_secs: u64,
    /// Seconds between alert rule evaluations. Read from ALERT_EVAL_INTERVAL_SECS
    /// (default 60, minimum 1).
    pub alert_eval_interval_secs: u64,
    /// Days of flow history to keep. Read from PAQTRA_FLOW_RETENTION_DAYS
    /// (default 7, between 1 and 90).
    pub flow_retention_days: i64,
    /// Seconds between Hubble flow captures. Read from FLOW_INGEST_INTERVAL_SECS
    /// (default 30, minimum 1).
    pub flow_ingest_interval_secs: u64,
}

/// How flows are fetched (`HUBBLE_MODE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HubbleMode {
    /// gRPC first; the `hubble` CLI only if gRPC fails and the binary exists.
    #[default]
    Auto,
    /// gRPC only; errors are reported instead of retried through the CLI.
    Grpc,
    /// The `hubble` CLI only (the behaviour before the gRPC client existed).
    Cli,
}

impl HubbleMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Some(Self::Auto),
            "grpc" => Some(Self::Grpc),
            "cli" => Some(Self::Cli),
            _ => None,
        }
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        // Only load .env file in non-production environments
        if env::var("ENVIRONMENT").unwrap_or_default() != "production" {
            dotenvy::dotenv().ok();
        }

        let jwt_secret = env::var("JWT_SECRET").map_err(|_| {
            anyhow::anyhow!(
                "JWT_SECRET environment variable is required. \
                 Set it to a strong random secret (at least 32 characters)."
            )
        })?;

        if jwt_secret.len() < 32 {
            anyhow::bail!(
                "JWT_SECRET must be at least 32 characters long for security. \
                 Current length: {}",
                jwt_secret.len()
            );
        }

        let auth_disabled = env::var("AUTH_DISABLED").unwrap_or_default() == "true";
        if auth_disabled {
            let environment = env::var("ENVIRONMENT").unwrap_or_default();
            if environment != "development" && environment != "test" && !environment.is_empty() {
                tracing::error!(
                    "FATAL: AUTH_DISABLED=true is set but ENVIRONMENT='{}' is not 'development' or 'test'. \
                     Refusing to start with authentication disabled in this environment.",
                    environment
                );
                anyhow::bail!(
                    "AUTH_DISABLED=true is not allowed when ENVIRONMENT='{}'. \
                     Set ENVIRONMENT=development or ENVIRONMENT=test, or remove AUTH_DISABLED.",
                    environment
                );
            }
            tracing::warn!(
                "!!! AUTH_DISABLED=true — authentication is completely bypassed !!! \
                 This is only safe in development/test environments."
            );
        }

        let hubble_address =
            env::var("HUBBLE_ADDRESS").unwrap_or_else(|_| "localhost:4245".to_string());

        let hubble_mode = match env::var("HUBBLE_MODE") {
            Ok(raw) => HubbleMode::parse(&raw).unwrap_or_else(|| {
                tracing::warn!("Ignoring invalid HUBBLE_MODE '{raw}': use auto, grpc or cli");
                Default::default()
            }),
            Err(_) => Default::default(),
        };

        let hubble_addresses = if let Ok(raw) = env::var("HUBBLE_ADDRESSES") {
            // Parse comma-separated name=host:port pairs
            raw.split(',')
                .filter_map(|entry| {
                    let entry = entry.trim();
                    if entry.is_empty() {
                        return None;
                    }
                    let parts: Vec<&str> = entry.splitn(2, '=').collect();
                    if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                        Some((parts[0].to_string(), parts[1].to_string()))
                    } else {
                        tracing::warn!(
                            "Ignoring invalid HUBBLE_ADDRESSES entry '{}': expected name=host:port",
                            entry
                        );
                        None
                    }
                })
                .collect()
        } else {
            // Derive a single entry from the primary hubble_address
            vec![("local".to_string(), hubble_address.clone())]
        };

        let admin_username = env::var("ADMIN_USERNAME").unwrap_or_else(|_| "admin".to_string());
        let (admin_password, api_key) = if auth_disabled {
            (
                env::var("ADMIN_PASSWORD").unwrap_or_default(),
                env::var("API_KEY").unwrap_or_default(),
            )
        } else {
            let admin_password = env::var("ADMIN_PASSWORD").map_err(|_| {
                anyhow::anyhow!(
                    "ADMIN_PASSWORD environment variable is required when auth is enabled. \
                     Set a strong password (at least 8 characters)."
                )
            })?;
            if admin_password.len() < 8 {
                anyhow::bail!(
                    "ADMIN_PASSWORD must be at least 8 characters long. Current length: {}",
                    admin_password.len()
                );
            }
            (admin_password, env::var("API_KEY").unwrap_or_default())
        };

        Ok(Self {
            host: env::var("PAQTRA_HOST")
                .or_else(|_| env::var("HOST"))
                .unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("PAQTRA_PORT")
                .or_else(|_| env::var("PORT"))
                .unwrap_or_else(|_| "9191".to_string())
                .parse()?,
            jwt_secret,
            hubble_address,
            hubble_mode,
            k8s_context: env::var("K8S_CONTEXT").ok(),
            auth_disabled,
            admin_username,
            admin_password,
            api_key,
            ui_dist_dir: env::var("UI_DIST_DIR").ok(),
            prometheus_url: env::var("PROMETHEUS_URL").ok(),
            hubble_addresses,
            otel_endpoint: env::var("OTEL_EXPORTER_ENDPOINT").ok(),
            otel_service_name: env::var("OTEL_SERVICE_NAME")
                .unwrap_or_else(|_| "paqtra-api".to_string()),
            tls_cert_path: env::var("TLS_CERT_PATH").ok().filter(|s| !s.is_empty()),
            tls_key_path: env::var("TLS_KEY_PATH").ok().filter(|s| !s.is_empty()),
            tls_port: env::var("TLS_PORT")
                .unwrap_or_else(|_| "9443".to_string())
                .parse()?,
            data_dir: env::var("PAQTRA_DATA_DIR").ok().filter(|s| !s.is_empty()),
            alert_cooldown_secs: env::var("ALERT_COOLDOWN_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(900),
            alert_eval_interval_secs: env::var("ALERT_EVAL_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .map(|v| v.max(1))
                .unwrap_or(60),
            flow_retention_days: env::var("PAQTRA_FLOW_RETENTION_DAYS")
                .ok()
                .and_then(|v| v.parse::<i64>().ok())
                .map(|v| v.clamp(1, 90))
                .unwrap_or(7),
            flow_ingest_interval_secs: env::var("FLOW_INGEST_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .map(|v| v.max(1))
                .unwrap_or(30),
        })
    }
}
