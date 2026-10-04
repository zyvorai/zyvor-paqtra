#![allow(clippy::upper_case_acronyms)]
#![allow(dead_code)]

mod bootstrap;
mod cilium;
mod cli;
mod ebpf;
mod endpoints;
mod hubble;
mod integration;
mod kubernetes;
mod metrics;
mod modules;
mod policies;
mod tui;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::Level;

use bootstrap::BootstrapManager;
use cli::{ConnOpts, Global, InstallOpts, StatusOpts, SysdumpOpts, UninstallOpts};
use tui::TuiApp;

#[derive(Parser, Debug)]
#[command(name = "paqtra")]
#[command(about = "Paqtra — trace every flow. Network observability for Kubernetes")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(propagate_version = true)]
#[command(disable_help_subcommand = false)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(flatten)]
    global: Global,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Install Paqtra into a Kubernetes cluster (Helm; the chart is built in)
    // `--version` here selects the chart version, so clap's own flag is off.
    #[command(disable_version_flag = true)]
    Install(InstallArgs),
    /// Upgrade a Paqtra installation
    #[command(disable_version_flag = true)]
    Upgrade(UpgradeArgs),
    /// Uninstall Paqtra
    Uninstall {
        /// Wait for the resources to be deleted
        #[arg(long)]
        wait: bool,
        /// Also delete the release's PersistentVolumeClaims (flow history) and,
        /// if nothing else runs there, the namespace
        #[arg(long)]
        purge: bool,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Display status; exits non-zero when Paqtra, Cilium or Hubble Relay is unhealthy
    Status {
        /// Wait until components are ready (or the timeout passes)
        #[arg(long)]
        wait: bool,
        /// How long --wait waits (e.g. 90s, 5m)
        #[arg(long, default_value = "5m", value_name = "DURATION")]
        wait_duration: String,
        /// Output format: summary | json
        #[arg(short, long, default_value = "summary")]
        output: String,
        /// Also report this machine's BPF inventory and drift
        #[arg(long)]
        local: bool,
    },
    /// Show cluster / install info
    Info,
    /// Collect a redacted support bundle (zip): objects, logs, Helm values, doctor report
    Sysdump {
        /// Output file (default: paqtra-sysdump-<timestamp>.zip)
        #[arg(short = 'o', long, value_name = "FILE")]
        output_filename: Option<PathBuf>,
        /// Only logs from this long ago (e.g. 30m, 1h)
        #[arg(long, default_value = "1h", value_name = "DURATION")]
        since: String,
        /// Log lines per container
        #[arg(long, default_value_t = 2000)]
        log_lines: i64,
        /// Namespace Cilium runs in
        #[arg(long, default_value = "kube-system")]
        cilium_namespace: String,
        /// Leave out CiliumNetworkPolicies and CiliumNodes
        #[arg(long)]
        no_policies: bool,
    },
    /// Verify that Cilium enforces policy and that Paqtra sees the flows
    Connectivity {
        #[command(subcommand)]
        command: ConnectivityCommands,
    },
    /// Diagnose the installation: prerequisites, pods, agent coverage, ingest health
    Doctor {
        /// Output format: summary | json
        #[arg(short, long, default_value = "summary")]
        output: String,
    },
    /// Enable, disable or reach Hubble
    Hubble {
        #[command(subcommand)]
        command: HubbleCommands,
    },
    /// Open the Paqtra UI through a port-forward
    Ui {
        /// Local port
        #[arg(long, default_value_t = 8443)]
        port: u16,
        /// Do not open a browser
        #[arg(long)]
        no_open: bool,
    },
    /// View and change the release's Helm values
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Display feature discovery catalog (observe tiers)
    Features {
        /// Output format: summary | json
        #[arg(short, long, default_value = "summary")]
        output: String,
    },
    /// Read-only BPF inventory and drift (never attaches)
    Ebpf {
        #[command(subcommand)]
        command: EbpfCommands,
    },
    /// Per-second node, pod and Hubble metrics through the API (read-only)
    Metrics {
        /// Paqtra API base URL
        #[arg(
            long,
            env = "PAQTRA_API_URL",
            default_value = "http://127.0.0.1:9191",
            global = true
        )]
        api: String,
        /// Paqtra API token (env PAQTRA_API_TOKEN)
        #[arg(long, env = "PAQTRA_API_TOKEN", hide_env_values = true, global = true)]
        api_token: Option<String>,
        /// Output format: summary | json
        #[arg(short, long, default_value = "summary", global = true)]
        output: String,
        #[command(subcommand)]
        command: MetricsCommands,
    },
    /// Display client, release and server versions
    Version {
        /// Only print the client version (no cluster or helm access)
        #[arg(long)]
        client: bool,
        /// Output format: summary | json
        #[arg(short, long, default_value = "summary")]
        output: String,
    },
    /// Print a shell completion script (bash, zsh, fish, powershell, elvish)
    Completion { shell: Shell },
    /// Launch the interactive TUI
    Tui {
        #[arg(long)]
        skip_bootstrap: bool,
        #[arg(long, default_value = "4245")]
        hubble_port: u16,
        #[arg(long)]
        auto_install: bool,
        #[arg(long)]
        auto_upgrade: bool,
    },
    /// Run the node agent (DaemonSet entrypoint)
    Agent {
        /// Listen address for health endpoint
        #[arg(long, default_value = "0.0.0.0:9192")]
        listen: SocketAddr,
    },
}

