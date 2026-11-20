// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Integration tests for the Paqtra Web API.
//
// These tests exercise configuration validation, model serialization,
// error response formatting, and auth token claim structures WITHOUT
// requiring any external service.

use paqtra_api::auth_types::Claims;
use paqtra_api::config::Config;
use paqtra_api::error::ApiError;
use paqtra_api::models::flow::{Flow, FlowEndpoint, FlowStats};
use paqtra_api::models::policy::{CreatePolicyRequest, Policy};

use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::json;

use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Serialize a value to JSON and parse it back as a generic serde_json::Value
/// so we can inspect fields without worrying about struct visibility.
fn roundtrip_json<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
    let json_str = serde_json::to_string(value).expect("serialization must succeed");
    serde_json::from_str(&json_str).expect("deserialization must succeed")
}

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
        std::env::remove_var("K8S_CONTEXT");
        std::env::remove_var("AUTH_DISABLED");
        std::env::remove_var("ADMIN_USERNAME");
        std::env::remove_var("ADMIN_PASSWORD");
        std::env::remove_var("API_KEY");
    }
}

// ===========================================================================
// 1. Config validation
// ===========================================================================

/// Config::load() must reject a JWT_SECRET shorter than 32 characters.
#[test]
fn test_jwt_secret_too_short() {
    let _lock = ENV_MUTEX.lock().unwrap();
    clear_config_env();

    unsafe {
        std::env::set_var("JWT_SECRET", "short");
    }
    let result = Config::load();
    assert!(
        result.is_err(),
        "Config::load should fail when JWT_SECRET is too short"
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("32 characters"),
        "Error message should mention the minimum length requirement, got: {}",
        err_msg
    );

    clear_config_env();
}

/// Config::load() must reject a missing JWT_SECRET entirely.
#[test]
fn test_jwt_secret_missing() {
    let _lock = ENV_MUTEX.lock().unwrap();
    clear_config_env();

    let result = Config::load();
    assert!(
        result.is_err(),
        "Config::load should fail when JWT_SECRET is missing"
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("JWT_SECRET"),
        "Error message should reference JWT_SECRET, got: {}",
        err_msg
    );
}

/// Config::load() must reject a missing ADMIN_PASSWORD when auth is enabled.
#[test]
fn test_admin_password_missing() {
    let _lock = ENV_MUTEX.lock().unwrap();
    clear_config_env();

    let secret = "a]3kF9#mP!xQ7$wL2^rT5&vB8*nJ0dYcHgUeZsAiOlCbNfR";
    unsafe {
        std::env::set_var("JWT_SECRET", secret);
    }
    let result = Config::load();
    assert!(
        result.is_err(),
        "Config::load should fail when ADMIN_PASSWORD is missing"
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("ADMIN_PASSWORD"),
        "Error message should reference ADMIN_PASSWORD, got: {}",
        err_msg
    );

    clear_config_env();
}

/// Config::load() must succeed when a sufficiently long JWT_SECRET is provided
/// and must populate default values for optional fields.
#[test]
fn test_config_load_with_valid_secret() {
    let _lock = ENV_MUTEX.lock().unwrap();
    clear_config_env();

    let secret = "a]3kF9#mP!xQ7$wL2^rT5&vB8*nJ0dYcHgUeZsAiOlCbNfR";
    unsafe {
        std::env::set_var("JWT_SECRET", secret);
        std::env::set_var("ADMIN_PASSWORD", "test-admin-pass-ok");
    }

    let config = Config::load().expect("Config::load should succeed with a valid secret");

    assert_eq!(config.jwt_secret, secret);
    assert_eq!(config.host, "0.0.0.0", "Default host should be 0.0.0.0");
    assert_eq!(config.port, 9191, "Default port should be 9191");
    assert_eq!(
        config.hubble_address, "localhost:4245",
        "Default Hubble address should be localhost:4245"
    );
    assert!(
        config.k8s_context.is_none(),
        "K8S_CONTEXT should be None when not set"
    );

    clear_config_env();
}

