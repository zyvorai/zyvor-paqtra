// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Comprehensive handler integration tests for the Paqtra Web API.
//
// These tests exercise configuration validation, model validation, flow model
// serialization, auth/RBAC logic, pagination, Hubble service validation,
// K8s name validation, and K8s memory parsing -- all WITHOUT requiring a
// running Hubble relay or Kubernetes cluster.

use paqtra_api::auth_types::Claims;
use paqtra_api::config::Config;
use paqtra_api::models::flow::{Flow, FlowEndpoint};
use paqtra_api::models::policy::CreatePolicyRequest;
use paqtra_api::utils::{
    cap_hubble_flow_limit, filter_by_namespace_access, has_namespace_access, paginate_json,
    parse_k8s_memory, validate_hubble_address, validate_hubble_namespace, validate_k8s_name,
    PaginationQuery,
};

use serde_json::json;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Mutex that serializes all tests which modify process-global environment
/// variables. Rust runs tests in parallel by default, so without this lock
/// concurrent set_var / remove_var calls cause non-deterministic failures.
static ENV_MUTEX: Mutex<()> = Mutex::new(());

/// Helper to clear all config-related env vars to a known baseline.
fn clear_config_env() {
    unsafe {
        std::env::remove_var("JWT_SECRET");
        std::env::remove_var("HOST");
        std::env::remove_var("PORT");
        std::env::remove_var("PAQTRA_HOST");
        std::env::remove_var("PAQTRA_PORT");
        std::env::remove_var("HUBBLE_ADDRESS");
        std::env::remove_var("HUBBLE_ADDRESSES");
        std::env::remove_var("K8S_CONTEXT");
        std::env::remove_var("AUTH_DISABLED");
        std::env::remove_var("ADMIN_USERNAME");
        std::env::remove_var("ADMIN_PASSWORD");
        std::env::remove_var("API_KEY");
        std::env::remove_var("UI_DIST_DIR");
        std::env::remove_var("PROMETHEUS_URL");
        std::env::remove_var("ENVIRONMENT");
    }
}

/// Build a minimal Flow for testing.
fn make_flow(id: &str) -> Flow {
    Flow {
        id: id.to_string(),
        timestamp: "2025-06-01T00:00:00Z".to_string(),
        source: FlowEndpoint {
            namespace: "kube-system".to_string(),
            pod: "coredns-abc".to_string(),
            ip: "10.0.0.1".to_string(),
        },
        destination: FlowEndpoint {
            namespace: "default".to_string(),
            pod: "frontend-xyz".to_string(),
            ip: "10.0.0.2".to_string(),
        },
        verdict: "FORWARDED".to_string(),
        protocol: "TCP".to_string(),
        port: 8080,
        http_method: None,
        http_url: None,
        http_code: None,
        cluster: None,
        ..Default::default()
    }
}

// ===========================================================================
// 1. Config validation tests
// ===========================================================================

/// Config::load() must reject a JWT_SECRET shorter than 32 characters.
#[test]
fn test_config_rejects_short_jwt_secret() {
    let _lock = ENV_MUTEX.lock().unwrap();
    clear_config_env();

    unsafe {
        std::env::set_var("JWT_SECRET", "too-short-only-20chars");
    }

    let result = Config::load();
    assert!(
        result.is_err(),
        "Config::load should fail with a JWT_SECRET < 32 chars"
    );

    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("32 characters"),
        "Error should mention the 32-char minimum, got: {}",
        err_msg
    );

    clear_config_env();
}

/// Config::load() with a valid secret populates sensible defaults.
#[test]
fn test_config_defaults() {
    let _lock = ENV_MUTEX.lock().unwrap();
    clear_config_env();

    let secret = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"; // 40 chars
    unsafe {
        std::env::set_var("JWT_SECRET", secret);
        std::env::set_var("ADMIN_PASSWORD", "test-admin-pass-ok");
    }

    let config = Config::load().expect("Config::load should succeed with a valid secret");

    assert_eq!(config.host, "0.0.0.0", "Default host");
    assert_eq!(config.port, 9191, "Default port");
    assert_eq!(
        config.hubble_address, "localhost:4245",
        "Default Hubble address"
    );
    assert!(config.k8s_context.is_none(), "K8S_CONTEXT defaults to None");
    assert!(!config.auth_disabled, "Auth should be enabled by default");
    assert!(config.ui_dist_dir.is_none(), "UI_DIST_DIR defaults to None");
    assert!(
        config.prometheus_url.is_none(),
        "PROMETHEUS_URL defaults to None"
    );

    clear_config_env();
}