/// Flags shared by `install` and `upgrade`.
#[derive(clap::Args, Debug)]
struct ReleaseArgs {
    /// Helm chart directory (default: the chart built into this binary)
    #[arg(long, value_name = "DIR")]
    chart_directory: Option<PathBuf>,
    /// Version to install (default: this CLI's version). Other versions are
    /// pulled from oci://ghcr.io/zyvorai/charts
    #[arg(long = "version", id = "chart_version", value_name = "X.Y.Z")]
    version: Option<String>,
    /// Helm values file (repeatable)
    #[arg(short = 'f', long = "values", value_name = "FILE")]
    values: Vec<PathBuf>,
    /// Helm --set key=value (repeatable)
    #[arg(long = "set", value_name = "KEY=VALUE")]
    set: Vec<String>,
    /// Helm --set-string key=value (repeatable)
    #[arg(long = "set-string", value_name = "KEY=VALUE")]
    set_string: Vec<String>,
    /// Helm --set-file key=path (repeatable)
    #[arg(long = "set-file", value_name = "KEY=PATH")]
    set_file: Vec<String>,
    /// Pull the images from a mirror: <prefix>/paqtra-api, <prefix>/paqtra-ui, <prefix>/paqtra
    #[arg(long, value_name = "PREFIX")]
    registry: Option<String>,
    /// Do not wait for the resources to become ready
    #[arg(long)]
    no_wait: bool,
    /// Kept for compatibility: waiting is the default
    #[arg(long, hide = true)]
    wait: bool,
    /// How long to wait (e.g. 90s, 5m)
    #[arg(long, default_value = "5m", value_name = "DURATION")]
    wait_duration: String,
    /// Render and validate against the cluster without changing anything
    #[arg(long)]
    dry_run: bool,
    /// Roll back automatically if the release does not become ready
    #[arg(long)]
    atomic: bool,
}

#[derive(clap::Args, Debug)]
struct InstallArgs {
    #[command(flatten)]
    release: ReleaseArgs,
    /// Install even if the prerequisite checks fail
    #[arg(long)]
    skip_preflight: bool,
    /// List released versions and exit
    #[arg(long)]
    list_versions: bool,
    /// Install Cilium (with Hubble + Relay + metrics) first if none is running
    #[arg(long)]
    with_cilium: bool,
    /// Cilium version for --with-cilium
    #[arg(long, default_value = cli::CILIUM_DEFAULT_VERSION, value_name = "X.Y.Z")]
    cilium_version: String,
    /// Extra Helm --set for the Cilium install (repeatable)
    #[arg(long = "cilium-set", value_name = "KEY=VALUE")]
    cilium_set: Vec<String>,
}

