// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Integration tests for policy generation and validation.
///
/// Validates that all generated CiliumNetworkPolicy YAML documents are
/// well-formed, contain the required Kubernetes fields, and that input
/// validation correctly rejects malformed data.
///
/// The `validate_k8s_name` and `validate_port` functions are imported
/// directly from the production code in `src/policies/mod.rs`.
use paqtra::policies::{validate_k8s_name, validate_port};

/// Generate an intra-namespace policy YAML string.
/// Mirrors the YAML template in `src/policies/mod.rs` `PolicyManager::apply_intra_namespace_policy`.
fn gen_intra_namespace_policy(namespace: &str) -> String {
    format!(
        r#"
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: allow-intra-namespace
  namespace: {namespace}
spec:
  endpointSelector: {{}}
  ingress:
    - fromEndpoints:
        - {{}}
"#
    )
}

/// Generate a DNS egress policy YAML string.
/// Mirrors the YAML template in `src/policies/mod.rs` `PolicyManager::apply_dns_policy`.
fn gen_dns_policy(namespace: &str) -> String {
    format!(
        r#"
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: allow-dns
  namespace: {namespace}
spec:
  endpointSelector: {{}}
  egress:
    - toEndpoints:
        - matchLabels:
            k8s-app: kube-dns
      toPorts:
        - ports:
            - port: "53"
              protocol: UDP
"#
    )
}

/// Generate a best-practice ingress policy YAML string.
/// Mirrors the YAML template in `src/policies/mod.rs` `PolicyManager::apply_best_practice_policy`.
fn gen_best_practice_policy(namespace: &str, from_app: &str, to_app: &str, port: u16) -> String {
    format!(
        r#"
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: allow-{from_app}-to-{to_app}
  namespace: {namespace}
spec:
  endpointSelector:
    matchLabels:
      app: {to_app}
  ingress:
    - fromEndpoints:
        - matchLabels:
            app: {from_app}
      toPorts:
        - ports:
            - port: "{port}"
              protocol: TCP
"#
    )
}

// ---------------------------------------------------------------------------
// Tests: YAML validity
// ---------------------------------------------------------------------------

#[test]
fn test_intra_namespace_policy_is_valid_yaml() {
    let yaml = gen_intra_namespace_policy("default");
    let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(&yaml);
    assert!(parsed.is_ok(), "Intra-namespace policy must be valid YAML");
}

#[test]
fn test_dns_policy_is_valid_yaml() {
    let yaml = gen_dns_policy("kube-system");
    let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(&yaml);
    assert!(parsed.is_ok(), "DNS policy must be valid YAML");
}

#[test]
fn test_best_practice_policy_is_valid_yaml() {
    let yaml = gen_best_practice_policy("prod", "web", "db", 5432);
    let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(&yaml);
    assert!(parsed.is_ok(), "Best-practice policy must be valid YAML");
}

// ---------------------------------------------------------------------------
// Tests: required Kubernetes fields
// ---------------------------------------------------------------------------

fn assert_has_k8s_fields(yaml_str: &str) {
    let doc: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
    let map = doc.as_mapping().expect("YAML must be a mapping");

    assert!(
        map.contains_key(&serde_yaml::Value::String("apiVersion".to_string())),
        "Policy YAML must contain apiVersion"
    );
    assert!(
        map.contains_key(&serde_yaml::Value::String("kind".to_string())),
        "Policy YAML must contain kind"
    );
    assert!(
        map.contains_key(&serde_yaml::Value::String("metadata".to_string())),
        "Policy YAML must contain metadata"
    );

    let api_version = map
        .get(&serde_yaml::Value::String("apiVersion".to_string()))
        .unwrap()
        .as_str()
        .expect("apiVersion must be a string");
    assert_eq!(api_version, "cilium.io/v2");

    let kind = map
        .get(&serde_yaml::Value::String("kind".to_string()))
        .unwrap()
        .as_str()
        .expect("kind must be a string");
    assert_eq!(kind, "CiliumNetworkPolicy");
}

#[test]
fn test_intra_namespace_policy_has_required_k8s_fields() {
    assert_has_k8s_fields(&gen_intra_namespace_policy("default"));
}

#[test]
fn test_dns_policy_has_required_k8s_fields() {
    assert_has_k8s_fields(&gen_dns_policy("kube-system"));
}

#[test]
fn test_best_practice_policy_has_required_k8s_fields() {
    assert_has_k8s_fields(&gen_best_practice_policy(
        "prod", "frontend", "backend", 8080,
    ));
}

// ---------------------------------------------------------------------------
// Tests: policy YAML structure (endpointSelector, ingress/egress)
// ---------------------------------------------------------------------------

#[test]
fn test_intra_namespace_policy_has_endpoint_selector_and_ingress() {
    let yaml = gen_intra_namespace_policy("test-ns");
    let doc: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let spec = doc.get("spec").expect("Policy must have spec");

    assert!(
        spec.get("endpointSelector").is_some(),
        "spec must contain endpointSelector"
    );
    assert!(
        spec.get("ingress").is_some(),
        "Intra-namespace policy must have ingress rules"
    );
}

#[test]
fn test_dns_policy_has_egress_rules() {
    let yaml = gen_dns_policy("app-ns");
    let doc: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let spec = doc.get("spec").expect("Policy must have spec");

    assert!(
        spec.get("egress").is_some(),
        "DNS policy must have egress rules"
    );

    // Verify port 53 appears in the egress rules
    let yaml_str = serde_yaml::to_string(&spec).unwrap();
    assert!(yaml_str.contains("53"), "DNS policy must reference port 53");
}