/// Config::load() must honour explicit environment overrides for every field.
#[test]
fn test_config_load_with_all_env_vars() {
    let _lock = ENV_MUTEX.lock().unwrap();
    clear_config_env();

    let secret = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef";
    unsafe {
        std::env::set_var("JWT_SECRET", secret);
        std::env::set_var("ADMIN_PASSWORD", "test-admin-pass-ok");
        std::env::set_var("HOST", "127.0.0.1");
        std::env::set_var("PORT", "8080");
        std::env::set_var("HUBBLE_ADDRESS", "hubble-relay:4245");
        std::env::set_var("K8S_CONTEXT", "my-cluster");
    }

    let config = Config::load().expect("Config::load should succeed");

    assert_eq!(config.host, "127.0.0.1");
    assert_eq!(config.port, 8080);
    assert_eq!(config.hubble_address, "hubble-relay:4245");
    assert_eq!(config.k8s_context.as_deref(), Some("my-cluster"));

    clear_config_env();
}

// ===========================================================================
// 2. Model serialization
// ===========================================================================

/// A Flow round-trips through JSON without data loss.
#[test]
fn test_flow_model_serialization() {
    let flow = Flow {
        id: "flow-123".to_string(),
        timestamp: "2025-01-15T10:30:00Z".to_string(),
        source: FlowEndpoint {
            namespace: "kube-system".to_string(),
            pod: "coredns-abc123".to_string(),
            ip: "10.0.0.1".to_string(),
        },
        destination: FlowEndpoint {
            namespace: "default".to_string(),
            pod: "frontend-xyz789".to_string(),
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
    };

    let restored: Flow = roundtrip_json(&flow);

    assert_eq!(restored.id, "flow-123");
    assert_eq!(restored.timestamp, "2025-01-15T10:30:00Z");
    assert_eq!(restored.source.namespace, "kube-system");
    assert_eq!(restored.source.pod, "coredns-abc123");
    assert_eq!(restored.source.ip, "10.0.0.1");
    assert_eq!(restored.destination.namespace, "default");
    assert_eq!(restored.destination.pod, "frontend-xyz789");
    assert_eq!(restored.destination.ip, "10.0.0.2");
    assert_eq!(restored.verdict, "FORWARDED");
    assert_eq!(restored.protocol, "TCP");
    assert_eq!(restored.port, 8080);
}

/// FlowEndpoint serializes to the expected JSON structure.
#[test]
fn test_flow_endpoint_json_keys() {
    let endpoint = FlowEndpoint {
        namespace: "production".to_string(),
        pod: "api-server-0".to_string(),
        ip: "172.16.0.5".to_string(),
    };

    let value = serde_json::to_value(&endpoint).expect("serialize endpoint");

    assert_eq!(value["namespace"], "production");
    assert_eq!(value["pod"], "api-server-0");
    assert_eq!(value["ip"], "172.16.0.5");
    // Ensure there are exactly 3 keys
    assert_eq!(
        value.as_object().unwrap().len(),
        3,
        "FlowEndpoint should serialize to exactly 3 JSON keys"
    );
}

/// FlowStats default values are all zero.
#[test]
fn test_flow_stats_default() {
    let stats = FlowStats::default();

    assert_eq!(stats.total_flows, 0);
    assert_eq!(stats.forwarded, 0);
    assert_eq!(stats.dropped, 0);
    assert!((stats.requests_per_second - 0.0).abs() < f64::EPSILON);
    assert!((stats.avg_latency_ms - 0.0).abs() < f64::EPSILON);
}

/// FlowStats round-trips through JSON without data loss.
#[test]
fn test_flow_stats_serialization() {
    let stats = FlowStats {
        total_flows: 42000,
        forwarded: 41500,
        dropped: 500,
        requests_per_second: 1234.56,
        avg_latency_ms: 2.75,
    };

    let restored: FlowStats = roundtrip_json(&stats);

    assert_eq!(restored.total_flows, 42000);
    assert_eq!(restored.forwarded, 41500);
    assert_eq!(restored.dropped, 500);
    assert!((restored.requests_per_second - 1234.56).abs() < f64::EPSILON);
    assert!((restored.avg_latency_ms - 2.75).abs() < f64::EPSILON);
}

/// A Policy round-trips through JSON without data loss.
#[test]
fn test_policy_model_serialization() {
    let policy = Policy {
        id: "policy-456".to_string(),
        name: "allow-dns".to_string(),
        namespace: "kube-system".to_string(),
        created_at: "2025-01-15T12:00:00Z".to_string(),
        status: "active".to_string(),
    };

    let restored: Policy = roundtrip_json(&policy);

    assert_eq!(restored.id, "policy-456");
    assert_eq!(restored.name, "allow-dns");
    assert_eq!(restored.namespace, "kube-system");
    assert_eq!(restored.created_at, "2025-01-15T12:00:00Z");
    assert_eq!(restored.status, "active");
}

/// Policy serializes to the expected JSON keys.
#[test]
fn test_policy_json_keys() {
    let policy = Policy {
        id: "p1".to_string(),
        name: "test".to_string(),
        namespace: "default".to_string(),
        created_at: "now".to_string(),
        status: "created".to_string(),
    };

    let value = serde_json::to_value(&policy).expect("serialize policy");
    let obj = value.as_object().expect("should be an object");

    assert!(obj.contains_key("id"));
    assert!(obj.contains_key("name"));
    assert!(obj.contains_key("namespace"));
    assert!(obj.contains_key("created_at"));
    assert!(obj.contains_key("status"));
    assert_eq!(
        obj.len(),
        5,
        "Policy should serialize to exactly 5 JSON keys"
    );
}

/// CreatePolicyRequest deserializes correctly from JSON input.
#[test]
fn test_create_policy_request_deserialization() {
    let input = json!({
        "name": "deny-egress",
        "namespace": "production",
        "spec": {
            "endpointSelector": {},
            "egressDeny": [{"toEndpoints": [{}]}]
        }
    });

    let req: CreatePolicyRequest =
        serde_json::from_value(input).expect("should deserialize CreatePolicyRequest");

    assert_eq!(req.name, "deny-egress");
    assert_eq!(req.namespace, "production");
    assert!(req.spec.is_object(), "spec should be a JSON object");
    assert!(
        req.spec.get("endpointSelector").is_some(),
        "spec should contain endpointSelector"
    );
}

/// CreatePolicyRequest must fail deserialization when required fields are absent.
#[test]
fn test_create_policy_request_missing_fields() {
    // Missing "spec"
    let input = json!({
        "name": "test",
        "namespace": "default"
    });
    let result = serde_json::from_value::<CreatePolicyRequest>(input);
    assert!(result.is_err(), "Should fail when 'spec' is missing");

    // Missing "name"
    let input = json!({
        "namespace": "default",
        "spec": {}
    });
    let result = serde_json::from_value::<CreatePolicyRequest>(input);
    assert!(result.is_err(), "Should fail when 'name' is missing");

    // Missing "namespace"
    let input = json!({
        "name": "test",
        "spec": {}
    });
    let result = serde_json::from_value::<CreatePolicyRequest>(input);
    assert!(result.is_err(), "Should fail when 'namespace' is missing");
}

/// A Flow list serializes and deserializes as a JSON array.
#[test]
fn test_flow_list_serialization() {
    let flows = vec![
        Flow {
            id: "f1".to_string(),
            timestamp: "t1".to_string(),
            source: FlowEndpoint {
                namespace: "ns1".to_string(),
                pod: "pod1".to_string(),
                ip: "1.1.1.1".to_string(),
            },
            destination: FlowEndpoint {
                namespace: "ns2".to_string(),
                pod: "pod2".to_string(),
                ip: "2.2.2.2".to_string(),
            },
            verdict: "FORWARDED".to_string(),
            protocol: "TCP".to_string(),
            port: 80,
            http_method: None,
            http_url: None,
            http_code: None,
            cluster: None,
            ..Default::default()
        },
        Flow {
            id: "f2".to_string(),
            timestamp: "t2".to_string(),
            source: FlowEndpoint {
                namespace: "ns3".to_string(),
                pod: "pod3".to_string(),
                ip: "3.3.3.3".to_string(),
            },
            destination: FlowEndpoint {
                namespace: "ns4".to_string(),
                pod: "pod4".to_string(),
                ip: "4.4.4.4".to_string(),
            },
            verdict: "DROPPED".to_string(),
            protocol: "UDP".to_string(),
            port: 53,
            http_method: None,
            http_url: None,
            http_code: None,
            cluster: None,
            ..Default::default()
        },
    ];

    let json_str = serde_json::to_string(&flows).expect("serialize flow list");
    let restored: Vec<Flow> = serde_json::from_str(&json_str).expect("deserialize flow list");

    assert_eq!(restored.len(), 2);
    assert_eq!(restored[0].id, "f1");
    assert_eq!(restored[1].id, "f2");
    assert_eq!(restored[0].verdict, "FORWARDED");
    assert_eq!(restored[1].verdict, "DROPPED");
}

// ===========================================================================
// 3. Error response formatting
// ===========================================================================

/// ApiError::NotFound must produce a 404 status code.
#[test]
fn test_api_error_not_found_status() {
    let error = ApiError::NotFound;
    let response = error.into_response();

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "NotFound should produce HTTP 404"
    );
}