/// Comma-separated HUBBLE_ADDRESSES env var parses into a cluster list.
#[test]
fn test_config_hubble_addresses_parsing() {
    let _lock = ENV_MUTEX.lock().unwrap();
    clear_config_env();

    let secret = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    unsafe {
        std::env::set_var("JWT_SECRET", secret);
        std::env::set_var("ADMIN_PASSWORD", "test-admin-pass-ok");
        std::env::set_var(
            "HUBBLE_ADDRESSES",
            "us-west=hubble-west:4245,eu-central=hubble-eu:4245,ap-south=hubble-ap:4245",
        );
    }

    let config = Config::load().expect("Config::load should succeed");

    assert_eq!(
        config.hubble_addresses.len(),
        3,
        "Should parse 3 cluster entries"
    );
    assert_eq!(config.hubble_addresses[0].0, "us-west");
    assert_eq!(config.hubble_addresses[0].1, "hubble-west:4245");
    assert_eq!(config.hubble_addresses[1].0, "eu-central");
    assert_eq!(config.hubble_addresses[1].1, "hubble-eu:4245");
    assert_eq!(config.hubble_addresses[2].0, "ap-south");
    assert_eq!(config.hubble_addresses[2].1, "hubble-ap:4245");

    clear_config_env();
}

/// When HUBBLE_ADDRESSES is not set, a single "local" entry is derived from HUBBLE_ADDRESS.
#[test]
fn test_config_hubble_addresses_fallback_to_local() {
    let _lock = ENV_MUTEX.lock().unwrap();
    clear_config_env();

    let secret = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    unsafe {
        std::env::set_var("JWT_SECRET", secret);
        std::env::set_var("ADMIN_PASSWORD", "test-admin-pass-ok");
        std::env::set_var("HUBBLE_ADDRESS", "my-relay:4245");
    }

    let config = Config::load().expect("Config::load should succeed");

    assert_eq!(config.hubble_addresses.len(), 1);
    assert_eq!(config.hubble_addresses[0].0, "local");
    assert_eq!(config.hubble_addresses[0].1, "my-relay:4245");

    clear_config_env();
}

/// Malformed HUBBLE_ADDRESSES entries (missing =) are silently skipped.
#[test]
fn test_config_hubble_addresses_ignores_malformed() {
    let _lock = ENV_MUTEX.lock().unwrap();
    clear_config_env();

    let secret = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    unsafe {
        std::env::set_var("JWT_SECRET", secret);
        std::env::set_var("ADMIN_PASSWORD", "test-admin-pass-ok");
        std::env::set_var(
            "HUBBLE_ADDRESSES",
            "good=host:4245,bad-no-equals,=no-name,also-good=h2:4245",
        );
    }

    let config = Config::load().expect("Config::load should succeed");

    // Only "good" and "also-good" should parse
    assert_eq!(config.hubble_addresses.len(), 2);
    assert_eq!(config.hubble_addresses[0].0, "good");
    assert_eq!(config.hubble_addresses[1].0, "also-good");

    clear_config_env();
}

// ===========================================================================
// 2. Model validation tests (policy spec)
// ===========================================================================

/// A spec larger than 100KB must be rejected.
#[test]
fn test_policy_spec_size_limit() {
    // Build a spec that exceeds 100 KB when serialized
    let large_string = "x".repeat(120_000);
    let req = CreatePolicyRequest {
        name: "large-policy".to_string(),
        namespace: "default".to_string(),
        spec: json!({ "data": large_string }),
    };

    let result = req.validate_spec();
    assert!(result.is_err(), "Specs > 100KB should be rejected");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("exceeds maximum size"),
        "Error message should mention size limit, got: {}",
        msg
    );
}

