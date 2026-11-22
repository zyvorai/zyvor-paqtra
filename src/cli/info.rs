// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

use anyhow::Result;
use k8s_openapi::api::core::v1::{ConfigMap, Node};
use kube::api::ListParams;
use kube::config::Kubeconfig;
use kube::Api;
use owo_colors::OwoColorize;

use super::global::Global;

/// The kube-context in use: `--context`, else the kubeconfig's current one.
fn context_name(g: &Global) -> Option<String> {
    if let Some(c) = &g.context {
        return Some(c.clone());
    }
    let kc = match &g.kubeconfig {
        Some(p) => Kubeconfig::read_from(p).ok()?,
        None => Kubeconfig::read().ok()?,
    };
    kc.current_context
}

pub async fn cmd_info(g: &Global) -> Result<()> {
    println!("{}", "Paqtra Info".bold());
    println!();
    println!(
        "  {:20} {}",
        "CLI Version:".dimmed(),
        env!("CARGO_PKG_VERSION")
    );
    println!("  {:20} {}", "Release:".dimmed(), g.release);
    println!("  {:20} {}", "Namespace:".dimmed(), g.namespace);
    if let Some(ctx) = context_name(g) {
        println!("  {:20} {}", "Kube Context:".dimmed(), ctx);
    }

    let client = match super::kube::client(g).await {
        Ok(c) => c,
        Err(e) => {
            println!(
                "  {:20} {}",
                "Cluster:".dimmed(),
                format!("unreachable ({e:#})").red()
            );
            println!();
            return Ok(());
        }
    };

    let cms: Api<ConfigMap> = Api::namespaced(client.clone(), &g.namespace);
    if let Ok(Some(cm)) = cms.get_opt(&format!("{}-config", g.release)).await {
        if let Some(addr) = cm.data.and_then(|d| d.get("HUBBLE_ADDRESS").cloned()) {
            println!("  {:20} {}", "Hubble:".dimmed(), addr);
        }
    }

    let nodes: Api<Node> = Api::all(client.clone());
    if let Ok(list) = nodes.list(&ListParams::default()).await {
        println!("  {:20} {}", "Cluster Nodes:".dimmed(), list.items.len());
    }

    let pods: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &g.namespace);
    let selector = format!(
        "app.kubernetes.io/instance={},app.kubernetes.io/component=agent",
        g.release
    );
    if let Ok(list) = pods.list(&ListParams::default().labels(&selector)).await {
        println!("  {:20} {}", "Agent Pods:".dimmed(), list.items.len());
    }
    println!();
    Ok(())
}