/// ApiError::BadRequest must produce a 400 status code and include the message.
#[test]
fn test_api_error_bad_request_includes_message() {
    let error = ApiError::BadRequest("invalid namespace filter".to_string());
    let response = error.into_response();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "BadRequest should produce HTTP 400"
    );
}

/// ApiError::Unauthorized must produce a 401 status code.
#[test]
fn test_api_error_unauthorized_status() {
    let error = ApiError::Unauthorized("token expired".to_string());
    let response = error.into_response();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Unauthorized should produce HTTP 401"
    );
}

/// ApiError::Forbidden must produce a 403 status code.
#[test]
fn test_api_error_forbidden_status() {
    let error = ApiError::Forbidden;
    let response = error.into_response();

    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "Forbidden should produce HTTP 403"
    );
}

/// ApiError::Conflict must produce a 409 status code.
#[test]
fn test_api_error_conflict_status() {
    let error = ApiError::Conflict("policy already exists".to_string());
    let response = error.into_response();

    assert_eq!(
        response.status(),
        StatusCode::CONFLICT,
        "Conflict should produce HTTP 409"
    );
}

/// ApiError::InternalError must produce a 500 status code.
#[test]
fn test_api_error_internal_error_status() {
    let error = ApiError::InternalError("database connection lost".to_string());
    let response = error.into_response();

    assert_eq!(
        response.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "InternalError should produce HTTP 500"
    );
}