/// A spec nested deeper than 20 levels must be rejected.
#[test]
fn test_policy_spec_depth_limit() {
    // Build a deeply nested JSON value (25 levels)
    let mut nested = json!("leaf");
    for _ in 0..25 {
        nested = json!({ "child": nested });
    }

    let req = CreatePolicyRequest {
        name: "deep-policy".to_string(),
        namespace: "default".to_string(),
        spec: nested,
    };

    let result = req.validate_spec();
    assert!(
        result.is_err(),
        "Specs nested > 20 levels should be rejected"
    );
    let msg = result.unwrap_err();
    assert!(
        msg.contains("nesting depth"),
        "Error message should mention depth limit, got: {}",
        msg
    );
}

/// A normal, valid spec passes validation.
#[test]
fn test_policy_spec_valid() {
    let req = CreatePolicyRequest {
        name: "allow-dns".to_string(),
        namespace: "kube-system".to_string(),
        spec: json!({
            "endpointSelector": { "matchLabels": { "app": "dns" } },
            "egress": [{ "toEndpoints": [{}] }]
        }),
    };

    let result = req.validate_spec();
    assert!(result.is_ok(), "A valid spec should pass validation");
}

/// RFC 1123 DNS name validation: valid names pass, invalid are rejected.
#[test]
fn test_policy_name_validation() {
    // Valid names
    assert!(validate_k8s_name("allow-dns", "name").is_ok());
    assert!(validate_k8s_name("my-policy-123", "name").is_ok());
    assert!(validate_k8s_name("a", "name").is_ok());
    assert!(validate_k8s_name("0abc", "name").is_ok());

    // Invalid names
    assert!(
        validate_k8s_name("UPPERCASE", "name").is_err(),
        "Uppercase names should be rejected"
    );
    assert!(
        validate_k8s_name("has spaces", "name").is_err(),
        "Names with spaces should be rejected"
    );
    assert!(
        validate_k8s_name("has_underscore", "name").is_err(),
        "Names with underscores should be rejected"
    );
}

/// Names starting with `-` must be rejected (flag injection prevention).
#[test]
fn test_policy_name_rejects_flags() {
    let result = validate_k8s_name("-flag-like", "name");
    assert!(
        result.is_err(),
        "Names starting with '-' should be rejected to prevent flag injection"
    );
}

// ===========================================================================
// 3. Flow model tests
// ===========================================================================

/// Flow serializes to JSON with all expected fields.
#[test]
fn test_flow_serialization() {
    let flow = Flow {
        id: "flow-42".to_string(),
        timestamp: "2025-06-01T12:00:00Z".to_string(),
        source: FlowEndpoint {
            namespace: "monitoring".to_string(),
            pod: "prometheus-0".to_string(),
            ip: "10.1.0.1".to_string(),
        },
        destination: FlowEndpoint {
            namespace: "default".to_string(),
            pod: "api-server".to_string(),
            ip: "10.2.0.1".to_string(),
        },
        verdict: "FORWARDED".to_string(),
        protocol: "TCP".to_string(),
        port: 443,
        http_method: Some("GET".to_string()),
        http_url: Some("/healthz".to_string()),
        http_code: Some(200),
        cluster: Some("us-east-1".to_string()),
        ..Default::default()
    };

    let value = serde_json::to_value(&flow).expect("serialize flow");
    let obj = value.as_object().expect("should be JSON object");

    assert_eq!(value["id"], "flow-42");
    assert_eq!(value["timestamp"], "2025-06-01T12:00:00Z");
    assert_eq!(value["verdict"], "FORWARDED");
    assert_eq!(value["protocol"], "TCP");
    assert_eq!(value["port"], 443);
    assert_eq!(value["http_method"], "GET");
    assert_eq!(value["http_url"], "/healthz");
    assert_eq!(value["http_code"], 200);
    assert_eq!(value["cluster"], "us-east-1");

    // Source and destination should be nested objects
    assert!(obj.contains_key("source"));
    assert!(obj.contains_key("destination"));
    assert_eq!(value["source"]["namespace"], "monitoring");
    assert_eq!(value["destination"]["pod"], "api-server");
}