#[test]
fn test_best_practice_policy_has_ingress_with_port() {
    let yaml = gen_best_practice_policy("prod", "web", "api", 8080);
    let doc: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let spec = doc.get("spec").expect("Policy must have spec");

    assert!(
        spec.get("ingress").is_some(),
        "Best-practice policy must have ingress rules"
    );

    let yaml_str = serde_yaml::to_string(&spec).unwrap();
    assert!(yaml_str.contains("8080"), "Policy must contain port 8080");
    assert!(yaml_str.contains("TCP"), "Policy must specify TCP protocol");
}

// ---------------------------------------------------------------------------
// Tests: metadata namespace propagation
// ---------------------------------------------------------------------------

#[test]
fn test_policy_namespace_propagates_correctly() {
    for ns in &["default", "kube-system", "prod", "staging-123"] {
        let yaml = gen_intra_namespace_policy(ns);
        let doc: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        let metadata = doc.get("metadata").unwrap();
        let namespace = metadata.get("namespace").unwrap().as_str().unwrap();
        assert_eq!(namespace, *ns, "Namespace in metadata must match input");
    }
}

// ---------------------------------------------------------------------------
// Tests: input validation (RFC 1123 names) — uses production validate_k8s_name
// ---------------------------------------------------------------------------

#[test]
fn test_valid_k8s_names_accepted() {
    let valid_names = [
        "a",
        "test-ns",
        "prod",
        "my-app-123",
        "a1b2c3",
        "x",
        // Maximum 63 chars
        &"a".repeat(63),
    ];
    for name in valid_names {
        assert!(
            validate_k8s_name(name, "test").is_ok(),
            "Name '{}' should be valid",
            name
        );
    }
}

#[test]
fn test_empty_name_rejected() {
    let result = validate_k8s_name("", "namespace");
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("must not be empty"),
        "Error should mention empty"
    );
}

#[test]
fn test_uppercase_name_rejected() {
    assert!(validate_k8s_name("UPPER", "namespace").is_err());
    assert!(validate_k8s_name("MixedCase", "namespace").is_err());
}

#[test]
fn test_name_starting_with_dash_rejected() {
    assert!(validate_k8s_name("-starts-bad", "name").is_err());
}

#[test]
fn test_name_ending_with_dash_rejected() {
    assert!(validate_k8s_name("ends-bad-", "name").is_err());
}

#[test]
fn test_name_with_spaces_rejected() {
    assert!(validate_k8s_name("has spaces", "name").is_err());
}

#[test]
fn test_name_with_underscores_rejected() {
    assert!(validate_k8s_name("has_underscore", "name").is_err());
}

#[test]
fn test_name_with_injection_rejected() {
    assert!(validate_k8s_name("test'; echo pwned; #", "name").is_err());
}

#[test]
fn test_name_exceeding_63_chars_rejected() {
    let long_name = "a".repeat(64);
    let result = validate_k8s_name(&long_name, "namespace");
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("63 characters"),
        "Error should mention 63-char limit"
    );
}

#[test]
fn test_name_exactly_63_chars_accepted() {
    let name = "a".repeat(63);
    assert!(validate_k8s_name(&name, "namespace").is_ok());
}

// ---------------------------------------------------------------------------
// Tests: port validation — uses production validate_port
// ---------------------------------------------------------------------------

#[test]
fn test_port_zero_rejected() {
    assert!(validate_port(0).is_err());
}

#[test]
fn test_port_one_accepted() {
    assert!(validate_port(1).is_ok());
}

#[test]
fn test_common_ports_accepted() {
    for port in [80, 443, 8080, 3306, 5432, 6379, 9090] {
        assert!(
            validate_port(port).is_ok(),
            "Port {} should be accepted",
            port
        );
    }
}

#[test]
fn test_port_max_accepted() {
    assert!(validate_port(65535).is_ok());
}

// ---------------------------------------------------------------------------
// Tests: namespace validation
// ---------------------------------------------------------------------------

#[test]
fn test_standard_namespaces_valid() {
    for ns in &["default", "kube-system", "kube-public", "kube-node-lease"] {
        assert!(
            validate_k8s_name(ns, "namespace").is_ok(),
            "Standard namespace '{}' should be valid",
            ns
        );
    }
}

#[test]
fn test_dots_in_namespace_rejected() {
    // RFC 1123 labels disallow dots
    assert!(validate_k8s_name("my.namespace", "namespace").is_err());
}

// ---------------------------------------------------------------------------
// Tests: policy name generation is RFC 1123 compliant
// ---------------------------------------------------------------------------

#[test]
fn test_generated_policy_names_are_rfc1123_compliant() {
    // Check that the 'name' field in metadata of each generated policy passes
    // RFC 1123 validation.
    let policies = vec![
        gen_intra_namespace_policy("default"),
        gen_dns_policy("prod"),
        gen_best_practice_policy("prod", "web", "api", 8080),
    ];

    for yaml in policies {
        let doc: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        let name = doc["metadata"]["name"].as_str().unwrap();
        assert!(
            validate_k8s_name(name, "policy name").is_ok(),
            "Generated policy name '{}' must be RFC 1123 compliant",
            name
        );
    }
}
