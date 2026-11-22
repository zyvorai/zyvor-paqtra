// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Prometheus query service -- fetches metrics from a Prometheus server via HTTP.
// Uses `curl` through `tokio::process::Command` to avoid adding an HTTP client dependency.

use anyhow::{bail, Context, Result};
use serde_json::Value;

pub struct PrometheusService {
    base_url: Option<String>,
}

/// Minimal percent-encoding for query parameter values (avoids adding a crate).
fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", b));
            }
        }
    }
    out
}

impl PrometheusService {
    pub fn new(url: Option<String>) -> Self {
        // Strip trailing slash for consistent URL construction
        let base_url = url.map(|u| u.trim_end_matches('/').to_string());
        Self { base_url }
    }

    /// Returns true if a Prometheus URL has been configured.
    pub fn is_configured(&self) -> bool {
        self.base_url.is_some()
    }

    /// Execute an instant PromQL query against `/api/v1/query`.
    pub async fn query(&self, promql: &str) -> Result<Value> {
        let base = self
            .base_url
            .as_deref()
            .context("Prometheus URL not configured")?;

        let encoded = percent_encode(promql);
        let url = format!("{}/api/v1/query?query={}", base, encoded);

        let output = tokio::process::Command::new("curl")
            .args(["-sf", &url])
            .output()
            .await
            .context("Failed to execute curl")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Prometheus query failed (exit {}): {}",
                output.status,
                stderr
            );
        }

        let body = String::from_utf8_lossy(&output.stdout);
        let json: Value =
            serde_json::from_str(&body).context("Failed to parse Prometheus response as JSON")?;
        Ok(json)
    }

    /// Execute a range PromQL query against `/api/v1/query_range`.
    pub async fn query_range(
        &self,
        promql: &str,
        start: &str,
        end: &str,
        step: &str,
    ) -> Result<Value> {
        let base = self
            .base_url
            .as_deref()
            .context("Prometheus URL not configured")?;

        let encoded = percent_encode(promql);
        let url = format!(
            "{}/api/v1/query_range?query={}&start={}&end={}&step={}",
            base,
            encoded,
            percent_encode(start),
            percent_encode(end),
            percent_encode(step),
        );

        let output = tokio::process::Command::new("curl")
            .args(["-sf", &url])
            .output()
            .await
            .context("Failed to execute curl")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Prometheus range query failed (exit {}): {}",
                output.status,
                stderr
            );
        }

        let body = String::from_utf8_lossy(&output.stdout);
        let json: Value =
            serde_json::from_str(&body).context("Failed to parse Prometheus response as JSON")?;
        Ok(json)
    }

    /// Convenience method: run an instant query and extract the first scalar value.
    ///
    /// Expects the standard Prometheus response shape:
    /// `{ "data": { "result": [ { "value": [<timestamp>, "<value>"] } ] } }`
    pub async fn get_metric_value(&self, promql: &str) -> Option<f64> {
        let json = self.query(promql).await.ok()?;
        json.get("data")?
            .get("result")?
            .as_array()?
            .first()?
            .get("value")?
            .as_array()?
            .get(1)?
            .as_str()?
            .parse::<f64>()
            .ok()
    }
}