/// When optional L7 fields (http_method, http_url, http_code) are None,
/// they should be omitted from the serialized JSON (skip_serializing_if).
#[test]
fn test_flow_optional_l7_fields() {
    let flow = make_flow("no-l7");
    assert!(flow.http_method.is_none());
    assert!(flow.http_url.is_none());
    assert!(flow.http_code.is_none());

    let value = serde_json::to_value(&flow).expect("serialize flow");
    let obj = value.as_object().expect("should be object");

    assert!(
        !obj.contains_key("http_method"),
        "http_method=None should be omitted from JSON"
    );
    assert!(
        !obj.contains_key("http_url"),
        "http_url=None should be omitted from JSON"
    );
    assert!(
        !obj.contains_key("http_code"),
        "http_code=None should be omitted from JSON"
    );
    assert!(
        !obj.contains_key("cluster"),
        "cluster=None should be omitted from JSON"
    );
}

/// FlowEndpoint round-trips through JSON without data loss.
#[test]
fn test_flow_endpoint_serialization() {
    let endpoint = FlowEndpoint {
        namespace: "production".to_string(),
        pod: "api-gateway-0".to_string(),
        ip: "172.20.0.55".to_string(),
    };

    let json_str = serde_json::to_string(&endpoint).expect("serialize");
    let restored: FlowEndpoint = serde_json::from_str(&json_str).expect("deserialize");

    assert_eq!(restored.namespace, "production");
    assert_eq!(restored.pod, "api-gateway-0");
    assert_eq!(restored.ip, "172.20.0.55");

    // Verify exactly 3 keys
    let value = serde_json::to_value(&endpoint).expect("to_value");
    assert_eq!(
        value.as_object().unwrap().len(),
        3,
        "FlowEndpoint should have exactly 3 fields"
    );
}

/// Flow with L7 HTTP fields round-trips correctly.
#[test]
fn test_flow_with_l7_roundtrip() {
    let flow = Flow {
        id: "l7-flow".to_string(),
        timestamp: "t".to_string(),
        source: FlowEndpoint {
            namespace: "ns".to_string(),
            pod: "p".to_string(),
            ip: "1.2.3.4".to_string(),
        },
        destination: FlowEndpoint {
            namespace: "ns2".to_string(),
            pod: "p2".to_string(),
            ip: "5.6.7.8".to_string(),
        },
        verdict: "FORWARDED".to_string(),
        protocol: "TCP".to_string(),
        port: 80,
        http_method: Some("POST".to_string()),
        http_url: Some("/api/v1/data".to_string()),
        http_code: Some(201),
        cluster: Some("cluster-a".to_string()),
        ..Default::default()
    };

    let json_str = serde_json::to_string(&flow).expect("serialize");
    let restored: Flow = serde_json::from_str(&json_str).expect("deserialize");

    assert_eq!(restored.http_method.as_deref(), Some("POST"));
    assert_eq!(restored.http_url.as_deref(), Some("/api/v1/data"));
    assert_eq!(restored.http_code, Some(201));
    assert_eq!(restored.cluster.as_deref(), Some("cluster-a"));
}

// ===========================================================================
// 4. Auth/RBAC tests
// ===========================================================================

/// Claims with namespaces field deserializes correctly.
#[test]
fn test_claims_deserialization_with_namespaces() {
    let input = json!({
        "sub": "user-1",
        "exp": 9999999999u64,
        "iat": 1700000000u64,
        "role": "viewer",
        "namespaces": ["default", "production", "staging"]
    });

    let claims: Claims = serde_json::from_value(input).expect("deserialize claims");

    assert_eq!(claims.sub, "user-1");
    assert_eq!(claims.role, "viewer");
    assert_eq!(claims.namespaces.len(), 3);
    assert_eq!(claims.namespaces[0], "default");
    assert_eq!(claims.namespaces[1], "production");
    assert_eq!(claims.namespaces[2], "staging");
}

