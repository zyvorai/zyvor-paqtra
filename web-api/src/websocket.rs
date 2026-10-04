// WebSocket handlers for real-time updates
use crate::middleware::auth::Claims;
use crate::services::hubble::LiveEvent;
use crate::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Query parameter for WebSocket token-based authentication.
#[derive(Debug, Deserialize)]
pub struct WsAuthQuery {
    pub token: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
}

/// Validate a JWT token from the WebSocket query string.
/// Returns Ok(()) if auth is disabled or the token is valid.
async fn validate_ws_token(
    state: &AppState,
    query: &WsAuthQuery,
) -> Result<(), (StatusCode, String)> {
    if state.config.auth_disabled {
        return Ok(());
    }
    let token = query.token.as_deref().ok_or((
        StatusCode::UNAUTHORIZED,
        "Missing 'token' query parameter for WebSocket authentication".to_string(),
    ))?;
    let decoding_key = DecodingKey::from_secret(state.config.jwt_secret.as_bytes());
    let validation = Validation::new(Algorithm::HS256);
    let claims = decode::<Claims>(token, &decoding_key, &validation)
        .map_err(|e| {
            tracing::debug!("WebSocket token validation failed: {}", e);
            (
                StatusCode::UNAUTHORIZED,
                "Invalid or expired token".to_string(),
            )
        })?
        .claims;
    // Same account checks as HTTP: a disabled or changed account is refused.
    let claims = crate::middleware::auth::resolve_claims(state, claims)
        .await
        .map_err(|msg| (StatusCode::UNAUTHORIZED, msg.to_string()))?;
    // The live streams carry cluster-wide flows and metrics and are not filtered
    // per namespace, so a namespace-limited account cannot open them.
    if claims.role != "admin" && !claims.namespaces.is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            "Live streams are not available to namespace-limited accounts".to_string(),
        ));
    }
    Ok(())
}

/// Maximum time without a pong before considering the connection dead
const PING_INTERVAL_SECS: u64 = 30;

/// Maximum concurrent WebSocket connections
const MAX_WS_CONNECTIONS: usize = 100;

/// Global counter of active WebSocket connections
static WS_CONNECTION_COUNT: AtomicUsize = AtomicUsize::new(0);

/// RAII guard that decrements the connection counter on drop
struct WsConnectionGuard;

impl WsConnectionGuard {
    fn try_acquire() -> Option<Self> {
        let mut current = WS_CONNECTION_COUNT.load(Ordering::Relaxed);
        loop {
            if current >= MAX_WS_CONNECTIONS {
                return None;
            }
            match WS_CONNECTION_COUNT.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(Self),
                Err(actual) => current = actual,
            }
        }
    }
}

impl Drop for WsConnectionGuard {
    fn drop(&mut self) {
        WS_CONNECTION_COUNT.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Send a ping; returns false if the client disconnected.
async fn ws_ping(socket: &mut WebSocket, label: &str) -> bool {
    if socket.send(Message::Ping(vec![].into())).await.is_err() {
        tracing::debug!("{} WebSocket client disconnected (ping failed)", label);
        return false;
    }
    true
}

/// Handle a received WebSocket message. Returns `true` to keep looping.
fn handle_ws_message(msg: Option<Result<Message, axum::Error>>, label: &str) -> bool {
    match msg {
        Some(Ok(Message::Close(_))) | None => {
            tracing::debug!("{} WebSocket client disconnected", label);
            false
        }
        Some(Ok(Message::Pong(_))) => true,
        Some(Err(e)) => {
            tracing::debug!("{} WebSocket error: {}", label, e);
            false
        }
        _ => true,
    }
}

pub async fn flows_websocket(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(auth_query): Query<WsAuthQuery>,
) -> Response {
    if let Err((status, msg)) = validate_ws_token(&state, &auth_query).await {
        return (status, msg).into_response();
    }
    let guard = match WsConnectionGuard::try_acquire() {
        Some(g) => g,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Too many WebSocket connections",
            )
                .into_response()
        }
    };
    ws.on_upgrade(move |socket| handle_flows_socket(socket, state, guard))
}

