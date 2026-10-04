#![recursion_limit = "256"]
// Paqtra Web API Server
mod config;
mod error;
pub mod handlers;
mod middleware;
mod models;
mod openapi;
mod services;
mod websocket;

use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::services::cache::CacheService;
use crate::services::hubble::HubbleService;
use crate::services::k8s::K8sService;
use crate::services::prometheus::PrometheusService;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install the default rustls CryptoProvider (required before any TLS operations)
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "paqtra_api=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    // Load configuration
    let config = Config::load()?;
    tracing::info!("Configuration loaded");

    // Initialize services
    let hubble = HubbleService::new(&config.hubble_address, config.hubble_addresses.clone())?
        .with_mode(config.hubble_mode);
    tracing::info!(
        "HubbleService initialized (relay: {}, clusters: {})",
        config.hubble_address,
        config
            .hubble_addresses
            .iter()
            .map(|(n, a)| format!("{}={}", n, a))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let k8s = K8sService::new(config.k8s_context.clone());
    tracing::info!("K8sService initialized (context: {:?})", config.k8s_context);

    let cache = match &config.data_dir {
        Some(dir) => {
            let db_path = std::path::Path::new(dir).join("paqtra.db");
            let cache = CacheService::with_persistence(&db_path)?;
            tracing::info!(
                "CacheService initialized with SQLite persistence at {}",
                db_path.display()
            );
            cache
        }
        None => {
            tracing::warn!(
                "PAQTRA_DATA_DIR not set: alert rules, audit log and other state are memory-only and will be lost on restart"
            );
            CacheService::new()
        }
    };

    let flow_store = match &config.data_dir {
        Some(dir) => {
            let store = services::flow_store::FlowStore::open(
                std::path::Path::new(dir),
                config.flow_retention_days,
            )?;
            tracing::info!("FlowStore initialized under {}", dir);
            Arc::new(store)
        }
        None => {
            tracing::warn!(
                "FlowStore running in-memory (set PAQTRA_DATA_DIR for durable flow index)"
            );
            Arc::new(services::flow_store::FlowStore::memory_only())
        }
    };

    let prometheus = PrometheusService::new(config.prometheus_url.clone());
    if prometheus.is_configured() {
        tracing::info!(
            "PrometheusService initialized (url: {:?})",
            config.prometheus_url
        );
    } else {
        tracing::info!("PrometheusService not configured (set PROMETHEUS_URL to enable)");
    }

    let metrics_platform = Arc::new(services::metrics_platform::MetricsPlatform::from_env(
        config.data_dir.as_deref(),
    )?);
    if metrics_platform.agent_key.is_none() && !config.auth_disabled {
        tracing::warn!(
            "PAQTRA_AGENT_KEY not set: agents cannot stream per-second metrics to this API"
        );
    }

    // Build shared application state
    let app_state = Arc::new(AppState {
        config: config.clone(),
        hubble,
        k8s,
        cache,
        flow_store,
        prometheus,
        metrics: AppMetrics::default(),
        metrics_platform,
    });

    // Per-second metrics: maintenance, Hubble-derived series, metric alerts, exporters
    services::metrics_platform::spawn_metrics_platform(app_state.clone());
    tracing::info!("Metrics platform started");

    // Start background export pipeline
    services::exporter::spawn_export_pipeline(app_state.clone());
    tracing::info!("Background export pipeline started");

    // Start background alerting engine
    services::alerting::spawn_alerting_engine(app_state.clone());
    tracing::info!("Background alerting engine started");

    // Start background GitOps change tracker
    services::change_tracker::spawn_change_tracker(app_state.clone());
    tracing::info!("Background change tracker started");

    // Cheap /health + /ready — live probes run on a sampler, not the request path
    handlers::health::spawn_health_sampler(app_state.clone());
    tracing::info!("Background health sampler started");

    // Declared connectivity path monitor (observe-only)
    services::connectivity::spawn_connectivity_monitor(app_state.clone());
    tracing::info!("Background connectivity monitor started");

    // Start Hubble → flow store ingest
    services::flow_ingest::spawn_flow_ingest(app_state.clone());
    tracing::info!("Background flow ingest started");

    // Start OpenTelemetry span exporter
    let span_exporter = Arc::new(services::tracing_svc::start_exporter(
        config.otel_endpoint.clone(),
        config.otel_service_name.clone(),
    ));
    if config.otel_endpoint.is_some() {
        tracing::info!(
            "OTEL span exporter started (endpoint: {:?}, service: {})",
            config.otel_endpoint,
            config.otel_service_name
        );
    } else {
        tracing::info!(
            "OTEL span exporter running in no-op mode (set OTEL_EXPORTER_ENDPOINT to enable export)"
        );
    }

    // Build API router
    let api_routes = Router::new()
        // Health checks (no auth required - handled by middleware)
        .route("/health", get(handlers::health::health_check))
        .route("/ready", get(handlers::health::readiness_check))
        // Login (no auth required)
        .route("/api/v1/auth/login", post(handlers::auth::login))
        .route("/api/v1/auth/me", get(handlers::auth::me))
        .route(
            "/api/v1/auth/password",
            post(handlers::auth::change_password),
        )
        .route(
            "/api/v1/users",
            get(handlers::users::list_users).post(handlers::users::create_user),
        )
        .route(
            "/api/v1/users/{username}",
            axum::routing::put(handlers::users::update_user).delete(handlers::users::delete_user),
        )
        // OpenAPI / Swagger UI (no auth required - handled by middleware)
        .route("/api-docs/openapi.json", get(openapi::openapi_json))
        .route("/swagger-ui", get(openapi::swagger_ui))
        // Flow monitoring
        .route("/api/v1/flows", get(handlers::flows::list_flows))
        .route(
            "/api/v1/flows/history",
            get(handlers::flow_history::flow_history),
        )
        .route(
            "/api/v1/flows/history/timeline",
            get(handlers::flow_history::flow_timeline),
        )
        .route("/api/v1/flows/{id}", get(handlers::flows::get_flow))
        .route("/api/v1/flows/stats", get(handlers::flows::flow_stats))
        // Policy management
        .route("/api/v1/policies", get(handlers::policies::list_policies))
        .route("/api/v1/policies", post(handlers::policies::create_policy))
        .route("/api/v1/policies/{id}", get(handlers::policies::get_policy))
        .route(
            "/api/v1/policies/{id}",
            axum::routing::put(handlers::policies::update_policy),
        )
        .route(
            "/api/v1/policies/{id}",
            axum::routing::delete(handlers::policies::delete_policy),
        )
        .route(
            "/api/v1/policies/{id}/rules",
            post(handlers::policies::add_policy_rule)
                .put(handlers::policies::edit_policy_rule)
                .delete(handlers::policies::delete_policy_rule),
        )
        .route(
            "/api/v1/policies/simulate",
            post(handlers::policies::simulate_policy),
        )
        // Hubble / Cilium observability
        .route(
            "/api/v1/hubble/nodes",
            get(handlers::cilium_obs::hubble_nodes),
        )
        .route(
            "/api/v1/hubble/metrics",
            get(handlers::cilium_obs::hubble_metrics),
        )
        .route(
            "/api/v1/cilium/metrics",
            get(handlers::cilium_obs::cilium_agent_metrics),
        )
        .route(
            "/api/v1/cilium/features",
            get(handlers::cilium_obs::cilium_features),
        )
        .route(
            "/api/v1/cilium/resources/{kind}",
            get(handlers::cilium_obs::cilium_resources),
        )
        .route(
            "/api/v1/cilium/agent/{what}",
            get(handlers::cilium_obs::agent_query),
        )
        // Path investigation (why can't A reach B?)
        .route(
            "/api/v1/investigate/path",
            post(handlers::investigate::investigate_path),
        )
        .route(
            "/api/v1/investigate/flow",
            post(handlers::investigate::investigate_flow),
        )
        .route(
            "/api/v1/investigate/bundles/{id}",
            get(handlers::investigate::get_bundle),
        )
        .route(
            "/api/v1/investigate/bundles/{id}/export",
            get(handlers::investigate::export_bundle),
        )
        .route(
            "/api/v1/investigate/bundles/{id}/share",
            post(handlers::investigate::share_bundle),
        )
        .route(
            "/api/v1/investigate/share/{token}",
            get(handlers::investigate::get_share),
        )
        // Declared connectivity paths (observe-only)
        .route(
            "/api/v1/connectivity/paths",
            get(handlers::connectivity::list_paths).post(handlers::connectivity::create_path),
        )
        .route(
            "/api/v1/connectivity/paths/{id}/status",
            get(handlers::connectivity::path_status),
        )
        .route(
            "/api/v1/connectivity/paths/{id}",
            axum::routing::delete(handlers::connectivity::delete_path),
        )
        .route(
            "/api/v1/connectivity/alerts",
            get(handlers::connectivity::list_alerts),
        )
        .route(
            "/api/v1/connectivity/alerts/{id}/silence",
            post(handlers::connectivity::silence_alert),
        )
        .route(
            "/api/v1/flows/store",
            get(handlers::flow_history::flow_store_info),
        )
        .route(
            "/api/v1/flows/store/purge",
            post(handlers::flow_history::flow_store_purge),
        )
        // Anomaly detection
        .route(
            "/api/v1/anomalies",
            get(handlers::anomalies::list_anomalies),
        )
        .route(
            "/api/v1/anomalies/{id}",
            get(handlers::anomalies::get_anomaly),
        )
        .route(
            "/api/v1/anomalies/{id}/remediate",
            post(handlers::anomalies::remediate_anomaly),
        )
        // Compliance
        .route(
            "/api/v1/compliance/frameworks",
            get(handlers::compliance::list_frameworks),
        )
        .route(
            "/api/v1/compliance/audit",
            post(handlers::compliance::run_audit),
        )
        .route(
            "/api/v1/compliance/audits",
            get(handlers::compliance::list_audits),
        )
        .route(
            "/api/v1/compliance/audits/{id}",
            get(handlers::compliance::get_audit),
        )
        .route(
            "/api/v1/compliance/audits/{id}/report",
            get(handlers::compliance::audit_report),
        )
        .route(
            "/api/v1/security/posture",
            get(handlers::compliance::security_posture),
        )
        // Intelligence modules
        .route(
            "/api/v1/modules/autopolicy/generate",
            post(handlers::modules::generate_autopolicy),
        )
        .route(
            "/api/v1/modules/chaos/experiments",
            get(handlers::modules::list_chaos_experiments),
        )
        .route(
            "/api/v1/modules/chaos/run",
            post(handlers::modules::run_chaos_experiment),
        )
        .route(
            "/api/v1/modules/canary/{id}",
            get(handlers::modules::canary_status),
        )
        // Events, Endpoints, Nodes
        .route("/api/v1/events", get(handlers::events::list_events))
        .route(
            "/api/v1/endpoints",
            get(handlers::endpoints::list_endpoints),
        )
        .route("/api/v1/nodes", get(handlers::nodes::list_nodes))
        // Replay
        .route(
            "/api/v1/modules/replay/recordings",
            get(handlers::extended::list_recordings),
        )
        .route(
            "/api/v1/modules/replay/start",
            post(handlers::extended::start_recording),
        )
        .route(
            "/api/v1/modules/replay/{id}/stop",
            post(handlers::extended::stop_recording),
        )
        // Healer
        .route(
            "/api/v1/modules/healer/problems",
            get(handlers::extended::list_healer_problems),
        )
        .route(
            "/api/v1/modules/healer/{id}/fix",
            post(handlers::extended::apply_healer_fix),
        )
        // RootCause
        .route(
            "/api/v1/modules/rootcause/drops",
            get(handlers::extended::list_packet_drops),
        )
        .route(
            "/api/v1/modules/rootcause/analyze",
            post(handlers::extended::analyze_drops),
        )
        // MultiCluster
        .route(
            "/api/v1/modules/multicluster/clusters",
            get(handlers::extended::list_clusters),
        )
        .route(
            "/api/v1/modules/multicluster/{name}/sync",
            post(handlers::extended::sync_cluster),
        )
        // Heatmap & Dependencies
        .route("/api/v1/heatmap", get(handlers::extended::heatmap_data))
        .route(
            "/api/v1/dependencies",
            get(handlers::extended::list_dependencies),
        )
        // Security Dashboard
        .route(
            "/api/v1/security/findings",
            get(handlers::extended::list_security_findings),
        )
        .route(
            "/api/v1/security/zero-trust",
            get(handlers::extended::zero_trust_score),
        )
        // eBPF Profiler (real kernel data via bpftool)
        .route(
            "/api/v1/ebpf/attachments",
            get(handlers::ebpf::list_attachments),
        )
        .route("/api/v1/ebpf/drift", get(handlers::ebpf::get_drift))
        .route(
            "/api/v1/ebpf/programs",
            get(handlers::ebpf::list_real_programs),
        )
        .route(
            "/api/v1/ebpf/programs/{id}",
            get(handlers::ebpf::get_program_stats),
        )
        .route("/api/v1/ebpf/maps", get(handlers::ebpf::list_real_maps))
        .route(
            "/api/v1/ebpf/maps/{id}/entries",
            get(handlers::ebpf::dump_map_entries),
        )
        .route("/api/v1/ebpf/conntrack", get(handlers::ebpf::get_conntrack))
        .route("/api/v1/ebpf/ipcache", get(handlers::ebpf::get_ipcache))
        .route("/api/v1/ebpf/lb", get(handlers::ebpf::get_lb_backends))
        .route("/api/v1/ebpf/drops", get(handlers::ebpf::get_drop_stats))
        .route(
            "/api/v1/ebpf/summary",
            get(handlers::ebpf::get_ebpf_summary),
        )
        // Metrics summary
        .route(
            "/api/v1/metrics/summary",
            get(handlers::extended::metrics_summary),
        )
        // Per-second metrics platform (agents stream, API stores and queries)
        .route(
            "/api/v1/agents/metrics",
            post(handlers::metrics_platform::ingest)
                .layer(DefaultBodyLimit::max(paqtra_metrics::wire::MAX_COMPRESSED)),
        )
        .route(
            "/api/v1/metrics/nodes",
            get(handlers::metrics_platform::nodes),
        )
        .route(
            "/api/v1/metrics/contexts",
            get(handlers::metrics_platform::contexts),
        )
        .route(
            "/api/v1/metrics/data",
            get(handlers::metrics_platform::data),
        )
        .route(
            "/api/v1/metrics/anomalies",
            get(handlers::metrics_platform::anomalies),
        )
        .route(
            "/api/v1/metrics/correlations",
            get(handlers::metrics_platform::correlations),
        )
        .route(
            "/api/v1/metrics/status",
            get(handlers::metrics_platform::status),
        )
        .route(
            "/api/v1/metrics/exporters",
            get(handlers::metrics_platform::exporters),
        )
        .route(
            "/api/v1/metrics/alerts",
            get(handlers::metrics_platform::alerts),
        )
        .route(
            "/api/v1/metrics/alerts/{id}/ack",
            post(handlers::metrics_platform::ack_alert),
        )
        .route(
            "/api/v1/metrics/silences",
            post(handlers::metrics_platform::create_silence),
        )
        .route(
            "/api/v1/metrics/silences/{id}",
            axum::routing::delete(handlers::metrics_platform::delete_silence),
        )
        .route("/api/v1/ws/metrics/live", get(websocket::ws_live_metrics))
        // Host Info
        .route("/api/v1/host/info", get(handlers::extended2::host_info))
        // Policy Templates
        .route(
            "/api/v1/policies/templates",
            get(handlers::extended2::list_policy_templates),
        )
        .route(
            "/api/v1/policies/templates/{id}/apply",
            post(handlers::extended2::apply_template),
        )
        // Diagnostics
        .route(
            "/api/v1/diagnostics/run",
            post(handlers::extended2::run_diagnostics),
        )
        .route(
            "/api/v1/diagnostics/connectivity",
            post(handlers::extended2::connectivity_test),
        )
        // Audit Log
        .route("/api/v1/audit/log", get(handlers::extended2::audit_log))
        // Alerts
        .route(
            "/api/v1/alerts/rules",
            get(handlers::extended2::list_alert_rules).post(handlers::extended2::create_alert_rule),
        )
        .route(
            "/api/v1/alerts/history",
            get(handlers::extended2::alert_history),
        )
        .route(
            "/api/v1/alerts/rules/{id}",
            axum::routing::put(handlers::extended2::toggle_alert_rule)
                .delete(handlers::extended2::delete_alert_rule),
        )
        .route(
            "/api/v1/alerts/rules/{id}/definition",
            axum::routing::put(handlers::extended2::update_alert_rule),
        )
        .route(
            "/api/v1/alerts/channels",
            get(handlers::notifications::list_channels)
                .post(handlers::notifications::create_channel),
        )
        .route(
            "/api/v1/alerts/channels/{id}",
            axum::routing::delete(handlers::notifications::delete_channel),
        )
        .route(
            "/api/v1/alerts/channels/{id}/test",
            post(handlers::notifications::test_channel),
        )
        .route(
            "/api/v1/alerts/silences",
            get(handlers::notifications::list_silences)
                .post(handlers::notifications::create_silence),
        )
        .route(
            "/api/v1/alerts/silences/{id}",
            axum::routing::delete(handlers::notifications::delete_silence),
        )
        // Service Map
        .route("/api/v1/servicemap", get(handlers::extended2::service_map))
        // Packet Capture
        .route(
            "/api/v1/modules/capture/sessions",
            get(handlers::extended2::list_capture_sessions),
        )
        .route(
            "/api/v1/modules/capture/start",
            post(handlers::extended2::start_capture),
        )
        .route(
            "/api/v1/modules/capture/{id}/stop",
            post(handlers::extended2::stop_capture),
        )
        // DNS Monitor
        .route("/api/v1/dns/queries", get(handlers::extended2::dns_queries))
        .route("/api/v1/dns/stats", get(handlers::extended2::dns_stats))
        // Identities
        .route(
            "/api/v1/identities",
            get(handlers::extended2::list_identities),
        )
        // Cluster Mesh
        .route(
            "/api/v1/clustermesh/peers",
            get(handlers::extended2::list_mesh_peers),
        )
        .route(
            "/api/v1/clustermesh/connect",
            post(handlers::extended2::connect_mesh_peer),
        )
        // BGP Peering
        .route(
            "/api/v1/bgp/peers",
            get(handlers::extended2::list_bgp_peers),
        )
        // Bandwidth
        .route(
            "/api/v1/bandwidth",
            get(handlers::extended2::bandwidth_data),
        )
        // Cost & Forecasting
        .route(
            "/api/v1/costs/breakdown",
            get(handlers::extended3::cost_breakdown),
        )
        .route(
            "/api/v1/forecast/metrics",
            get(handlers::extended3::forecast_metrics),
        )
        .route(
            "/api/v1/forecast/{metric}",
            get(handlers::extended3::forecast_data),
        )
        // Encryption
        .route(
            "/api/v1/encryption/status",
            get(handlers::extended3::encryption_status),
        )
        // Load Balancer & Ingress
        .route(
            "/api/v1/loadbalancer/services",
            get(handlers::extended3::lb_services),
        )
        .route(
            "/api/v1/ingress/routes",
            get(handlers::extended3::ingress_routes),
        )
        // IPAM
        .route("/api/v1/ipam/pools", get(handlers::extended3::ipam_pools))
        .route(
            "/api/v1/ipam/allocations",
            get(handlers::extended3::ip_allocations),
        )
        // Latency
        .route(
            "/api/v1/latency/analysis",
            get(handlers::extended3::latency_analysis),
        )
        // Traffic Mirroring
        .route(
            "/api/v1/modules/mirror/rules",
            get(handlers::extended3::mirror_rules).post(handlers::extended3::create_mirror_rule),
        )
        .route(
            "/api/v1/modules/mirror/rules/{id}",
            axum::routing::delete(handlers::extended3::delete_mirror_rule),
        )
        // Cluster Health & RBAC
        .route(
            "/api/v1/cluster/health",
            get(handlers::extended3::cluster_health),
        )
        .route(
            "/api/v1/rbac/bindings",
            get(handlers::extended3::rbac_bindings),
        )
        // Network Interfaces
        .route(
            "/api/v1/network/interfaces",
            get(handlers::extended3::net_interfaces),
        )
        // Troubleshoot
        .route(
            "/api/v1/troubleshoot/run",
            post(handlers::extended3::run_troubleshoot),
        )
        // WireGuard
        .route(
            "/api/v1/wireguard/peers",
            get(handlers::extended4::wireguard_peers),
        )
        // Cilium Status
        .route(
            "/api/v1/cilium/status",
            get(handlers::extended4::cilium_status),
        )
        // Policy Validation
        .route(
            "/api/v1/policies/validate",
            post(handlers::extended4::validate_policy),
        )
        // Flow Exports
        .route(
            "/api/v1/flows/exports",
            get(handlers::extended4::list_export_configs)
                .post(handlers::extended4::create_export_config),
        )
        .route(
            "/api/v1/flows/exports/{id}",
            axum::routing::delete(handlers::extended4::delete_export_config),
        )
        // SLOs
        .route(
            "/api/v1/slo/targets",
            get(handlers::slo_incidents::list_slos).post(handlers::slo_incidents::create_slo),
        )
        .route(
            "/api/v1/slo/targets/{id}",
            axum::routing::delete(handlers::slo_incidents::delete_slo),
        )
        // Incidents
        .route(
            "/api/v1/incidents",
            get(handlers::slo_incidents::list_incidents)
                .post(handlers::slo_incidents::create_incident),
        )
        .route(
            "/api/v1/incidents/{id}/ack",
            post(handlers::slo_incidents::ack_incident),
        )
        .route(
            "/api/v1/incidents/{id}/resolve",
            post(handlers::slo_incidents::resolve_incident),
        )
        .route(
            "/api/v1/incidents/{id}/notes",
            post(handlers::slo_incidents::add_incident_note),
        )
        // Changes
        .route("/api/v1/changes", get(handlers::extended4::list_changes))
        .route(
            "/api/v1/changes/{id}/impact",
            get(handlers::extended4::change_impact),
        )
        .route(
            "/api/v1/changes/{id}/rollback",
            post(handlers::extended4::rollback_change),
        )
        // Node Drain
        .route(
            "/api/v1/nodes/drain/status",
            get(handlers::extended4::node_drain_status),
        )
        .route("/api/v1/nodes/drain", post(handlers::extended4::drain_node))
        .route(
            "/api/v1/nodes/uncordon",
            post(handlers::extended4::uncordon_node),
        )
        // Pod Security
        .route(
            "/api/v1/security/pods",
            get(handlers::extended4::pod_security),
        )
        // Egress Gateway
        .route(
            "/api/v1/egress/policies",
            get(handlers::extended4::egress_policies),
        )
        // Service Mesh
        .route(
            "/api/v1/servicemesh/services",
            get(handlers::extended4::mesh_services),
        )
        // KubeProxy Replacement
        .route("/api/v1/kpr/status", get(handlers::extended4::kpr_status))
        // WebSocket endpoints
        .route("/api/v1/ws/flows", get(websocket::flows_websocket))
        .route("/api/v1/ws/flows/live", get(websocket::ws_live_flows))
        .route("/api/v1/ws/metrics", get(websocket::metrics_websocket))
        // Metrics endpoint for Prometheus (no auth required - handled by middleware)
        .route("/metrics", get(handlers::metrics::prometheus_metrics))
        // State
        .with_state(app_state.clone());

    // Serve the web UI static files as a fallback after API routes.
    // If UI_DIST_DIR is set and the directory exists, serve index.html for
    // all non-API paths (SPA client-side routing).
    let app = if let Some(ref ui_dir) = config.ui_dist_dir {
        let ui_path = std::path::PathBuf::from(ui_dir);
        if ui_path.join("index.html").exists() {
            tracing::info!("Serving web UI from {}", ui_dir);
            // Nest static file serving under "/" so it catches all non-API paths.
            // ServeDir serves real files (JS, CSS, images); the fallback serves
            // index.html for SPA client-side routes (e.g. /flows, /healer).
            let index_path = ui_path.join("index.html");
            let serve_dir = ServeDir::new(ui_dir).fallback(ServeFile::new(index_path));
            api_routes.fallback_service(serve_dir)
        } else {
            tracing::warn!("UI_DIST_DIR set to '{}' but index.html not found", ui_dir);
            api_routes
        }
    } else {
        api_routes
    };

    // Configure middleware (outermost layer runs first)
    //
    // Request flow order (outermost → innermost):
    //   TraceLayer → CORS → correlation_id → otel_trace → rate_limit → auth → compression → body limit → handler
    let app = app
        .layer(DefaultBodyLimit::max(1_048_576))
        .layer(CompressionLayer::new())
        .layer(axum::middleware::from_fn_with_state(
            app_state,
            middleware::auth::auth_middleware,
        ))
        .layer(axum::middleware::from_fn(
            middleware::rate_limit::rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn(
            middleware::otel_trace::otel_trace_middleware,
        ))
        .layer(axum::Extension(span_exporter))
        .layer(axum::middleware::from_fn(
            middleware::correlation::correlation_id_middleware,
        ))
        .layer(middleware::cors::cors_layer())
        .layer(TraceLayer::new_for_http());

    // Start server — with optional TLS
    if let (Some(cert_path), Some(key_path)) = (&config.tls_cert_path, &config.tls_key_path) {
        // ── HTTPS mode: TLS server + HTTP redirect ──────────────────
        let tls_port = config.tls_port;
        let tls_addr: SocketAddr = format!("{}:{}", config.host, tls_port).parse()?;
        let http_addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;

        // Build TLS config with HTTP/1.1 ALPN only (HTTP/2 doesn't support WebSocket upgrades)
        let tls_config = {
            use std::io::BufReader;
            let cert_data = std::fs::read(cert_path)?;
            let key_data = std::fs::read(key_path)?;
            let certs: Vec<_> = rustls_pemfile::certs(&mut BufReader::new(&cert_data[..]))
                .filter_map(|c| c.ok())
                .collect();
            let key = rustls_pemfile::private_key(&mut BufReader::new(&key_data[..]))?
                .ok_or_else(|| anyhow::anyhow!("No private key found in {}", key_path))?;
            let mut server_config = rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certs, key)?;
            // Only advertise HTTP/1.1 — this ensures WebSocket upgrades work over TLS
            server_config.alpn_protocols = vec![b"http/1.1".to_vec()];
            axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(server_config))
        };
        tracing::info!("TLS configured (cert={}, key={})", cert_path, key_path);

        // HTTP redirect router — sends all requests to HTTPS
        let redirect_tls_port = tls_port;
        let redirect_app = Router::new().fallback(
            move |req: axum::http::Request<axum::body::Body>| async move {
                let host_header = req
                    .headers()
                    .get(axum::http::header::HOST)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("localhost")
                    .to_owned();
                let host = host_header.split(':').next().unwrap_or(&host_header);
                let path = req.uri().path();
                let https_uri = format!("https://{}:{}{}", host, redirect_tls_port, path);
                axum::response::Redirect::permanent(&https_uri)
            },
        );

        let http_listener = tokio::net::TcpListener::bind(http_addr).await?;
        tracing::info!(
            "HTTP redirect server listening on {} -> https://...:{}",
            http_addr,
            tls_port
        );

        tracing::info!("Starting Paqtra API server (HTTPS) on {}", tls_addr);

        tokio::select! {
            res = axum_server::bind_rustls(tls_addr, tls_config)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>()) => {
                res?;
            }
            res = axum::serve(
                http_listener,
                redirect_app.into_make_service_with_connect_info::<SocketAddr>(),
            ).with_graceful_shutdown(shutdown_signal()) => {
                res?;
            }
        }
    } else {
        // ── Plain HTTP mode (unchanged behaviour) ───────────────────
        let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let actual_addr = listener.local_addr()?;
        tracing::info!("Starting Paqtra API server on {}", actual_addr);

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    }

    tracing::info!("Server shut down gracefully");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("Received Ctrl+C, shutting down"),
        _ = terminate => tracing::info!("Received SIGTERM, shutting down"),
    }
}

/// Application-level metrics tracked via lock-free atomic counters.
#[derive(Debug)]
pub struct AppMetrics {
    pub total_requests: AtomicU64,
    pub total_errors: AtomicU64,
    pub flows_fetched: AtomicU64,
    pub policies_created: AtomicU64,
    pub policies_deleted: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub hubble_queries: AtomicU64,
    pub k8s_queries: AtomicU64,
}

impl Default for AppMetrics {
    fn default() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            total_errors: AtomicU64::new(0),
            flows_fetched: AtomicU64::new(0),
            policies_created: AtomicU64::new(0),
            policies_deleted: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            hubble_queries: AtomicU64::new(0),
            k8s_queries: AtomicU64::new(0),
        }
    }
}

// Application state shared across handlers
pub struct AppState {
    pub config: Config,
    pub hubble: HubbleService,
    pub k8s: K8sService,
    pub cache: CacheService,
    pub flow_store: Arc<services::flow_store::FlowStore>,
    pub prometheus: PrometheusService,
    pub metrics: AppMetrics,
    pub metrics_platform: Arc<services::metrics_platform::MetricsPlatform>,
}