/// Claims without namespaces field defaults to an empty vec (backwards compat).
#[test]
fn test_claims_deserialization_without_namespaces() {
    let input = json!({
        "sub": "legacy-user",
        "exp": 9999999999u64,
        "iat": 1700000000u64,
        "role": "admin"
    });

    let claims: Claims = serde_json::from_value(input).expect("deserialize claims");

    assert_eq!(claims.sub, "legacy-user");
    assert_eq!(claims.role, "admin");
    assert!(
        claims.namespaces.is_empty(),
        "Omitted namespaces should default to empty vec"
    );
}

/// Admin role bypasses namespace restrictions.
#[test]
fn test_has_namespace_access_admin_bypasses() {
    let ns_list = vec!["production".to_string()];
    assert!(
        has_namespace_access("admin", &ns_list, "any-namespace"),
        "Admin role should have access to any namespace"
    );
    assert!(
        has_namespace_access("admin", &ns_list, "secret-ns"),
        "Admin role should bypass namespace list"
    );
    // Admin with empty list
    assert!(has_namespace_access("admin", &[], "anything"));
}

/// Non-admin users are restricted to their listed namespaces.
#[test]
fn test_has_namespace_access_scoped() {
    let allowed = vec!["default".to_string(), "staging".to_string()];

    assert!(
        has_namespace_access("viewer", &allowed, "default"),
        "Should have access to listed namespace 'default'"
    );
    assert!(
        has_namespace_access("viewer", &allowed, "staging"),
        "Should have access to listed namespace 'staging'"
    );
    assert!(
        !has_namespace_access("viewer", &allowed, "production"),
        "Should NOT have access to unlisted namespace 'production'"
    );
    assert!(
        !has_namespace_access("viewer", &allowed, "kube-system"),
        "Should NOT have access to unlisted namespace 'kube-system'"
    );
}

/// Wildcard "*" in namespaces list allows access to all namespaces.
#[test]
fn test_has_namespace_access_wildcard() {
    let allowed = vec!["*".to_string()];

    assert!(
        has_namespace_access("viewer", &allowed, "default"),
        "Wildcard should grant access to 'default'"
    );
    assert!(
        has_namespace_access("viewer", &allowed, "any-namespace"),
        "Wildcard should grant access to any namespace"
    );
    assert!(
        has_namespace_access("editor", &allowed, "production"),
        "Wildcard should grant access regardless of role"
    );
}

/// Empty namespaces list grants access to all namespaces (all-access).
#[test]
fn test_has_namespace_access_empty_means_all() {
    let empty: Vec<String> = vec![];

    assert!(
        has_namespace_access("viewer", &empty, "default"),
        "Empty namespace list should grant access to all"
    );
    assert!(
        has_namespace_access("viewer", &empty, "anything"),
        "Empty namespace list should grant access to all"
    );
}

/// filter_by_namespace_access keeps only items in allowed namespaces.
#[test]
fn test_filter_by_namespace_access() {
    let items = vec![
        json!({"name": "a", "namespace": "default"}),
        json!({"name": "b", "namespace": "production"}),
        json!({"name": "c", "namespace": "staging"}),
        json!({"name": "d"}), // no namespace field
    ];

    let allowed = vec!["default".to_string(), "staging".to_string()];

    let filtered = filter_by_namespace_access("viewer", &allowed, items);

    assert_eq!(
        filtered.len(),
        3,
        "Should keep default, staging, and the item without namespace"
    );
    assert_eq!(filtered[0]["name"], "a"); // default - kept
    assert_eq!(filtered[1]["name"], "c"); // staging - kept
    assert_eq!(filtered[2]["name"], "d"); // no namespace - kept
}

/// filter_by_namespace_access with admin role keeps all items.
#[test]
fn test_filter_by_namespace_access_admin_keeps_all() {
    let items = vec![
        json!({"name": "a", "namespace": "default"}),
        json!({"name": "b", "namespace": "production"}),
        json!({"name": "c", "namespace": "secret"}),
    ];

    let allowed = vec!["default".to_string()]; // restrictive list, but admin overrides

    let filtered = filter_by_namespace_access("admin", &allowed, items);
    assert_eq!(filtered.len(), 3, "Admin should see all items");
}

