// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

pub mod identity;

use anyhow::{Context, Result};
use k8s_openapi::api::core::v1::{ConfigMap, Namespace, Pod, ServiceAccount};
use k8s_openapi::api::rbac::v1::ClusterRoleBinding;
use kube::{Api, Client, Config};

pub use identity::{CacheStats, K8sIdentityResolver};

#[derive(Clone)]
pub struct K8sClient {
    client: Client,
    config: Config,
}

impl K8sClient {
    pub async fn new() -> Result<Self> {
        let config = Config::infer()
            .await
            .context("Failed to infer Kubernetes config")?;
        let client =
            Client::try_from(config.clone()).context("Failed to create Kubernetes client")?;

        Ok(Self { client, config })
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Create a mock K8sClient for testing (uses default kube config or fails gracefully)
    #[doc(hidden)]
    #[allow(dead_code)]
    pub fn mock() -> Self {
        // Create a minimal client for testing that doesn't require a real cluster
        let config = Config::new("https://localhost:6443".parse().expect("valid URL"));
        let client = Client::try_from(config.clone()).expect("mock client");
        Self { client, config }
    }

    pub async fn get_current_context(&self) -> Result<String> {
        let context = self.config.cluster_url.to_string();
        Ok(context)
    }

    pub async fn list_namespaces(&self) -> Result<Vec<String>> {
        let api: Api<Namespace> = Api::all(self.client.clone());
        let namespaces = api.list(&Default::default()).await?;

        Ok(namespaces
            .items
            .iter()
            .filter_map(|ns| ns.metadata.name.clone())
            .collect())
    }

    pub async fn list_pods(&self, namespace: &str) -> Result<Vec<Pod>> {
        let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
        let pods = api.list(&Default::default()).await?;
        Ok(pods.items)
    }

    pub async fn get_pods_by_label(
        &self,
        namespace: &str,
        label_selector: &str,
    ) -> Result<Vec<Pod>> {
        let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
        let lp = kube::api::ListParams::default().labels(label_selector);
        let pods = api.list(&lp).await?;
        Ok(pods.items)
    }

    pub async fn create_or_update_configmap(
        &self,
        namespace: &str,
        configmap: ConfigMap,
    ) -> Result<()> {
        let api: Api<ConfigMap> = Api::namespaced(self.client.clone(), namespace);
        let name = configmap
            .metadata
            .name
            .as_ref()
            .context("ConfigMap name missing")?;

        match api.get(name).await {
            Ok(_) => {
                api.replace(name, &Default::default(), &configmap).await?;
            }
            Err(kube::Error::Api(ae)) if ae.code == 404 => {
                api.create(&Default::default(), &configmap).await?;
            }
            Err(e) => return Err(e.into()),
        }

        Ok(())
    }

    pub async fn create_or_update_service_account(
        &self,
        namespace: &str,
        sa: ServiceAccount,
    ) -> Result<()> {
        let api: Api<ServiceAccount> = Api::namespaced(self.client.clone(), namespace);
        let name = sa
            .metadata
            .name
            .as_ref()
            .context("ServiceAccount name missing")?;

        match api.get(name).await {
            Ok(_) => {
                // Already exists, skip
            }
            Err(kube::Error::Api(ae)) if ae.code == 404 => {
                api.create(&Default::default(), &sa).await?;
            }
            Err(e) => return Err(e.into()),
        }

        Ok(())
    }

    pub async fn create_or_update_cluster_role_binding(
        &self,
        crb: ClusterRoleBinding,
    ) -> Result<()> {
        let api: Api<ClusterRoleBinding> = Api::all(self.client.clone());
        let name = crb
            .metadata
            .name
            .as_ref()
            .context("ClusterRoleBinding name missing")?;

        match api.get(name).await {
            Ok(_) => {
                // Already exists, skip
            }
            Err(kube::Error::Api(ae)) if ae.code == 404 => {
                api.create(&Default::default(), &crb).await?;
            }
            Err(e) => return Err(e.into()),
        }

        Ok(())
    }

    /// Annotate a pod with a key=value annotation
    pub async fn annotate_pod(&self, namespace: &str, pod: &str, annotation: &str) -> Result<()> {
        let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
        let parts: Vec<&str> = annotation.splitn(2, '=').collect();
        if parts.len() != 2 {
            anyhow::bail!("annotation must be in key=value format");
        }
        let patch = serde_json::json!({
            "metadata": {
                "annotations": {
                    parts[0]: parts[1]
                }
            }
        });
        api.patch(
            pod,
            &kube::api::PatchParams::apply("paqtra"),
            &kube::api::Patch::Merge(&patch),
        )
        .await?;
        Ok(())
    }

    /// Restart a deployment by patching the template annotation
    pub async fn restart_rollout(&self, namespace: &str, deployment: &str) -> Result<()> {
        use k8s_openapi::api::apps::v1::Deployment;
        let api: Api<Deployment> = Api::namespaced(self.client.clone(), namespace);
        let now = chrono::Utc::now().to_rfc3339();
        let patch = serde_json::json!({
            "spec": {
                "template": {
                    "metadata": {
                        "annotations": {
                            "kubectl.kubernetes.io/restartedAt": now
                        }
                    }
                }
            }
        });
        api.patch(
            deployment,
            &kube::api::PatchParams::apply("paqtra"),
            &kube::api::Patch::Merge(&patch),
        )
        .await?;
        Ok(())
    }

    /// Patch a configmap with a JSON merge patch
    pub async fn patch_configmap(
        &self,
        namespace: &str,
        name: &str,
        patch_json: &str,
    ) -> Result<()> {
        let api: Api<ConfigMap> = Api::namespaced(self.client.clone(), namespace);
        let patch: serde_json::Value =
            serde_json::from_str(patch_json).context("Invalid JSON patch for configmap")?;
        api.patch(
            name,
            &kube::api::PatchParams::apply("paqtra"),
            &kube::api::Patch::Merge(&patch),
        )
        .await?;
        Ok(())
    }

    pub async fn apply_custom_resource(&self, _namespace: Option<&str>, yaml: &str) -> Result<()> {
        // Validate the YAML is well-formed before passing to kubectl
        let parsed: serde_yaml::Value = serde_yaml::from_str(yaml)
            .context("Invalid YAML: refusing to apply malformed resource")?;

        // Basic validation: ensure it looks like a Kubernetes resource
        let mapping = parsed
            .as_mapping()
            .context("YAML must be a mapping (object)")?;

        if !mapping.contains_key(serde_yaml::Value::String("apiVersion".to_string())) {
            anyhow::bail!("YAML missing required field: apiVersion");
        }
        if !mapping.contains_key(serde_yaml::Value::String("kind".to_string())) {
            anyhow::bail!("YAML missing required field: kind");
        }

        // Allowlist of permitted resource kinds
        const ALLOWED_KINDS: &[&str] = &[
            "CiliumNetworkPolicy",
            "CiliumClusterwideNetworkPolicy",
            "NetworkPolicy",
            "CiliumExternalWorkload",
        ];

        let kind = mapping
            .get(serde_yaml::Value::String("kind".to_string()))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !ALLOWED_KINDS.contains(&kind) {
            anyhow::bail!(
                "Resource kind '{}' is not allowed. Permitted kinds: {}",
                kind,
                ALLOWED_KINDS.join(", ")
            );
        }

        // Apply via kubectl with --validate flag for server-side validation
        use tokio::io::AsyncWriteExt;
        use tokio::process::Command;

        let mut child = Command::new("kubectl")
            .args(["apply", "--validate=true", "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn kubectl")?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(yaml.as_bytes()).await?;
        }

        let output = child.wait_with_output().await?;
        if !output.status.success() {
            anyhow::bail!(
                "Failed to apply resource: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }
}