#[derive(clap::Args, Debug)]
struct UpgradeArgs {
    #[command(flatten)]
    release: ReleaseArgs,
    /// Discard the release's previous values instead of reusing them
    #[arg(long)]
    reset_values: bool,
}

impl ReleaseArgs {
    fn into_opts(self) -> InstallOpts {
        InstallOpts {
            chart_directory: self.chart_directory,
            version: self.version,
            values: self.values,
            set: self.set,
            set_string: self.set_string,
            set_file: self.set_file,
            registry: self.registry,
            wait: !self.no_wait,
            wait_duration: self.wait_duration,
            dry_run: self.dry_run,
            atomic: self.atomic,
            ..InstallOpts::default()
        }
    }
}

#[derive(Subcommand, Debug)]
enum HubbleCommands {
    /// Enable Hubble (and Relay, which Paqtra reads flows through) on the Cilium Helm release
    Enable {
        /// Do not enable Hubble Relay
        #[arg(long)]
        no_relay: bool,
        /// Hubble metrics, comma-separated (default: dns,drop,tcp,flow,icmp,http,policy)
        #[arg(long, value_name = "LIST")]
        metrics: Option<String>,
    },
    /// Disable Hubble on the Cilium Helm release
    Disable,
    /// Forward a local port to Hubble Relay
    PortForward {
        #[arg(long, default_value_t = cli::DEFAULT_RELAY_PORT)]
        port: u16,
        #[arg(long, default_value = "127.0.0.1")]
        address: std::net::IpAddr,
    },
}