/// filter_by_namespace_access with empty namespace list keeps all items.
#[test]
fn test_filter_by_namespace_access_empty_keeps_all() {
    let items = vec![
        json!({"name": "a", "namespace": "ns1"}),
        json!({"name": "b", "namespace": "ns2"}),
    ];

    let filtered = filter_by_namespace_access("viewer", &[], items);
    assert_eq!(filtered.len(), 2, "Empty namespace list means all access");
}

/// filter_by_namespace_access with wildcard keeps all items.
#[test]
fn test_filter_by_namespace_access_wildcard() {
    let items = vec![
        json!({"name": "a", "namespace": "ns1"}),
        json!({"name": "b", "namespace": "ns2"}),
        json!({"name": "c", "namespace": "ns3"}),
    ];

    let allowed = vec!["*".to_string()];
    let filtered = filter_by_namespace_access("viewer", &allowed, items);
    assert_eq!(filtered.len(), 3, "Wildcard should keep all items");
}

// ===========================================================================
// 5. Pagination tests
// ===========================================================================

/// paginate_json with default parameters: offset=0, limit=50.
#[test]
fn test_paginate_json_defaults() {
    let items: Vec<serde_json::Value> = (0..100).map(|i| json!({"id": i})).collect();

    let params = PaginationQuery {
        limit: None,
        offset: None,
    };
    let result = paginate_json(items, &params, "items");

    assert_eq!(result["total"], 100);
    assert_eq!(result["offset"], 0);
    assert_eq!(result["limit"], 50);
    assert_eq!(
        result["items"].as_array().unwrap().len(),
        50,
        "Default limit should return 50 items"
    );
    // First item should be id=0
    assert_eq!(result["items"][0]["id"], 0);
    // Last item should be id=49
    assert_eq!(result["items"][49]["id"], 49);
}

/// paginate_json with custom offset and limit.
#[test]
fn test_paginate_json_custom() {
    let items: Vec<serde_json::Value> = (0..100).map(|i| json!({"id": i})).collect();

    let params = PaginationQuery {
        limit: Some(10),
        offset: Some(20),
    };
    let result = paginate_json(items, &params, "records");

    assert_eq!(result["total"], 100);
    assert_eq!(result["offset"], 20);
    assert_eq!(result["limit"], 10);
    assert_eq!(
        result["records"].as_array().unwrap().len(),
        10,
        "Should return exactly 10 items"
    );
    // First item should be id=20
    assert_eq!(result["records"][0]["id"], 20);
    // Last item should be id=29
    assert_eq!(result["records"][9]["id"], 29);
}

/// paginate_json limit is capped at 1000.
#[test]
fn test_paginate_json_limit_cap() {
    let items: Vec<serde_json::Value> = (0..2000).map(|i| json!({"id": i})).collect();

    let params = PaginationQuery {
        limit: Some(5000),
        offset: None,
    };
    let result = paginate_json(items, &params, "items");

    assert_eq!(result["total"], 2000);
    assert_eq!(result["limit"], 1000, "Limit should be capped at 1000");
    assert_eq!(
        result["items"].as_array().unwrap().len(),
        1000,
        "Should return at most 1000 items"
    );
}

/// paginate_json with empty items returns empty page.
#[test]
fn test_paginate_json_empty() {
    let items: Vec<serde_json::Value> = vec![];

    let params = PaginationQuery {
        limit: Some(50),
        offset: None,
    };
    let result = paginate_json(items, &params, "data");

    assert_eq!(result["total"], 0);
    assert_eq!(result["data"].as_array().unwrap().len(), 0);
    assert_eq!(result["limit"], 50);
    assert_eq!(result["offset"], 0);
}

/// paginate_json with offset beyond total returns empty page.
#[test]
fn test_paginate_json_offset_beyond_total() {
    let items: Vec<serde_json::Value> = (0..10).map(|i| json!({"id": i})).collect();

    let params = PaginationQuery {
        limit: Some(10),
        offset: Some(100),
    };
    let result = paginate_json(items, &params, "items");

    assert_eq!(result["total"], 10);
    assert_eq!(result["offset"], 100);
    assert_eq!(
        result["items"].as_array().unwrap().len(),
        0,
        "Offset beyond total should return empty page"
    );
}

