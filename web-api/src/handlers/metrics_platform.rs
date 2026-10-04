// Per-second metrics platform: agent ingest, queries, anomalies, metric
// alerts and exporter status. Everything except ingest, acks and silences is
// a read.

use super::{actor_from_claims, audit_log, check_editor, track_request};
use crate::services::metrics_platform::exporter_statuses;
use crate::AppState;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use paqtra_metrics::anomaly::{correlate, summarize};
use paqtra_metrics::metricalert::Silence;
use paqtra_metrics::stream::IngestError;
use paqtra_metrics::tsdb::{contexts as list_contexts, run, Query as MetricQuery};
use paqtra_metrics::wire::{decode, MAX_COMPRESSED};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use subtle::ConstantTimeEq;

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;
type Claims = Option<axum::Extension<crate::middleware::auth::Claims>>;

fn err(code: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<Value>) {
    (code, Json(json!({ "error": msg.into() })))
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn csv(s: &Option<String>) -> Vec<String> {
    s.as_deref()
        .unwrap_or("")
        .split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

/// `POST /api/v1/agents/metrics`: gzip JSON batches from node agents. Auth is
/// the shared agent key, not a user token.
pub async fn ingest(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult {
    let p = state.metrics_platform.clone();
    match p.agent_key.as_deref() {
        Some(key) => {
            let got = headers
                .get("x-paqtra-agent-key")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if !bool::from(got.as_bytes().ct_eq(key.as_bytes())) {
                return Err(err(StatusCode::UNAUTHORIZED, "invalid agent key"));
            }
        }
        None if state.config.auth_disabled => {}
        None => {
            return Err(err(
                StatusCode::UNAUTHORIZED,
                "metrics ingest needs PAQTRA_AGENT_KEY on the API and the agents",
            ))
        }
    }
    if body.len() > MAX_COMPRESSED {
        return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "batch too large"));
    }
    let gz = headers
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("gzip"));
    let res = tokio::task::spawn_blocking(move || {
        let batch = decode(&body, gz).map_err(IngestError::Invalid)?;
        p.hub.ingest(&batch)
    })
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    match res {
        Ok(r) => Ok(Json(serde_json::to_value(r).unwrap_or_default())),
        Err(IngestError::NodeLimit) => Err(err(
            StatusCode::TOO_MANY_REQUESTS,
            "metrics node limit reached",
        )),
        Err(IngestError::Invalid(e)) => Err(err(StatusCode::BAD_REQUEST, e)),
    }
}

pub async fn nodes(State(state): State<Arc<AppState>>) -> ApiResult {
    track_request(&state, |_| {}).await;
    let p = &state.metrics_platform;
    let mut nodes = serde_json::to_value(p.hub.nodes()).unwrap_or_default();
    if !p.cluster.list().is_empty() {
        if let Some(a) = nodes.as_array_mut() {
            a.push(json!({ "node": crate::services::metrics_platform::HUBBLE_NODE, "lastIngest": now(), "stats": p.cluster.stats(), "derived": true }));
        }
    }
    Ok(Json(json!({ "nodes": nodes })))
}

#[derive(Debug, Deserialize)]
pub struct ContextsQuery {
    pub nodes: Option<String>,
    pub q: Option<String>,
}

pub async fn contexts(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ContextsQuery>,
) -> ApiResult {
    track_request(&state, |_| {}).await;
    let p = state.metrics_platform.clone();
    let nodes = csv(&q.nodes);
    let filter = q.q.unwrap_or_default().to_lowercase();
    let mut cs = tokio::task::spawn_blocking(move || list_contexts(&p.sources(), &nodes))
        .await
        .unwrap_or_default();
    if !filter.is_empty() {
        cs.retain(|c| {
            c.context.to_lowercase().contains(&filter) || c.title.to_lowercase().contains(&filter)
        });
    }
    Ok(Json(json!({ "contexts": cs, "total": cs.len() })))
}

#[derive(Debug, Deserialize)]
pub struct DataQuery {
    pub context: String,
    pub charts: Option<String>,
    pub dimensions: Option<String>,
    pub nodes: Option<String>,
    /// `k=v,k2=v2`, values are globs.
    pub labels: Option<String>,
    pub after: Option<i64>,
    pub before: Option<i64>,
    pub points: Option<usize>,
    pub group: Option<String>,
    pub group_by: Option<String>,
    pub aggregate: Option<String>,
    pub tier: Option<usize>,
}