async fn handle_flows_socket(
    mut socket: WebSocket,
    state: Arc<AppState>,
    _guard: WsConnectionGuard,
) {
    tracing::info!("WebSocket connection established for flows");

    // Send initial message
    if let Err(e) = socket
        .send(Message::Text(
            serde_json::json!({"type": "connected", "message": "Flow stream ready"})
                .to_string()
                .into(),
        ))
        .await
    {
        tracing::error!("WebSocket send error: {}", e);
        return;
    }

    let mut ping_interval =
        tokio::time::interval(tokio::time::Duration::from_secs(PING_INTERVAL_SECS));
    let mut flow_interval = tokio::time::interval(tokio::time::Duration::from_secs(5));

    loop {
        tokio::select! {
            _ = flow_interval.tick() => {
                let summary = serde_json::json!({
                    "type": "flow_summary",
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "flows_fetched": state.metrics.flows_fetched.load(Ordering::Relaxed),
                    "hubble_queries": state.metrics.hubble_queries.load(Ordering::Relaxed),
                });
                if socket.send(Message::Text(summary.to_string().into())).await.is_err() {
                    tracing::debug!("Flow WebSocket client disconnected");
                    break;
                }
            }
            _ = ping_interval.tick() => {
                if !ws_ping(&mut socket, "Flow").await { break; }
            }
            msg = socket.recv() => {
                if !handle_ws_message(msg, "Flow") { break; }
            }
        }
    }
}

pub async fn metrics_websocket(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(auth_query): Query<WsAuthQuery>,
) -> Response {
    if let Err((status, msg)) = validate_ws_token(&state, &auth_query).await {
        return (status, msg).into_response();
    }
    let guard = match WsConnectionGuard::try_acquire() {
        Some(g) => g,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Too many WebSocket connections",
            )
                .into_response()
        }
    };
    ws.on_upgrade(move |socket| handle_metrics_socket(socket, state, guard))
}

async fn handle_metrics_socket(
    mut socket: WebSocket,
    state: Arc<AppState>,
    _guard: WsConnectionGuard,
) {
    tracing::info!("WebSocket connection established for metrics");

    let mut metrics_interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
    let mut ping_interval =
        tokio::time::interval(tokio::time::Duration::from_secs(PING_INTERVAL_SECS));

    loop {
        tokio::select! {
            _ = metrics_interval.tick() => {
                let m = &state.metrics;
                let metrics = serde_json::json!({
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "total_requests": m.total_requests.load(Ordering::Relaxed),
                    "total_errors": m.total_errors.load(Ordering::Relaxed),
                    "flows_fetched": m.flows_fetched.load(Ordering::Relaxed),
                    "cache_hits": m.cache_hits.load(Ordering::Relaxed),
                    "cache_misses": m.cache_misses.load(Ordering::Relaxed),
                    "hubble_queries": m.hubble_queries.load(Ordering::Relaxed),
                    "k8s_queries": m.k8s_queries.load(Ordering::Relaxed),
                });

                if socket.send(Message::Text(metrics.to_string().into())).await.is_err() {
                    tracing::debug!("Metrics WebSocket client disconnected");
                    break;
                }
            }
            _ = ping_interval.tick() => {
                if !ws_ping(&mut socket, "Metrics").await { break; }
            }
            msg = socket.recv() => {
                if !handle_ws_message(msg, "Metrics") { break; }
            }
        }
    }
}

pub async fn ws_live_flows(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WsAuthQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, (StatusCode, String)> {
    validate_ws_token(&state, &query).await?;

    let guard = match WsConnectionGuard::try_acquire() {
        Some(g) => g,
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "Too many WebSocket connections".to_string(),
            ));
        }
    };

    let namespace = query.namespace.clone();

    Ok(ws.on_upgrade(move |socket| handle_live_flows(socket, state, guard, namespace)))
}

async fn handle_live_flows(
    mut socket: WebSocket,
    state: Arc<AppState>,
    _guard: WsConnectionGuard,
    namespace: Option<String>,
) {
    tracing::info!(
        "Live flow WebSocket connection established (namespace: {:?})",
        namespace
    );

    // Send initial connected message
    if let Err(e) = socket
        .send(Message::Text(
            serde_json::json!({
                "type": "connected",
                "message": "Live flow stream ready",
                "namespace": namespace,
            })
            .to_string()
            .into(),
        ))
        .await
    {
        tracing::error!("Live flow WebSocket send error: {}", e);
        return;
    }

    let mut events = match state.hubble.stream_flows(namespace.as_deref()).await {
        Ok(rx) => rx,
        Err(e) => {
            tracing::error!("Failed to start live flow stream: {e:#}");
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({
                        "type": "error",
                        "message": format!("Failed to start flow stream: {e}"),
                    })
                    .to_string()
                    .into(),
                ))
                .await;
            return;
        }
    };

    let mut ping_interval =
        tokio::time::interval(tokio::time::Duration::from_secs(PING_INTERVAL_SECS));

    // Dropping `events` on any exit stops the gRPC stream or kills the CLI child.
    loop {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Some(LiveEvent::Flow(flow)) => {
                        let msg = serde_json::json!({
                            "type": "flow",
                            "data": flow,
                        });
                        if socket
                            .send(Message::Text(msg.to_string().into()))
                            .await
                            .is_err()
                        {
                            tracing::debug!("Live flow WebSocket client disconnected");
                            break;
                        }
                    }
                    Some(LiveEvent::Ended(reason)) => {
                        tracing::debug!("Live flow stream ended: {reason}");
                        let _ = socket
                            .send(Message::Text(
                                serde_json::json!({ "type": "error", "message": reason })
                                    .to_string()
                                    .into(),
                            ))
                            .await;
                        break;
                    }
                    None => break,
                }
            }
            _ = ping_interval.tick() => {
                if !ws_ping(&mut socket, "LiveFlow").await {
                    break;
                }
            }
            msg = socket.recv() => {
                if !handle_ws_message(msg, "LiveFlow") {
                    break;
                }
            }
        }
    }
}