/// paginate_json preserves the custom items_key.
#[test]
fn test_paginate_json_custom_items_key() {
    let items = vec![json!({"x": 1})];
    let params = PaginationQuery {
        limit: None,
        offset: None,
    };

    let result = paginate_json(items, &params, "flows");

    assert!(
        result.get("flows").is_some(),
        "Should use custom key 'flows'"
    );
    assert!(
        result.get("items").is_none(),
        "Should not use default 'items' key"
    );
}

// ===========================================================================
// 6. Hubble service tests
// ===========================================================================

/// Valid Hubble addresses pass validation.
#[test]
fn test_hubble_address_validation_valid() {
    assert!(validate_hubble_address("localhost:4245").is_ok());
    assert!(validate_hubble_address("hubble-relay:4245").is_ok());
    assert!(validate_hubble_address("10.0.0.1:4245").is_ok());
    assert!(validate_hubble_address("hubble-relay.kube-system.svc:4245").is_ok());
    assert!(validate_hubble_address("[::1]:4245").is_ok());
}

/// Invalid Hubble addresses are rejected.
#[test]
fn test_hubble_address_validation_invalid() {
    // No port
    assert!(
        validate_hubble_address("localhost").is_err(),
        "Address without port should be rejected"
    );
    // Empty string
    assert!(
        validate_hubble_address("").is_err(),
        "Empty address should be rejected"
    );
    // No host
    assert!(
        validate_hubble_address(":4245").is_err(),
        "Address without host should be rejected"
    );
    // Port is not a number
    assert!(
        validate_hubble_address("host:notaport").is_err(),
        "Non-numeric port should be rejected"
    );
    // Port out of u16 range
    assert!(
        validate_hubble_address("host:99999").is_err(),
        "Port > 65535 should be rejected"
    );
}

/// Flow limit is capped at 10000.
#[test]
fn test_hubble_flow_limit_cap() {
    assert_eq!(cap_hubble_flow_limit(100), 100);
    assert_eq!(cap_hubble_flow_limit(10_000), 10_000);
    assert_eq!(
        cap_hubble_flow_limit(50_000),
        10_000,
        "Limit should be capped at 10000"
    );
    assert_eq!(
        cap_hubble_flow_limit(usize::MAX),
        10_000,
        "Limit should be capped at 10000"
    );
    assert_eq!(cap_hubble_flow_limit(0), 0, "Zero limit stays zero");
}

/// Flag-like namespace strings are rejected.
#[test]
fn test_hubble_namespace_validation() {
    // Valid
    assert!(validate_hubble_namespace("default").is_ok());
    assert!(validate_hubble_namespace("kube-system").is_ok());
    assert!(validate_hubble_namespace("my-app-123").is_ok());

    // Invalid: flag-like
    assert!(
        validate_hubble_namespace("-n").is_err(),
        "Flag-like namespace '-n' should be rejected"
    );
    assert!(
        validate_hubble_namespace("--all").is_err(),
        "Flag-like namespace '--all' should be rejected"
    );

    // Invalid: whitespace
    assert!(
        validate_hubble_namespace("default production").is_err(),
        "Namespace with spaces should be rejected"
    );
    assert!(
        validate_hubble_namespace("ns\ttab").is_err(),
        "Namespace with tabs should be rejected"
    );
}

// ===========================================================================
// 7. K8s service tests
// ===========================================================================

/// Valid RFC 1123 names pass validation.
#[test]
fn test_k8s_name_validation_valid() {
    assert!(validate_k8s_name("my-service", "name").is_ok());
    assert!(validate_k8s_name("coredns", "name").is_ok());
    assert!(validate_k8s_name("app-v2", "name").is_ok());
    assert!(validate_k8s_name("0-starts-with-number", "name").is_ok());
    assert!(validate_k8s_name("a", "name").is_ok());
    assert!(
        validate_k8s_name("a.b.c", "name").is_ok(),
        "Dots are allowed in DNS subdomains"
    );
    assert!(validate_k8s_name("a-b-c", "name").is_ok());
}