impl DataQuery {
    fn into_query(self) -> MetricQuery {
        let labels: BTreeMap<String, String> = csv(&self.labels)
            .into_iter()
            .filter_map(|kv| {
                kv.split_once('=')
                    .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            })
            .collect();
        MetricQuery {
            charts: csv(&self.charts),
            dimensions: csv(&self.dimensions),
            nodes: csv(&self.nodes),
            labels,
            after: self.after.unwrap_or(-600),
            before: self.before.unwrap_or(0),
            points: self.points.unwrap_or(300).min(5000),
            group: self.group.unwrap_or_default(),
            group_by: self.group_by.unwrap_or_default(),
            aggregate: self.aggregate.unwrap_or_default(),
            tier: self.tier,
            context: self.context,
        }
    }
}

pub async fn data(State(state): State<Arc<AppState>>, Query(q): Query<DataQuery>) -> ApiResult {
    track_request(&state, |_| {}).await;
    let p = state.metrics_platform.clone();
    let q = q.into_query();
    let res = tokio::task::spawn_blocking(move || run(&p.sources(), q, now()))
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    res.map(|r| Json(serde_json::to_value(r).unwrap_or_default()))
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

#[derive(Debug, Deserialize)]
pub struct AnomalyQuery {
    pub after: Option<i64>,
    pub before: Option<i64>,
    pub nodes: Option<String>,
    pub top: Option<usize>,
}

fn window(after: Option<i64>, before: Option<i64>) -> (i64, i64) {
    let n = now();
    let before = match before.unwrap_or(0) {
        b if b <= 0 => n + b,
        b => b,
    };
    let after = match after.unwrap_or(-3600) {
        a if a <= 0 => before + a,
        a => a,
    };
    (after.min(before - 1), before)
}

pub async fn anomalies(
    State(state): State<Arc<AppState>>,
    Query(q): Query<AnomalyQuery>,
) -> ApiResult {
    track_request(&state, |_| {}).await;
    let p = state.metrics_platform.clone();
    let (after, before) = window(q.after, q.before);
    let nodes = csv(&q.nodes);
    let top = q.top.unwrap_or(30).min(500);
    let out = tokio::task::spawn_blocking(move || {
        let sum = summarize(&p.sources(), &nodes, after, before, top, now());
        json!({ "summary": sum, "detector": p.detector.stats() })
    })
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(out))
}

/// Ranks dimensions whose distribution in the window differs most from the
/// four windows before it (two-sample KS).
pub async fn correlations(
    State(state): State<Arc<AppState>>,
    Query(q): Query<AnomalyQuery>,
) -> ApiResult {
    track_request(&state, |_| {}).await;
    let p = state.metrics_platform.clone();
    let (after, before) = window(q.after.or(Some(-300)), q.before);
    let nodes = csv(&q.nodes);
    let top = q.top.unwrap_or(30).min(500);
    let ranked = tokio::task::spawn_blocking(move || {
        correlate(&p.sources(), &nodes, after, before, top, now())
    })
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(
        json!({ "after": after, "before": before, "ranked": ranked }),
    ))
}

pub async fn status(State(state): State<Arc<AppState>>) -> ApiResult {
    track_request(&state, |_| {}).await;
    let p = &state.metrics_platform;
    Ok(Json(json!({
        "nodes": p.hub.nodes().len(),
        "ingestAuth": if p.agent_key.is_some() { "agent-key" } else if state.config.auth_disabled { "open (auth disabled)" } else { "closed (PAQTRA_AGENT_KEY unset)" },
        "hubble": p.hubble_stats(),
        "detector": p.detector.stats(),
        "alerts": p.alerts.as_ref().map(|a| a.snapshot(false, 1).stats),
        "alertsError": p.alerts_error,
        "exporters": exporter_statuses(p),
    })))
}

#[derive(Debug, Deserialize)]
pub struct AlertsQuery {
    pub all: Option<bool>,
    pub history: Option<usize>,
}