/// `GET /api/v1/ws/metrics/live`: the client sends
/// `{"subscribe": [{id, context, charts, nodes, labels, window, points,
/// groupBy, group, aggregate}]}` (at most 50 queries) and receives
/// `{"results": {id: result}}` once a second.
#[derive(Debug, Deserialize)]
pub struct LiveMetricsAuth {
    pub token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveSub {
    id: String,
    context: String,
    #[serde(default)]
    charts: Vec<String>,
    #[serde(default)]
    nodes: Vec<String>,
    #[serde(default)]
    labels: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    window: i64,
    #[serde(default)]
    points: usize,
    #[serde(default)]
    group_by: String,
    #[serde(default)]
    group: String,
    #[serde(default)]
    aggregate: String,
}

#[derive(Debug, Deserialize)]
struct LiveSubscribe {
    subscribe: Vec<LiveSub>,
}

const MAX_LIVE_QUERIES: usize = 50;

pub async fn ws_live_metrics(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(q): Query<LiveMetricsAuth>,
) -> Response {
    let auth = WsAuthQuery {
        token: q.token,
        namespace: None,
    };
    if let Err((status, msg)) = validate_ws_token(&state, &auth).await {
        return (status, msg).into_response();
    }
    let guard = match WsConnectionGuard::try_acquire() {
        Some(g) => g,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Too many WebSocket connections",
            )
                .into_response()
        }
    };
    ws.on_upgrade(move |socket| handle_live_metrics(socket, state, guard))
}

fn live_query(s: &LiveSub) -> paqtra_metrics::tsdb::Query {
    let window = s.window.clamp(10, 3600);
    paqtra_metrics::tsdb::Query {
        context: s.context.clone(),
        charts: s.charts.clone(),
        nodes: s.nodes.clone(),
        labels: s.labels.clone(),
        after: -window,
        before: 0,
        points: if s.points == 0 {
            window.min(300) as usize
        } else {
            s.points.min(600)
        },
        group: s.group.clone(),
        group_by: s.group_by.clone(),
        aggregate: s.aggregate.clone(),
        ..Default::default()
    }
}

async fn handle_live_metrics(
    mut socket: WebSocket,
    state: Arc<AppState>,
    _guard: WsConnectionGuard,
) {
    let mut subs: Vec<LiveSub> = Vec::new();
    let mut tick = tokio::time::interval(tokio::time::Duration::from_secs(1));
    let mut ping = tokio::time::interval(tokio::time::Duration::from_secs(PING_INTERVAL_SECS));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if subs.is_empty() {
                    continue;
                }
                let (p, qs) = (state.metrics_platform.clone(), subs.clone());
                let res = tokio::task::spawn_blocking(move || {
                    let sources = p.sources();
                    let now = chrono::Utc::now().timestamp();
                    let mut results = serde_json::Map::new();
                    let mut error = None;
                    for s in &qs {
                        match paqtra_metrics::tsdb::run(&sources, live_query(s), now) {
                            Ok(r) => {
                                results.insert(s.id.clone(), serde_json::to_value(r).unwrap_or_default());
                            }
                            Err(e) => error = Some(e),
                        }
                    }
                    serde_json::json!({ "results": results, "error": error })
                })
                .await;
                let Ok(body) = res else { break };
                if socket.send(Message::Text(body.to_string().into())).await.is_err() {
                    break;
                }
            }
            _ = ping.tick() => {
                if !ws_ping(&mut socket, "LiveMetrics").await { break; }
            }
            msg = socket.recv() => {
                if let Some(Ok(Message::Text(t))) = &msg {
                    match serde_json::from_str::<LiveSubscribe>(t) {
                        Ok(s) => {
                            subs = s.subscribe.into_iter().filter(|q| !q.context.is_empty()).take(MAX_LIVE_QUERIES).collect();
                            tick.reset_immediately();
                        }
                        Err(e) => {
                            let _ = socket.send(Message::Text(serde_json::json!({ "error": format!("bad subscribe message: {e}") }).to_string().into())).await;
                        }
                    }
                    continue;
                }
                if !handle_ws_message(msg, "LiveMetrics") { break; }
            }
        }
    }
}
