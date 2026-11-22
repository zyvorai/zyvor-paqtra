// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

use anyhow::Result;
use k8s_openapi::api::core::v1::Pod;
use std::collections::HashMap;

use crate::kubernetes::K8sClient;

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub name: String,
    pub namespace: String,
    pub ip: String,
    pub labels: HashMap<String, String>,
    pub status: EndpointStatus,
    pub node: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EndpointStatus {
    Running,
    Pending,
    Failed,
    Unknown,
}

impl std::fmt::Display for EndpointStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EndpointStatus::Running => write!(f, "Running"),
            EndpointStatus::Pending => write!(f, "Pending"),
            EndpointStatus::Failed => write!(f, "Failed"),
            EndpointStatus::Unknown => write!(f, "Unknown"),
        }
    }
}

pub struct EndpointManager {
    k8s_client: K8sClient,
}

impl EndpointManager {
    pub fn new(k8s_client: K8sClient) -> Self {
        Self { k8s_client }
    }

    pub async fn discover_endpoints(&self) -> Result<Vec<Endpoint>> {
        let namespaces = self.k8s_client.list_namespaces().await?;
        let mut endpoints = Vec::new();

        for ns in namespaces {
            let pods = self.k8s_client.list_pods(&ns).await?;
            for pod in pods {
                if let Some(endpoint) = self.pod_to_endpoint(pod) {
                    endpoints.push(endpoint);
                }
            }
        }

        Ok(endpoints)
    }

    pub async fn get_endpoints_by_namespace(&self, namespace: &str) -> Result<Vec<Endpoint>> {
        let pods = self.k8s_client.list_pods(namespace).await?;
        Ok(pods
            .into_iter()
            .filter_map(|pod| self.pod_to_endpoint(pod))
            .collect())
    }

    fn pod_to_endpoint(&self, pod: Pod) -> Option<Endpoint> {
        let name = pod.metadata.name?;
        let namespace = pod.metadata.namespace.unwrap_or_default();

        let ip = pod
            .status
            .as_ref()
            .and_then(|s| s.pod_ip.clone())
            .unwrap_or_default();

        let labels: HashMap<String, String> = pod
            .metadata
            .labels
            .unwrap_or_default()
            .into_iter()
            .collect();

        let status = pod
            .status
            .as_ref()
            .and_then(|s| s.phase.as_deref())
            .map(|phase| match phase {
                "Running" => EndpointStatus::Running,
                "Pending" => EndpointStatus::Pending,
                "Failed" => EndpointStatus::Failed,
                _ => EndpointStatus::Unknown,
            })
            .unwrap_or(EndpointStatus::Unknown);

        let node = pod
            .spec
            .as_ref()
            .and_then(|s| s.node_name.clone())
            .unwrap_or_default();

        Some(Endpoint {
            name,
            namespace,
            ip,
            labels,
            status,
            node,
        })
    }

    /// Returns (total_count, running_count) from a single discover_endpoints() call.
    pub async fn get_endpoint_counts(&self) -> Result<(usize, usize)> {
        let endpoints = self.discover_endpoints().await?;
        let total = endpoints.len();
        let running = endpoints
            .iter()
            .filter(|e| e.status == EndpointStatus::Running)
            .count();
        Ok((total, running))
    }

    pub async fn get_endpoint_count(&self) -> Result<usize> {
        let (total, _) = self.get_endpoint_counts().await?;
        Ok(total)
    }

    pub async fn get_running_count(&self) -> Result<usize> {
        let (_, running) = self.get_endpoint_counts().await?;
        Ok(running)
    }
}