// ===========================================================================
// 4. Auth token (Claims) validation
// ===========================================================================

/// Claims struct can be created and its fields accessed.
#[test]
fn test_valid_jwt_claims() {
    let now = 1700000000usize;
    let claims = Claims {
        sub: "user-42".to_string(),
        exp: now + 3600,
        iat: now,
        role: "admin".to_string(),
        namespaces: vec![],
    };

    assert_eq!(claims.sub, "user-42");
    assert_eq!(claims.exp, now + 3600);
    assert_eq!(claims.iat, now);
    assert_eq!(claims.role, "admin");
}

/// Claims round-trips through JSON without data loss.
#[test]
fn test_claims_serialization() {
    let claims = Claims {
        sub: "service-account".to_string(),
        exp: 1700003600,
        iat: 1700000000,
        role: "viewer".to_string(),
        namespaces: vec![],
    };

    let restored: Claims = roundtrip_json(&claims);

    assert_eq!(restored.sub, "service-account");
    assert_eq!(restored.exp, 1700003600);
    assert_eq!(restored.iat, 1700000000);
    assert_eq!(restored.role, "viewer");
}

/// Claims serializes to the exact JSON keys expected by the JWT middleware.
#[test]
fn test_claims_json_keys() {
    let claims = Claims {
        sub: "u".to_string(),
        exp: 0,
        iat: 0,
        role: "r".to_string(),
        namespaces: vec![],
    };

    let value = serde_json::to_value(&claims).expect("serialize claims");
    let obj = value.as_object().expect("should be an object");

    assert!(obj.contains_key("sub"), "Claims must have 'sub' field");
    assert!(obj.contains_key("exp"), "Claims must have 'exp' field");
    assert!(obj.contains_key("iat"), "Claims must have 'iat' field");
    assert!(obj.contains_key("role"), "Claims must have 'role' field");
    assert!(
        obj.contains_key("namespaces"),
        "Claims must have 'namespaces' field"
    );
    assert_eq!(
        obj.len(),
        5,
        "Claims should serialize to exactly 5 JSON keys"
    );
}