/// Invalid RFC 1123 names are rejected.
#[test]
fn test_k8s_name_validation_invalid() {
    assert!(
        validate_k8s_name("Capital", "name").is_err(),
        "Uppercase letters should be rejected"
    );
    assert!(
        validate_k8s_name("has_underscore", "name").is_err(),
        "Underscores should be rejected"
    );
    assert!(
        validate_k8s_name("has space", "name").is_err(),
        "Spaces should be rejected"
    );
    assert!(
        validate_k8s_name("special!char", "name").is_err(),
        "Special characters should be rejected"
    );
    // Very long name (> 253 chars)
    let long_name = "a".repeat(254);
    assert!(
        validate_k8s_name(&long_name, "name").is_err(),
        "Names longer than 253 chars should be rejected"
    );
}

/// Empty name is rejected.
#[test]
fn test_k8s_name_rejects_empty() {
    let result = validate_k8s_name("", "policy name");
    assert!(result.is_err(), "Empty name should be rejected");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("must not be empty"),
        "Error should mention 'must not be empty', got: {}",
        msg
    );
}

/// Names starting with `-` are rejected (flag injection prevention).
#[test]
fn test_k8s_name_rejects_flag() {
    // Note: the regex ^[a-z0-9] already rejects '-' as first char,
    // but the explicit check provides a clearer error message.
    let result = validate_k8s_name("-delete-all", "name");
    assert!(
        result.is_err(),
        "Names starting with '-' should be rejected"
    );
}

/// Field name is included in the error message for K8s validation.
#[test]
fn test_k8s_name_error_includes_field() {
    let result = validate_k8s_name("INVALID", "namespace");
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(
        msg.contains("namespace"),
        "Error should include the field name, got: {}",
        msg
    );
}

// ===========================================================================
// 8. Cache key / K8s memory parsing tests
// ===========================================================================

/// Parse "16384Ki" (kibibytes) to GB.
#[test]
fn test_parse_k8s_memory_ki() {
    let gb = parse_k8s_memory("16384Ki");
    // 16384 Ki = 16384 / 1024 / 1024 GB = 16384 / 1048576 GB
    let expected = 16384.0 / (1024.0 * 1024.0);
    assert!(
        (gb - expected).abs() < 1e-6,
        "16384Ki should parse to ~{:.6} GB, got {:.6}",
        expected,
        gb
    );
}

/// Parse "8Gi" (gibibytes) to GB.
#[test]
fn test_parse_k8s_memory_gi() {
    let gb = parse_k8s_memory("8Gi");
    assert!(
        (gb - 8.0).abs() < 1e-6,
        "8Gi should parse to 8.0 GB, got {}",
        gb
    );
}

/// Parse "512Mi" (mebibytes) to GB.
#[test]
fn test_parse_k8s_memory_mi() {
    let gb = parse_k8s_memory("512Mi");
    let expected = 512.0 / 1024.0; // 0.5 GB
    assert!(
        (gb - expected).abs() < 1e-6,
        "512Mi should parse to {:.6} GB, got {:.6}",
        expected,
        gb
    );
}

/// Parse bare bytes (no suffix) to GB.
#[test]
fn test_parse_k8s_memory_bytes() {
    let gb = parse_k8s_memory("1073741824"); // 1 GiB in bytes
    let expected = 1073741824.0 / (1024.0 * 1024.0 * 1024.0);
    assert!(
        (gb - expected).abs() < 1e-6,
        "1073741824 bytes should parse to ~1.0 GB, got {}",
        gb
    );
}

/// Unparseable memory strings return 0.
#[test]
fn test_parse_k8s_memory_invalid() {
    assert!(
        parse_k8s_memory("notanumber").abs() < 1e-10,
        "Invalid strings should return 0.0"
    );
    assert!(
        parse_k8s_memory("").abs() < 1e-10,
        "Empty string should return 0.0"
    );
}

/// Large "Ki" values parse correctly (e.g. 32 GB in Ki).
#[test]
fn test_parse_k8s_memory_large_ki() {
    // 32 GB = 32 * 1024 * 1024 Ki = 33554432 Ki
    let gb = parse_k8s_memory("33554432Ki");
    assert!(
        (gb - 32.0).abs() < 1e-4,
        "33554432Ki should be ~32.0 GB, got {}",
        gb
    );
}
