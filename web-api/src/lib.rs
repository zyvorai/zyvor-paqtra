// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Library root for integration tests and external consumers.
// The binary entry point remains in main.rs.
//
// Only modules that do not depend on AppState are re-exported here,
// keeping the library target self-contained and free of runtime
// dependencies like Hubble, or Kubernetes.

pub mod config;
pub mod error;
pub mod models;

// Re-export the Claims struct from the auth middleware.
// The full middleware module is not exported because the
// auth_middleware function depends on AppState.
pub mod auth_types {
    use serde::{Deserialize, Serialize};

    /// JWT claims - mirrors the definition in middleware/auth.rs
    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct Claims {
        pub sub: String,
        pub exp: usize,
        pub iat: usize,
        pub role: String,
        #[serde(default)]
        pub namespaces: Vec<String>, // Empty = all namespaces (for backwards compat)
    }
}

/// Pure utility functions extracted from handlers and services for testability.
/// These have no runtime dependencies (no Hubble, or Kubernetes).
pub mod utils {
    use regex::Regex;
    use serde_json::Value;
    use std::sync::LazyLock;

    // -----------------------------------------------------------------------
    // Pagination
    // -----------------------------------------------------------------------

    /// Shared pagination query params for list endpoints.
    #[derive(Debug, serde::Deserialize)]
    pub struct PaginationQuery {
        pub limit: Option<usize>,
        pub offset: Option<usize>,
    }

    /// Apply pagination to a list of JSON values and return a response with metadata.
    /// `items_key` is the JSON field name for the items array.
    pub fn paginate_json(items: Vec<Value>, params: &PaginationQuery, items_key: &str) -> Value {
        let total = items.len();
        let offset = params.offset.unwrap_or(0);
        let limit = params.limit.unwrap_or(50).min(1000);
        let page: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
        serde_json::json!({
            items_key: page,
            "total": total,
            "limit": limit,
            "offset": offset,
        })
    }

    // -----------------------------------------------------------------------
    // Kubernetes name validation (RFC 1123 DNS subdomain)
    // -----------------------------------------------------------------------

    /// Kubernetes resource name validation regex: RFC 1123 DNS subdomain
    static K8S_NAME_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9.\-]{0,252}$").unwrap());

    /// Validate a Kubernetes resource name according to RFC 1123 DNS subdomain rules.
    /// Returns `Ok(())` on success, `Err(message)` on failure.
    pub fn validate_k8s_name(name: &str, field: &str) -> Result<(), String> {
        if name.is_empty() {
            return Err(format!("{} must not be empty", field));
        }
        if !K8S_NAME_RE.is_match(name) {
            return Err(format!(
                "{} '{}' is not a valid Kubernetes name (must match RFC 1123 DNS subdomain)",
                field, name
            ));
        }
        // Reject names that look like kubectl flags
        if name.starts_with('-') {
            return Err(format!("{} must not start with '-'", field));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // K8s memory string parsing
    // -----------------------------------------------------------------------

    /// Parse Kubernetes memory strings like "16384Ki", "8Gi", "512Mi" to GB.
    pub fn parse_k8s_memory(s: &str) -> f64 {
        if let Some(ki) = s.strip_suffix("Ki") {
            ki.parse::<f64>().unwrap_or(0.0) / (1024.0 * 1024.0)
        } else if let Some(mi) = s.strip_suffix("Mi") {
            mi.parse::<f64>().unwrap_or(0.0) / 1024.0
        } else if let Some(gi) = s.strip_suffix("Gi") {
            gi.parse::<f64>().unwrap_or(0.0)
        } else {
            s.parse::<f64>().unwrap_or(0.0) / (1024.0 * 1024.0 * 1024.0)
        }
    }

    // -----------------------------------------------------------------------
    // Namespace RBAC helpers (pure logic, no AppState dependency)
    // -----------------------------------------------------------------------

    /// Check if a given role + namespace list grants access to a specific namespace.
    /// Returns true if:
    /// - `role` is `"admin"`,
    /// - `namespaces` list is empty (all access),
    /// - `namespaces` contains `namespace` or `"*"`.
    pub fn has_namespace_access(role: &str, namespaces: &[String], namespace: &str) -> bool {
        if role == "admin" {
            return true;
        }
        if namespaces.is_empty() {
            return true; // empty = all namespaces
        }
        namespaces.iter().any(|ns| ns == namespace || ns == "*")
    }

    /// Filter a list of JSON values by namespace access.
    /// Checks the `"namespace"` field on each item.
    /// Items without a `"namespace"` field are kept.
    pub fn filter_by_namespace_access(
        role: &str,
        namespaces: &[String],
        items: Vec<Value>,
    ) -> Vec<Value> {
        if role == "admin" || namespaces.is_empty() {
            return items;
        }
        items
            .into_iter()
            .filter(|item| {
                item.get("namespace")
                    .and_then(|v| v.as_str())
                    .map(|ns| {
                        namespaces
                            .iter()
                            .any(|allowed| allowed == ns || allowed == "*")
                    })
                    .unwrap_or(true)
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // Hubble address validation
    // -----------------------------------------------------------------------

    /// Validate a Hubble address in `host:port` format.
    /// Returns `Ok(())` on success, `Err(message)` on failure.
    pub fn validate_hubble_address(address: &str) -> Result<(), String> {
        let parts: Vec<&str> = address.rsplitn(2, ':').collect();
        if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
            return Err(format!(
                "Invalid Hubble address '{}': expected host:port format (e.g. hubble-relay:4245)",
                address
            ));
        }
        if parts[0].parse::<u16>().is_err() {
            return Err(format!(
                "Invalid Hubble address '{}': port must be a valid u16",
                address
            ));
        }
        Ok(())
    }

    /// Validate a namespace string for Hubble queries.
    /// Rejects flag-like strings (starting with `-`) and strings containing whitespace.
    pub fn validate_hubble_namespace(ns: &str) -> Result<(), String> {
        if ns.starts_with('-') {
            return Err(format!("Invalid namespace: '{}'", ns));
        }
        if ns.contains(char::is_whitespace) {
            return Err(format!("Invalid namespace: '{}'", ns));
        }
        Ok(())
    }

    /// Cap a Hubble flow limit to the maximum of 10000.
    pub fn cap_hubble_flow_limit(limit: usize) -> usize {
        limit.min(10_000)
    }
}
