// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Kubernetes service client
//
// This service wraps Kubernetes API interactions for the web API.
// Uses kubectl as a subprocess for policy management.

use crate::models::policy::{CreatePolicyRequest, Policy};
use anyhow::{Context, Result};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

/// Kubernetes resource name validation: RFC 1123 DNS subdomain
static K8S_NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9.\-]{0,252}$").unwrap());

const SA_TOKEN: &str = "/var/run/secrets/kubernetes.io/serviceaccount/token";
const SA_CA: &str = "/var/run/secrets/kubernetes.io/serviceaccount/ca.crt";

/// Validate a Kubernetes resource name
fn validate_k8s_name(name: &str, field: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("{} must not be empty", field);
    }
    if !K8S_NAME_RE.is_match(name) {
        anyhow::bail!(
            "{} '{}' is not a valid Kubernetes name (must match RFC 1123 DNS subdomain)",
            field,
            name
        );
    }
    // Reject names that look like kubectl flags
    if name.starts_with('-') {
        anyhow::bail!("{} must not start with '-'", field);
    }
    Ok(())
}

/// In-cluster API server URL + credentials from the mounted service account.
/// kubectl does not reliably auto-detect this when no kubeconfig exists (it
/// falls back to localhost:8080), so we pass flags explicitly.
fn incluster_api() -> Option<(String, String)> {
    let host = std::env::var("KUBERNETES_SERVICE_HOST").ok()?;
    let port = std::env::var("KUBERNETES_SERVICE_PORT").ok()?;
    if host.is_empty() || port.is_empty() {
        return None;
    }
    if !Path::new(SA_TOKEN).exists() || !Path::new(SA_CA).exists() {
        return None;
    }
    let token = std::fs::read_to_string(SA_TOKEN).ok()?;
    let token = token.trim().to_string();
    if token.is_empty() {
        return None;
    }
    Some((format!("https://{host}:{port}"), token))
}

pub struct K8sService {
    context: Option<String>,
}

impl K8sService {
    pub fn new(context: Option<String>) -> Self {
        Self { context }
    }

    pub fn context(&self) -> Option<&str> {
        self.context.as_deref()
    }

    /// Build a kubectl command with in-cluster SA auth (preferred) or `--context`.
    fn kubectl(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("kubectl");
        // Writable cache for readOnlyRootFilesystem pods (chart mounts emptyDir on /tmp).
        cmd.env("KUBECACHEDIR", "/tmp/kubectl-cache");

        if let Some((server, token)) = incluster_api() {
            cmd.arg(format!("--server={server}"));
            cmd.arg(format!("--certificate-authority={SA_CA}"));
            cmd.arg(format!("--token={token}"));
        } else if let Some(ctx) = &self.context {
            cmd.arg("--context").arg(ctx);
        }

        for arg in args {
            cmd.arg(arg);
        }
        cmd
    }

    /// Check if Kubernetes API is reachable via a lightweight authenticated call.
    pub async fn is_healthy(&self) -> bool {
        // Prefer /readyz; fall back to listing namespaces (works with narrow RBAC).
        let mut cmd = self.kubectl(&["get", "--raw=/readyz", "--request-timeout=3s"]);
        match timeout(Duration::from_secs(8), cmd.output()).await {
            Ok(Ok(out)) if out.status.success() => return true,
            _ => {}
        }
        let mut cmd = self.kubectl(&["get", "ns", "--request-timeout=3s"]);
        match timeout(Duration::from_secs(8), cmd.output()).await {
            Ok(Ok(out)) => out.status.success(),
            _ => false,
        }
    }

