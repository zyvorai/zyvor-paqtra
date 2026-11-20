// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::kubernetes::K8sClient;
use anyhow::{bail, Result};
use regex::Regex;
use std::sync::LazyLock;

#[cfg(test)]
mod tests;

/// Pre-compiled regex for RFC 1123 label validation.
/// Using `LazyLock` avoids recompiling on every call and removes the
/// runtime `unwrap()` that could theoretically panic in production.
static NAME_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9]([a-z0-9\-]*[a-z0-9])?$").expect("valid regex literal"));

pub struct PolicyManager {
    k8s_client: K8sClient,
}

/// Validate a Kubernetes resource name (RFC 1123 label).
/// Must be lowercase alphanumeric or '-', start/end with alphanumeric, max 63 chars.
pub fn validate_k8s_name(name: &str, field: &str) -> Result<()> {
    if name.is_empty() {
        bail!("{} must not be empty", field);
    }
    if name.len() > 63 {
        bail!("{} must be at most 63 characters", field);
    }
    if !NAME_REGEX.is_match(name) {
        bail!(
            "{} '{}' is invalid: must be lowercase alphanumeric or '-', \
             and must start and end with an alphanumeric character",
            field,
            name
        );
    }
    Ok(())
}

/// Validate a port number.
#[allow(dead_code)]
pub fn validate_port(port: u16) -> Result<()> {
    if port == 0 {
        bail!("Port must be between 1 and 65535");
    }
    Ok(())
}

impl PolicyManager {
    pub fn new(k8s_client: K8sClient) -> Self {
        Self { k8s_client }
    }

    pub async fn apply_intra_namespace_policy(&self, namespace: &str) -> Result<()> {
        validate_k8s_name(namespace, "namespace")?;

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

        self.k8s_client
            .apply_custom_resource(Some(namespace), &policy)
            .await?;
        Ok(())
    }

    pub async fn apply_dns_policy(&self, namespace: &str) -> Result<()> {
        validate_k8s_name(namespace, "namespace")?;

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
    - toEndpoints:
        - matchLabels:
            k8s-app: kube-dns
      toPorts:
        - ports:
            - port: "53"
              protocol: TCP
"#
        );

        self.k8s_client
            .apply_custom_resource(Some(namespace), &policy)
            .await?;
        Ok(())
    }

    pub async fn apply_hubble_policy(&self) -> Result<()> {
        let policy = r#"
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: allow-hubble
  namespace: kube-system
spec:
  endpointSelector:
    matchLabels:
      k8s-app: cilium
  ingress:
    - fromEndpoints:
        - matchLabels:
            app: paqtra
"#;

        self.k8s_client
            .apply_custom_resource(Some("kube-system"), policy)
            .await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn apply_best_practice_policy(
        &self,
        namespace: &str,
        from_app: &str,
        to_app: &str,
        port: u16,
    ) -> Result<()> {
        validate_k8s_name(namespace, "namespace")?;
        validate_k8s_name(from_app, "from_app")?;
        validate_k8s_name(to_app, "to_app")?;
        validate_port(port)?;

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

        self.k8s_client
            .apply_custom_resource(Some(namespace), &policy)
            .await?;
        Ok(())
    }
}