fn engine(
    state: &AppState,
) -> Result<Arc<paqtra_metrics::metricalert::Engine>, (StatusCode, Json<Value>)> {
    state.metrics_platform.alerts.clone().ok_or_else(|| {
        err(
            StatusCode::SERVICE_UNAVAILABLE,
            state
                .metrics_platform
                .alerts_error
                .clone()
                .unwrap_or_else(|| "metric alerts are disabled (PAQTRA_METRICALERT=false)".into()),
        )
    })
}

pub async fn alerts(State(state): State<Arc<AppState>>, Query(q): Query<AlertsQuery>) -> ApiResult {
    track_request(&state, |_| {}).await;
    let e = engine(&state)?;
    let snap = e.snapshot(q.all.unwrap_or(false), q.history.unwrap_or(200).min(1000));
    Ok(Json(serde_json::to_value(snap).unwrap_or_default()))
}

pub async fn ack_alert(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Path(id): Path<String>,
) -> ApiResult {
    check_editor(&state, &claims)?;
    let e = engine(&state)?;
    let actor = actor_from_claims(&claims);
    e.ack(&id, &actor)
        .map_err(|m| err(StatusCode::NOT_FOUND, format!("alert {m}")))?;
    audit_log(
        &state,
        "metric_alert_ack",
        &id,
        "",
        "acknowledged",
        &actor,
        "success",
    )
    .await;
    Ok(Json(json!({ "acked": id })))
}

#[derive(Debug, Deserialize)]
pub struct SilenceRequest {
    #[serde(default)]
    pub rule: String,
    #[serde(default)]
    pub node: String,
    #[serde(default)]
    pub chart: String,
    /// Seconds from now.
    pub duration: Option<i64>,
    /// Unix seconds; used when `duration` is absent.
    pub until: Option<i64>,
    #[serde(default)]
    pub comment: String,
}

pub async fn create_silence(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Json(req): Json<SilenceRequest>,
) -> ApiResult {
    check_editor(&state, &claims)?;
    let e = engine(&state)?;
    let n = now();
    let until = req
        .duration
        .map(|d| n + d)
        .or(req.until)
        .unwrap_or(n + 3600);
    let actor = actor_from_claims(&claims);
    let s = e
        .add_silence(
            Silence {
                rule: req.rule,
                node: req.node,
                chart: req.chart,
                until,
                comment: req.comment,
                created_by: actor.clone(),
                ..Default::default()
            },
            n,
        )
        .map_err(|m| err(StatusCode::BAD_REQUEST, m))?;
    audit_log(
        &state,
        "metric_alert_silence",
        &s.id,
        "",
        &format!(
            "rule={} node={} chart={} until={}",
            s.rule, s.node, s.chart, s.until
        ),
        &actor,
        "success",
    )
    .await;
    Ok(Json(serde_json::to_value(s).unwrap_or_default()))
}

pub async fn delete_silence(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Path(id): Path<String>,
) -> ApiResult {
    check_editor(&state, &claims)?;
    let e = engine(&state)?;
    e.delete_silence(&id)
        .map_err(|m| err(StatusCode::NOT_FOUND, format!("silence {m}")))?;
    audit_log(
        &state,
        "metric_alert_unsilence",
        &id,
        "",
        "deleted",
        &actor_from_claims(&claims),
        "success",
    )
    .await;
    Ok(Json(json!({ "deleted": id })))
}

pub async fn exporters(State(state): State<Arc<AppState>>) -> ApiResult {
    track_request(&state, |_| {}).await;
    Ok(Json(
        json!({ "exporters": exporter_statuses(&state.metrics_platform) }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_query_parsing() {
        let q = DataQuery {
            context: "system.cpu".into(),
            charts: None,
            dimensions: Some("user, system".into()),
            nodes: Some("n1".into()),
            labels: Some("k8s_namespace=prod,bad".into()),
            after: None,
            before: None,
            points: Some(99999),
            group: None,
            group_by: Some("node".into()),
            aggregate: None,
            tier: None,
        }
        .into_query();
        assert_eq!(q.dimensions, vec!["user", "system"]);
        assert_eq!(q.labels.len(), 1);
        assert_eq!(q.points, 5000);
        assert_eq!(q.after, -600);
    }

    #[test]
    fn windows() {
        let (a, b) = window(Some(-60), None);
        assert_eq!(b - a, 60);
        let (a, b) = window(Some(100), Some(50));
        assert!(a < b);
    }
}
