// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! A kube-rs client honouring `--context` / `--kubeconfig`, so cluster-facing
//! commands talk to the API directly instead of shelling out to `kubectl`.

use anyhow::{Context, Result};
use kube::config::{KubeConfigOptions, Kubeconfig};
use kube::{Client, Config};
use std::time::Duration;

use super::global::Global;

pub async fn client(g: &Global) -> Result<Client> {
    let options = KubeConfigOptions {
        context: g.context.clone(),
        ..Default::default()
    };
    let mut config = match &g.kubeconfig {
        Some(path) => {
            let kc = Kubeconfig::read_from(path)
                .with_context(|| format!("cannot read kubeconfig {}", path.display()))?;
            Config::from_custom_kubeconfig(kc, &options)
                .await
                .context("invalid kubeconfig")?
        }
        // No flags: the usual resolution ($KUBECONFIG, ~/.kube/config, in-cluster).
        None if g.context.is_none() => Config::infer()
            .await
            .context("no Kubernetes configuration found (is a kube-context set?)")?,
        None => Config::from_kubeconfig(&options).await.with_context(|| {
            format!(
                "cannot use context {:?}",
                g.context.as_deref().unwrap_or_default()
            )
        })?,
    };
    // Fail fast: a wrong context should not hang a command for minutes.
    config.connect_timeout = Some(Duration::from_secs(8));
    config.read_timeout = Some(Duration::from_secs(30));
    Client::try_from(config).context("cannot create a Kubernetes client")
}