/// Claims deserialization must fail when required fields are missing.
#[test]
fn test_claims_missing_fields() {
    let input = json!({"sub": "u", "exp": 0});
    let result = serde_json::from_value::<Claims>(input);
    assert!(
        result.is_err(),
        "Should fail when 'iat' and 'role' are missing"
    );
}

/// JWT encode/decode round-trip using jsonwebtoken crate to verify Claims
/// is compatible with the HS256 algorithm used by the auth middleware.
#[test]
fn test_jwt_encode_decode_roundtrip() {
    use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};

    let secret = "a]3kF9#mP!xQ7$wL2^rT5&vB8*nJ0dYcHgUeZsAiOlCbNfR";

    let claims = Claims {
        sub: "test-user".to_string(),
        exp: 9999999999, // Far future so the token does not expire during the test
        iat: 1700000000,
        role: "editor".to_string(),
        namespaces: vec![],
    };

    // Encode
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("JWT encoding must succeed");

    assert!(!token.is_empty(), "Encoded token must not be empty");

    // Decode
    let decoded = decode::<Claims>(
        &token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .expect("JWT decoding must succeed");

    assert_eq!(decoded.claims.sub, "test-user");
    assert_eq!(decoded.claims.role, "editor");
    assert_eq!(decoded.claims.iat, 1700000000);
}

/// Decoding a JWT with the wrong secret must fail.
#[test]
fn test_jwt_decode_wrong_secret() {
    use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};

    let claims = Claims {
        sub: "user".to_string(),
        exp: 9999999999,
        iat: 1700000000,
        role: "admin".to_string(),
        namespaces: vec![],
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(b"correct-secret-that-is-long-enough!!"),
    )
    .expect("JWT encoding must succeed");

    let result = decode::<Claims>(
        &token,
        &DecodingKey::from_secret(b"wrong-secret-that-is-also-long-enough!!"),
        &Validation::new(Algorithm::HS256),
    );

    assert!(result.is_err(), "Decoding with wrong secret must fail");
}

/// Decoding an expired JWT must fail.
#[test]
fn test_jwt_decode_expired_token() {
    use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};

    let secret = b"test-secret-for-expiry-check-32chars!";

    let claims = Claims {
        sub: "user".to_string(),
        exp: 1, // Expired in 1970
        iat: 0,
        role: "viewer".to_string(),
        namespaces: vec![],
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret),
    )
    .expect("JWT encoding must succeed");

    let result = decode::<Claims>(
        &token,
        &DecodingKey::from_secret(secret),
        &Validation::new(Algorithm::HS256),
    );

    assert!(result.is_err(), "Decoding an expired token must fail");
}
