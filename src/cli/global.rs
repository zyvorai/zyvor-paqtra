// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Flags every cluster-facing command shares (like cilium-cli's `--context`,
//! `--namespace`, `--kubeconfig`). They are `global`, so both
//! `paqtra --context prod status` and `paqtra status --context prod` work.

use clap::Args;
use std::path::PathBuf;

#[derive(Args, Debug, Clone)]
pub struct Global {
    /// Kubernetes context to use (default: the current context)
    #[arg(long, global = true, env = "PAQTRA_CONTEXT", value_name = "NAME")]
    pub context: Option<String>,

    /// Path to a kubeconfig file (default: $KUBECONFIG, then ~/.kube/config)
    #[arg(long, global = true, value_name = "PATH")]
    pub kubeconfig: Option<PathBuf>,

    /// Namespace of the Paqtra release
    #[arg(
        short = 'n',
        long,
        global = true,
        env = "PAQTRA_NAMESPACE",
        default_value = "paqtra"
    )]
    pub namespace: String,

    /// Helm release name
    #[arg(long, global = true, env = "PAQTRA_RELEASE", default_value = "paqtra")]
    pub release: String,

    /// Use this `helm` binary instead of the one on PATH
    #[arg(long, global = true, env = "PAQTRA_HELM", value_name = "PATH")]
    pub helm_path: Option<PathBuf>,

    /// Never download helm; fail with instructions if it is not installed
    #[arg(long, global = true, env = "PAQTRA_NO_DOWNLOAD")]
    pub no_download_helm: bool,
}

impl Default for Global {
    fn default() -> Self {
        Self {
            context: None,
            kubeconfig: None,
            namespace: "paqtra".into(),
            release: "paqtra".into(),
            helm_path: None,
            no_download_helm: false,
        }
    }
}