#[derive(Subcommand, Debug)]
enum ConnectivityCommands {
    /// Deploy a temporary server and clients, apply a CiliumNetworkPolicy, and check who can connect
    Test {
        /// Run only this scenario (repeatable): baseline, enforcement, restore, flows
        #[arg(long = "test", value_name = "NAME")]
        tests: Vec<String>,
        /// How long to wait for the server and for each result
        #[arg(long, default_value = "3m", value_name = "DURATION")]
        timeout: String,
        /// Leave the test namespace in place
        #[arg(long)]
        no_cleanup: bool,
        /// Test image (default: agnhost from registry.k8s.io)
        #[arg(long, default_value = cli::CONNECTIVITY_IMAGE)]
        image: String,
        /// Pull the test image from a mirror: <prefix>/agnhost:<tag>
        #[arg(long, value_name = "PREFIX")]
        registry: Option<String>,
        /// Paqtra API token; enables the `flows` scenario (env PAQTRA_API_TOKEN)
        #[arg(long, env = "PAQTRA_API_TOKEN", hide_env_values = true)]
        api_token: Option<String>,
        /// Output format: summary | json
        #[arg(short, long, default_value = "summary")]
        output: String,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCommands {
    /// Print the release's Helm values
    View {
        /// Include chart defaults, not only what was set
        #[arg(long)]
        all: bool,
    },
    /// Print one value, e.g. `api.env.hubbleMode`
    Get { key: String },
    /// Change values (an upgrade that keeps everything else), e.g. `api.env.hubbleMode=auto`
    Set {
        #[arg(value_name = "KEY=VALUE")]
        assignments: Vec<String>,
        /// Do not wait for the rollout
        #[arg(long)]
        no_wait: bool,
    },
}

#[derive(Subcommand, Debug)]
enum EbpfCommands {
    /// List BPF programs with cilium|netra|other classification
    Attachments {
        #[arg(short, long, default_value = "summary")]
        output: String,
    },
    /// Warn-only brotherhood drift findings
    Drift {
        #[arg(short, long, default_value = "summary")]
        output: String,
    },
}

#[derive(Subcommand, Debug)]
enum MetricsCommands {
    /// Ingest, alert engine and exporter status
    Status,
    /// Nodes streaming metrics
    Nodes,
    /// List metric contexts
    Contexts {
        /// Substring filter on context or title
        #[arg(long, default_value = "")]
        filter: String,
        /// Comma-separated node globs
        #[arg(long, default_value = "")]
        nodes: String,
    },
    /// Query one context: last, average and max per dimension with a sparkline
    Query {
        context: String,
        /// Window: negative seconds (default the last 10 minutes)
        #[arg(long, default_value_t = -600, allow_hyphen_values = true)]
        after: i64,
        #[arg(long, default_value_t = 120)]
        points: usize,
        #[arg(long, default_value = "")]
        dimensions: String,
        #[arg(long, default_value = "")]
        nodes: String,
        #[arg(long, default_value = "")]
        charts: String,
        /// k=v,k2=v2 label globs
        #[arg(long, default_value = "")]
        labels: String,
        /// dimension, chart, node, instance, all or label:<key>
        #[arg(long, default_value = "")]
        group_by: String,
        /// avg, min, max, sum, last, p50, p90, p95, p99
        #[arg(long, default_value = "")]
        group: String,
    },
    /// CPU, memory, load and network per node right now
    Top,
    /// Anomaly rate per node and the most anomalous dimensions
    Anomalies {
        #[arg(long, default_value_t = -3600, allow_hyphen_values = true)]
        after: i64,
        #[arg(long, default_value_t = 20)]
        top: usize,
    },
    /// Raised metric alerts and recent transitions
    Alerts {
        /// Include clear and undefined instances
        #[arg(long)]
        all: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let log_level = if args.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };

    // Agent and status prefer quieter default logs unless verbose
    let quiet_cmds = matches!(
        args.command,
        None | Some(Commands::Status { .. })
            | Some(Commands::Info)
            | Some(Commands::Version { .. })
            | Some(Commands::Completion { .. })
            | Some(Commands::Features { .. })
            | Some(Commands::Ebpf { .. })
            | Some(Commands::Metrics { .. })
    );
    if !quiet_cmds || args.verbose {
        tracing_subscriber::fmt()
            .with_max_level(log_level)
            .with_target(false)
            .init();
    }

    match args.command {
        None => {
            cli::print_root_help();
            Ok(())
        }
        Some(Commands::Install(a)) => {
            let mut opts = a.release.into_opts();
            opts.skip_preflight = a.skip_preflight;
            opts.list_versions = a.list_versions;
            opts.with_cilium = a.with_cilium;
            opts.cilium_version = a.cilium_version;
            opts.cilium_set = a.cilium_set;
            cli::cmd_install(&args.global, opts).await
        }
        Some(Commands::Upgrade(a)) => {
            let mut opts = a.release.into_opts();
            opts.reset_values = a.reset_values;
            cli::cmd_upgrade(&args.global, opts).await
        }
        Some(Commands::Uninstall { wait, purge, yes }) => {
            cli::cmd_uninstall(&args.global, UninstallOpts { wait, purge, yes }).await
        }
        Some(Commands::Status {
            wait,
            wait_duration,
            output,
            local,
        }) => {
            let wait_duration = cli::parse_duration(&wait_duration)?;
            let ok = cli::cmd_status(
                &args.global,
                StatusOpts {
                    wait,
                    wait_duration,
                    output,
                    local,
                },
            )
            .await?;
            if !ok {
                std::process::exit(1);
            }
            Ok(())
        }
        Some(Commands::Info) => cli::cmd_info(&args.global).await,
        Some(Commands::Connectivity { command }) => match command {
            ConnectivityCommands::Test {
                tests,
                timeout,
                no_cleanup,
                image,
                registry,
                api_token,
                output,
            } => {
                let ok = cli::cmd_connectivity_test(
                    &args.global,
                    ConnOpts {
                        tests,
                        timeout: cli::parse_duration(&timeout)?,
                        cleanup: !no_cleanup,
                        image,
                        registry,
                        api_token,
                        output,
                    },
                )
                .await?;
                if !ok {
                    std::process::exit(1);
                }
                Ok(())
            }
        },
        Some(Commands::Sysdump {
            output_filename,
            since,
            log_lines,
            cilium_namespace,
            no_policies,
        }) => {
            cli::cmd_sysdump(
                &args.global,
                SysdumpOpts {
                    output: output_filename,
                    since: cli::parse_duration(&since)?,
                    log_lines,
                    cilium_namespace,
                    no_policies,
                },
            )
            .await
        }
        Some(Commands::Doctor { output }) => {
            let ok = cli::cmd_doctor(&args.global, &output).await?;
            if !ok {
                std::process::exit(1);
            }
            Ok(())
        }
        Some(Commands::Hubble { command }) => match command {
            HubbleCommands::Enable { no_relay, metrics } => {
                cli::cmd_hubble_enable(&args.global, no_relay, metrics).await
            }
            HubbleCommands::Disable => cli::cmd_hubble_disable(&args.global).await,
            HubbleCommands::PortForward { port, address } => {
                cli::cmd_hubble_port_forward(&args.global, port, address).await
            }
        },
        Some(Commands::Ui { port, no_open }) => cli::cmd_ui(&args.global, port, !no_open).await,
        Some(Commands::Config { command }) => match command {
            ConfigCommands::View { all } => cli::cmd_config_view(&args.global, all).await,
            ConfigCommands::Get { key } => cli::cmd_config_get(&args.global, &key).await,
            ConfigCommands::Set {
                assignments,
                no_wait,
            } => cli::cmd_config_set(&args.global, assignments, no_wait).await,
        },
        Some(Commands::Features { output }) => cli::cmd_features(&output),
        Some(Commands::Ebpf { command }) => match command {
            EbpfCommands::Attachments { output } => cli::cmd_ebpf_attachments(&output),
            EbpfCommands::Drift { output } => cli::cmd_ebpf_drift(&output),
        },
        Some(Commands::Metrics {
            api,
            api_token,
            output,
            command,
        }) => {
            use cli::metrics as m;
            let api = m::Api {
                url: api,
                token: api_token,
            };
            tokio::task::spawn_blocking(move || match command {
                MetricsCommands::Status => m::cmd_status(&api, &output),
                MetricsCommands::Nodes => m::cmd_nodes(&api, &output),
                MetricsCommands::Contexts { filter, nodes } => {
                    m::cmd_contexts(&api, &filter, &nodes, &output)
                }
                MetricsCommands::Query {
                    context,
                    after,
                    points,
                    dimensions,
                    nodes,
                    charts,
                    labels,
                    group_by,
                    group,
                } => m::cmd_query(
                    &api,
                    &m::QueryOpts {
                        context: &context,
                        after,
                        points,
                        dimensions: &dimensions,
                        nodes: &nodes,
                        charts: &charts,
                        labels: &labels,
                        group_by: &group_by,
                        group: &group,
                    },
                    &output,
                ),
                MetricsCommands::Top => m::cmd_top(&api, &output),
                MetricsCommands::Anomalies { after, top } => {
                    m::cmd_anomalies(&api, after, top, &output)
                }
                MetricsCommands::Alerts { all } => m::cmd_alerts(&api, all, &output),
            })
            .await?
        }
        Some(Commands::Version { client, output }) => {
            cli::cmd_version(&args.global, client, &output).await
        }
        Some(Commands::Completion { shell }) => {
            clap_complete::generate(
                shell,
                &mut Args::command(),
                "paqtra",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        Some(Commands::Tui {
            skip_bootstrap,
            hubble_port,
            auto_install,
            auto_upgrade,
        }) => run_tui(skip_bootstrap, hubble_port, auto_install, auto_upgrade).await,
        Some(Commands::Agent { listen }) => cli::run_agent(listen).await,
    }
}

async fn run_tui(
    skip_bootstrap: bool,
    hubble_port: u16,
    auto_install: bool,
    auto_upgrade: bool,
) -> Result<()> {
    let (context, hubble_port, k8s_client) = if skip_bootstrap {
        let k8s_client = kubernetes::K8sClient::new().await?;
        ("unknown".to_string(), hubble_port, k8s_client)
    } else {
        let bootstrap = BootstrapManager::new().await?;
        let k8s_client_clone = bootstrap.get_k8s_client();
        let result = bootstrap
            .run_bootstrap_with_options(auto_install, auto_upgrade)
            .await?;
        (result.context, result.hubble_port, k8s_client_clone)
    };

    let mut app = TuiApp::new(context, hubble_port, k8s_client).await?;
    app.run().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cli_definition_is_valid() {
        // clap only checks duplicate flags/ids at runtime; this catches them in CI.
        Args::command().debug_assert();
    }

    fn parse(line: &[&str]) -> Args {
        Args::try_parse_from(std::iter::once("paqtra").chain(line.iter().copied()))
            .unwrap_or_else(|e| panic!("{line:?} did not parse: {e}"))
    }

    #[test]
    fn global_flags_work_before_or_after_the_subcommand() {
        for line in [
            &[
                "--context",
                "prod",
                "-n",
                "obs",
                "--release",
                "px",
                "status",
            ][..],
            &[
                "status",
                "--context",
                "prod",
                "-n",
                "obs",
                "--release",
                "px",
            ][..],
        ] {
            let a = parse(line);
            assert_eq!(a.global.context.as_deref(), Some("prod"));
            assert_eq!(a.global.namespace, "obs");
            assert_eq!(a.global.release, "px");
        }
        let d = parse(&["status"]);
        assert_eq!(
            (d.global.namespace.as_str(), d.global.release.as_str()),
            ("paqtra", "paqtra")
        );
    }

    #[test]
    fn install_takes_a_chart_version_and_the_documented_flags() {
        let a = parse(&[
            "install",
            "--version",
            "2.0.0",
            "-f",
            "a.yaml",
            "-f",
            "b.yaml",
            "--set",
            "x=1",
            "--set-string",
            "y=2",
            "--registry",
            "r.io/z",
            "--no-wait",
            "--dry-run",
            "--atomic",
            "--skip-preflight",
            "--with-cilium",
            "--cilium-version",
            "1.20.2",
            "--cilium-set",
            "operator.replicas=1",
        ]);
        let Some(Commands::Install(i)) = a.command else {
            panic!("not install")
        };
        assert_eq!(i.release.version.as_deref(), Some("2.0.0"));
        assert_eq!(i.release.values.len(), 2);
        assert!(
            i.release.no_wait
                && i.release.dry_run
                && i.release.atomic
                && i.skip_preflight
                && i.with_cilium
        );
        assert_eq!(i.cilium_set, ["operator.replicas=1"]);
        let opts = i.release.into_opts();
        assert!(!opts.wait, "--no-wait turns waiting off");
    }

    #[test]
    fn the_old_wait_flag_is_still_accepted() {
        let Some(Commands::Install(i)) = parse(&["install", "--wait"]).command else {
            panic!()
        };
        assert!(i.release.into_opts().wait);
    }

    #[test]
    fn the_top_level_version_flag_still_prints_the_version() {
        let err = Args::try_parse_from(["paqtra", "--version"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn subcommands_parse() {
        for line in [
            &["upgrade", "--reset-values", "--version", "2.1.0"][..],
            &["uninstall", "--purge", "--yes"][..],
            &[
                "status",
                "--wait",
                "--wait-duration",
                "90s",
                "-o",
                "json",
                "--local",
            ][..],
            &["version", "--client"][..],
            &["doctor"][..],
            &[
                "connectivity",
                "test",
                "--test",
                "baseline",
                "--timeout",
                "90s",
                "--no-cleanup",
            ][..],
            &["doctor", "-o", "json"][..],
            &[
                "sysdump",
                "-o",
                "/tmp/x.zip",
                "--since",
                "30m",
                "--log-lines",
                "50",
                "--no-policies",
            ][..],
            &["completion", "zsh"][..],
            &["hubble", "enable", "--no-relay", "--metrics", "dns,drop"][..],
            &["hubble", "disable"][..],
            &["hubble", "port-forward", "--port", "4246"][..],
            &["ui", "--no-open", "--port", "9443"][..],
            &["config", "view", "--all"][..],
            &["config", "get", "api.env.hubbleMode"][..],
            &["config", "set", "a=1", "b=2", "--no-wait"][..],
        ] {
            parse(line);
        }
    }
}
