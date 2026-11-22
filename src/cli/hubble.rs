// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! `paqtra hubble enable|disable|port-forward` and `paqtra ui`.

use anyhow::{bail, Result};
use owo_colors::OwoColorize;
use std::net::{IpAddr, SocketAddr};

use super::global::Global;
use super::{cilium, helm, portforward};

pub const DEFAULT_RELAY_PORT: u16 = 4245;
const UI_CONTAINER_PORT: u16 = 8443;

const NOT_HELM_HINT: &str = "Cilium is not a Helm release in kube-system, so paqtra cannot change it safely. \
    Enable Hubble the way Cilium was installed (e.g. `cilium hubble enable --ui=false` or your Helm values)";

pub async fn cmd_enable(g: &Global, no_relay: bool, metrics: Option<String>) -> Result<()> {
    change(g, true, !no_relay, metrics).await
}

pub async fn cmd_disable(g: &Global) -> Result<()> {
    change(g, false, false, None).await
}

async fn change(g: &Global, enable: bool, relay: bool, metrics: Option<String>) -> Result<()> {
    let helm = helm::ensure(g).await?;
    let Some(release) = cilium::find_release(&helm).await? else {
        bail!("{NOT_HELM_HINT}");
    };
    let version = cilium::installed_version(&release).to_string();
    println!(
        "{} {} Hubble{} on Cilium {} ...",
        "→".cyan(),
        if enable { "Enabling" } else { "Disabling" },
        if enable && relay { " and Relay" } else { "" },
        version.bold()
    );
    let metrics = metrics.or_else(|| enable.then(|| cilium::DEFAULT_HUBBLE_METRICS.to_string()));
    let args = cilium::hubble_args(enable, relay, metrics.as_deref(), &version, "5m")?;
    helm.run_ok(&args, "helm upgrade cilium").await?;
    println!(
        "{} Hubble {}.",
        "✔".green(),
        if enable { "enabled" } else { "disabled" }
    );
    Ok(())
}

pub async fn cmd_port_forward(g: &Global, port: u16, address: IpAddr) -> Result<()> {
    let client = super::kube::client(g).await?;
    let pod = portforward::find_pod(&client, cilium::NAMESPACE, "k8s-app=hubble-relay").await?;
    println!(
        "{} Forwarding {}:{} -> {}/{}:{} (Ctrl-C to stop)",
        "→".cyan(),
        address,
        port,
        cilium::NAMESPACE,
        pod,
        DEFAULT_RELAY_PORT
    );
    println!("  Use it with: HUBBLE_ADDRESS={address}:{port}");
    portforward::serve(
        client,
        cilium::NAMESPACE,
        &pod,
        DEFAULT_RELAY_PORT,
        SocketAddr::new(address, port),
    )
    .await
}

pub async fn cmd_ui(g: &Global, port: u16, open: bool) -> Result<()> {
    portforward::bail_if_privileged(port)?;
    let client = super::kube::client(g).await?;
    let selector = format!(
        "app.kubernetes.io/instance={},app.kubernetes.io/component=ui",
        g.release
    );
    let pod = portforward::find_pod(&client, &g.namespace, &selector).await?;
    let url = format!("https://127.0.0.1:{port}");
    println!(
        "{} Paqtra UI at {} (self-signed certificate; Ctrl-C to stop)",
        "→".cyan(),
        url.bold()
    );
    if open {
        portforward::open_browser(&url);
    }
    portforward::serve(
        client,
        &g.namespace,
        &pod,
        UI_CONTAINER_PORT,
        SocketAddr::new(IpAddr::from([127, 0, 0, 1]), port),
    )
    .await
}