    /// Run a kubectl command and return the parsed JSON output.
    /// Returns an empty JSON object `{}` on failure.
    pub async fn kubectl_json(&self, args: &[&str]) -> serde_json::Value {
        let mut cmd = self.kubectl(args);
        match timeout(Duration::from_secs(15), cmd.output()).await {
            Ok(Ok(out)) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                serde_json::from_str(&stdout).unwrap_or(serde_json::json!({}))
            }
            Ok(Ok(out)) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                tracing::debug!("kubectl {:?} failed: {}", args.first(), stderr.trim());
                serde_json::json!({})
            }
            _ => serde_json::json!({}),
        }
    }

    /// Run an arbitrary CLI command and return stdout as a string.
    pub async fn run_cmd(program: &str, args: &[&str]) -> String {
        let mut cmd = Command::new(program);
        for a in args {
            cmd.arg(a);
        }
        match timeout(Duration::from_secs(10), cmd.output()).await {
            Ok(Ok(out)) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            }
            _ => String::new(),
        }
    }

    /// Run `args` inside the `cilium-agent` container of one Cilium agent pod
    /// and return stdout (empty on any failure, like [`Self::run_cmd`]).
    pub async fn exec_cilium_agent(&self, args: &[&str]) -> String {
        self.exec_cilium_agent_result(args).await.unwrap_or_default()
    }

    /// Like [`Self::exec_cilium_agent`], but says why it failed.
    ///
    /// `kubectl exec` has no label selector (`-l` is rejected), so a pod name is
    /// resolved first. Only that one node's agent answers: use it for status and
    /// samples, not cluster-wide totals.
    pub async fn exec_cilium_agent_result(&self, args: &[&str]) -> Result<String> {
        let mut get = self.kubectl(&[
            "get",
            "pods",
            "-n",
            "kube-system",
            "-l",
            "k8s-app=cilium",
            "-o",
            "jsonpath={.items[0].metadata.name}",
        ]);
        let out = timeout(Duration::from_secs(5), get.output())
            .await
            .context("timed out looking up a Cilium agent pod")?
            .context("kubectl not available")?;
        if !out.status.success() {
            anyhow::bail!(
                "cannot list Cilium agent pods: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let pod = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if pod.is_empty() {
            anyhow::bail!("no Cilium agent pod found (label k8s-app=cilium in kube-system)");
        }

        let mut exec_args = vec![
            "exec",
            "-n",
            "kube-system",
            pod.as_str(),
            "-c",
            "cilium-agent",
            "--",
        ];
        exec_args.extend_from_slice(args);
        let mut cmd = self.kubectl(&exec_args);
        let out = timeout(Duration::from_secs(15), cmd.output())
            .await
            .context("timed out running the command in the Cilium agent")?
            .context("kubectl not available")?;
        if !out.status.success() {
            anyhow::bail!(
                "command failed in the Cilium agent: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Run a `cilium-dbg` subcommand in the agent, falling back to the older
    /// `cilium` binary name (Cilium < 1.15 has no `cilium-dbg`).
    pub async fn cilium_dbg(&self, args: &[&str]) -> Result<String> {
        let mut with_dbg = vec!["cilium-dbg"];
        with_dbg.extend_from_slice(args);
        match self.exec_cilium_agent_result(&with_dbg).await {
            Ok(out) => Ok(out),
            Err(first) => {
                let mut legacy = vec!["cilium"];
                legacy.extend_from_slice(args);
                self.exec_cilium_agent_result(&legacy)
                    .await
                    .map_err(|second| anyhow::anyhow!("{first}; fallback: {second}"))
            }
        }
    }

    /// Run a CLI command with data piped to stdin. Returns (success, stdout, stderr).
    pub async fn run_cmd_stdin(
        program: &str,
        args: &[&str],
        stdin_data: &str,
    ) -> (bool, String, String) {
        use tokio::io::AsyncWriteExt;
        let mut cmd = Command::new(program);
        for a in args {
            cmd.arg(a);
        }
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(_) => return (false, String::new(), "Failed to spawn process".to_string()),
        };

        // We need to write to stdin before waiting, so split into parts
        let mut child = child;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(stdin_data.as_bytes()).await;
            drop(stdin);
        }

        match timeout(Duration::from_secs(10), child.wait_with_output()).await {
            Ok(Ok(out)) => {
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                (out.status.success(), stdout, stderr)
            }
            _ => (false, String::new(), "Command timed out".to_string()),
        }
    }

    /// List CiliumNetworkPolicy resources across all namespaces
    pub async fn list_policies(&self) -> Result<Vec<Policy>> {
        let mut cmd = self.kubectl(&[
            "get",
            "ciliumnetworkpolicies",
            "--all-namespaces",
            "-o",
            "json",
        ]);

        let output = timeout(Duration::from_secs(30), cmd.output())
            .await
            .context("kubectl list policies timed out after 30 seconds")?;

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let list: serde_json::Value =
                    serde_json::from_str(&stdout).context("Failed to parse kubectl JSON output")?;

                let policies = list
                    .get("items")
                    .and_then(|items| items.as_array())
                    .map(|items| items.iter().map(k8s_resource_to_policy).collect())
                    .unwrap_or_default();

                Ok(policies)
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                tracing::debug!("kubectl list policies failed: {}", stderr);
                Ok(Vec::new())
            }
            Err(e) => {
                tracing::debug!("kubectl not available: {}", e);
                Ok(Vec::new())
            }
        }
    }

    /// Fetch one CiliumNetworkPolicy object (spec, resourceVersion, ...).
    /// `Ok(None)` when it does not exist.
    pub async fn get_policy_object(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<serde_json::Value>> {
        validate_k8s_name(name, "policy name")?;
        validate_k8s_name(namespace, "namespace")?;

        let mut cmd = self.kubectl(&[
            "get",
            "ciliumnetworkpolicy",
            name,
            "-n",
            namespace,
            "-o",
            "json",
        ]);
        let out = timeout(Duration::from_secs(30), cmd.output())
            .await
            .context("kubectl get policy timed out after 30 seconds")?
            .context("kubectl not available")?;
        if out.status.success() {
            let obj = serde_json::from_slice(&out.stdout)
                .context("Failed to parse kubectl JSON output")?;
            return Ok(Some(obj));
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("NotFound") || stderr.contains("not found") {
            return Ok(None);
        }
        anyhow::bail!("kubectl get failed: {}", stderr.trim())
    }

    /// Create a CiliumNetworkPolicy from a request
    pub async fn create_policy(&self, req: &CreatePolicyRequest) -> Result<Policy> {
        self.apply_policy(req, &ApplyOptions::default()).await
    }

    /// `kubectl apply` a CiliumNetworkPolicy. With `resource_version` the API
    /// server rejects the write if the object changed meanwhile (see
    /// [`is_conflict`]); with `dry_run` nothing is persisted.
    pub async fn apply_policy(
        &self,
        req: &CreatePolicyRequest,
        opts: &ApplyOptions,
    ) -> Result<Policy> {
        validate_k8s_name(&req.name, "policy name")?;
        validate_k8s_name(&req.namespace, "namespace")?;

        let mut metadata = serde_json::json!({
            "name": req.name,
            "namespace": req.namespace,
        });
        if let Some(rv) = &opts.resource_version {
            metadata["resourceVersion"] = serde_json::Value::String(rv.clone());
        }
        let policy_manifest = serde_json::json!({
            "apiVersion": "cilium.io/v2",
            "kind": "CiliumNetworkPolicy",
            "metadata": metadata,
            "spec": req.spec,
        });

        let manifest_str = serde_json::to_string(&policy_manifest)?;

        let mut args = vec!["apply", "-f", "-"];
        if opts.dry_run {
            args.push("--dry-run=server");
        }
        let mut cmd = self.kubectl(&args);

        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn kubectl")?;

        // Write manifest to stdin
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(manifest_str.as_bytes()).await?;
            // drop stdin to close it
        }

        let output = timeout(Duration::from_secs(30), child.wait_with_output())
            .await
            .context("kubectl apply timed out after 30 seconds")??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("kubectl apply failed: {}", stderr);
        }

        Ok(Policy {
            id: uuid::Uuid::new_v4().to_string(),
            name: req.name.clone(),
            namespace: req.namespace.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
            status: if opts.dry_run { "dry-run" } else { "created" }.to_string(),
        })
    }

    /// Revert a Deployment to its previous revision (`kubectl rollout undo`).
    /// With `dry_run` the API server validates the request without changing
    /// anything. Returns kubectl's output.
    pub async fn rollout_undo(&self, namespace: &str, name: &str, dry_run: bool) -> Result<String> {
        validate_k8s_name(name, "deployment name")?;
        validate_k8s_name(namespace, "namespace")?;

        let target = format!("deployment/{}", name);
        let mut args = vec!["rollout", "undo", target.as_str(), "-n", namespace];
        if dry_run {
            args.push("--dry-run=server");
        }
        let mut cmd = self.kubectl(&args);

        let output = timeout(Duration::from_secs(30), cmd.output())
            .await
            .context("kubectl rollout undo timed out after 30 seconds")?
            .context("kubectl not available")?;
        if !output.status.success() {
            anyhow::bail!(
                "kubectl rollout undo failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Delete a CiliumNetworkPolicy by name (id is treated as "namespace/name")
    pub async fn delete_policy(&self, id: &str) -> Result<()> {
        // id format: "namespace/name" or just "name" (default namespace)
        let (namespace, name) = if let Some(pos) = id.find('/') {
            (&id[..pos], &id[pos + 1..])
        } else {
            ("default", id)
        };

        validate_k8s_name(name, "policy name")?;
        validate_k8s_name(namespace, "namespace")?;

        let mut cmd = self.kubectl(&["delete", "ciliumnetworkpolicy", name, "-n", namespace]);

        let output = timeout(Duration::from_secs(30), cmd.output())
            .await
            .context("kubectl delete timed out after 30 seconds")?;

        match output {
            Ok(out) if out.status.success() => Ok(()),
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                anyhow::bail!("kubectl delete failed: {}", stderr)
            }
            Err(e) => anyhow::bail!("kubectl not available: {}", e),
        }
    }
}

/// Knobs for [`K8sService::apply_policy`].
#[derive(Debug, Default, Clone)]
pub struct ApplyOptions {
    pub dry_run: bool,
    /// Optimistic-concurrency token: the resourceVersion the caller read.
    pub resource_version: Option<String>,
}

/// Whether an apply error is the API server's optimistic-concurrency conflict.
pub fn is_conflict(err: &anyhow::Error) -> bool {
    let msg = err.to_string();
    msg.contains("Conflict") || msg.contains("the object has been modified")
}

/// Extract a string field from a JSON value, returning `fallback` if absent.
fn json_str(v: &serde_json::Value, key: &str, fallback: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or(fallback)
        .to_string()
}

/// Convert a raw Kubernetes resource JSON into our Policy model
fn k8s_resource_to_policy(item: &serde_json::Value) -> Policy {
    let metadata = item.get("metadata").unwrap_or(item);

    let name = json_str(metadata, "name", "unknown");
    let namespace = json_str(metadata, "namespace", "default");
    let created_at = json_str(metadata, "creationTimestamp", "");
    let uid = json_str(metadata, "uid", "");

    let status = item
        .get("status")
        .and_then(|s| s.get("phase"))
        .and_then(|v| v.as_str())
        .unwrap_or("active")
        .to_string();

    Policy {
        id: uid,
        name,
        namespace,
        created_at,
        status,
    }
}
