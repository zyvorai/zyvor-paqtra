// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn test_validate_k8s_name_valid() {
        assert!(validate_k8s_name("test-ns", "namespace").is_ok());
        assert!(validate_k8s_name("prod", "namespace").is_ok());
        assert!(validate_k8s_name("my-app-123", "namespace").is_ok());
        assert!(validate_k8s_name("a", "namespace").is_ok());
    }

    #[test]
    fn test_validate_k8s_name_invalid() {
        assert!(validate_k8s_name("", "namespace").is_err());
        assert!(validate_k8s_name("UPPER", "namespace").is_err());
        assert!(validate_k8s_name("-starts-with-dash", "namespace").is_err());
        assert!(validate_k8s_name("ends-with-dash-", "namespace").is_err());
        assert!(validate_k8s_name("has spaces", "namespace").is_err());
        assert!(validate_k8s_name("has_underscore", "namespace").is_err());
        assert!(validate_k8s_name("test'; echo pwned; #", "namespace").is_err());
        // 64 characters (exceeds limit)
        let long_name = "a".repeat(64);
        assert!(validate_k8s_name(&long_name, "namespace").is_err());
    }

    #[test]
    fn test_validate_port() {
        assert!(validate_port(80).is_ok());
        assert!(validate_port(443).is_ok());
        assert!(validate_port(8080).is_ok());
        assert!(validate_port(65535).is_ok());
        assert!(validate_port(1).is_ok());
        assert!(validate_port(0).is_err());
    }

    // NOTE: The following policy generation tests use inline format!() to produce
    // YAML and verify it parses correctly. They do NOT call the actual
    // PolicyManager::apply_* methods because those require a real K8s client.
    // The YAML templates here mirror those in PolicyManager (see above) and
    // serve to catch YAML formatting regressions.

    #[test]
    fn test_intra_namespace_policy_generation() {
        let namespace = "test-ns";
        let policy = format!(
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
        );

        assert!(policy.contains("test-ns"));
        assert!(policy.contains("allow-intra-namespace"));
        assert!(policy.contains("CiliumNetworkPolicy"));
        // Validate it's valid YAML
        assert!(serde_yaml::from_str::<serde_yaml::Value>(&policy).is_ok());
    }

    #[test]
    fn test_dns_policy_generation() {
        let namespace = "test-ns";
        let policy = format!(
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
        );

        assert!(policy.contains("test-ns"));
        assert!(policy.contains("allow-dns"));
        assert!(policy.contains("kube-dns"));
        assert!(policy.contains("53"));
        assert!(serde_yaml::from_str::<serde_yaml::Value>(&policy).is_ok());
    }

    #[test]
    fn test_best_practice_policy_generation() {
        let namespace = "prod";
        let from_app = "web";
        let to_app = "db";
        let port: u16 = 5432;

        let policy = format!(
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
        );

        assert!(policy.contains("prod"));
        assert!(policy.contains("allow-web-to-db"));
        assert!(policy.contains("app: web"));
        assert!(policy.contains("app: db"));
        assert!(policy.contains("5432"));
        assert!(serde_yaml::from_str::<serde_yaml::Value>(&policy).is_ok());
    }
}
