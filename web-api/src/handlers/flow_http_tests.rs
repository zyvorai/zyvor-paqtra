// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Handler-level tests for the views derived from stored flow history. Each
//! request goes through the real route table (`flow_insights::routes`), query
//! parsing, namespace scoping and JSON rendering, against an in-memory store.

use super::flow_insights;
use crate::config::{Config, HubbleMode};
use crate::middleware::auth::Claims;
use crate::services::cache::CacheService;
use crate::services::flow_store::{format_ts, FlowSource, FlowStore, StoredFlow};
use crate::services::hubble::HubbleService;
use crate::services::k8s::K8sService;
use crate::services::metrics_platform::MetricsPlatform;
use crate::services::prometheus::PrometheusService;
use crate::{AppMetrics, AppState};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use chrono::{Duration, Utc};
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

fn config(auth_disabled: bool) -> Config {
    Config {
        host: "127.0.0.1".into(),
        port: 0,
        jwt_secret: "x".repeat(40),
        hubble_address: "127.0.0.1:4245".into(),
        hubble_mode: HubbleMode::default(),
        k8s_context: None,
        auth_disabled,
        admin_username: "admin".into(),
        admin_password: "password-123".into(),
        api_key: String::new(),
        ui_dist_dir: None,
        prometheus_url: None,
        hubble_addresses: vec![("local".into(), "127.0.0.1:4245".into())],
        otel_endpoint: None,
        otel_service_name: "t".into(),
        tls_cert_path: None,
        tls_key_path: None,
        tls_port: 0,
        data_dir: None,
        alert_cooldown_secs: 900,
        alert_eval_interval_secs: 60,
        flow_retention_days: 7,
        flow_ingest_interval_secs: 30,
    }
}

fn state(auth_disabled: bool) -> Arc<AppState> {
    Arc::new(AppState {
        config: config(auth_disabled),
        hubble: HubbleService::new(
            "127.0.0.1:4245",
            vec![("local".into(), "127.0.0.1:4245".into())],
        )
        .unwrap(),
        k8s: K8sService::new(None),
        cache: CacheService::new(),
        flow_store: Arc::new(FlowStore::memory_only()),
        prometheus: PrometheusService::new(None),
        metrics: AppMetrics::default(),
        metrics_platform: Arc::new(MetricsPlatform::from_env(None).unwrap()),
    })
}

struct F<'a> {
    src: (&'a str, &'a str),
    dst: (&'a str, &'a str),
    port: u16,
    verdict: &'a str,
    reason: &'a str,
    mins_ago: i64,
}

fn insert(st: &AppState, flows: &[F]) {
    let now = Utc::now();
    let rows: Vec<StoredFlow> = flows
        .iter()
        .enumerate()
        .map(|(i, f)| StoredFlow {
            id: format!("f{i}-{}-{}", f.src.1, f.dst.1),
            ts: format_ts(now - Duration::minutes(f.mins_ago) - Duration::seconds(i as i64 % 7)),
            cluster: "local".into(),
            verdict: f.verdict.into(),
            drop_reason: f.reason.into(),
            protocol: "TCP".into(),
            port: f.port,
            src_namespace: f.src.0.into(),
            src_pod: f.src.1.into(),
            src_ip: "10.0.0.1".into(),
            src_identity: 1,
            dst_namespace: f.dst.0.into(),
            dst_pod: f.dst.1.into(),
            dst_ip: "10.0.0.2".into(),
            dst_identity: 2,
            source: FlowSource::HubbleGrpc,
        })
        .collect();
    st.flow_store.insert_batch(&rows).unwrap();
}

fn f<'a>(
    src: (&'a str, &'a str),
    dst: (&'a str, &'a str),
    port: u16,
    verdict: &'a str,
    reason: &'a str,
    mins_ago: i64,
) -> F<'a> {
    F {
        src,
        dst,
        port,
        verdict,
        reason,
        mins_ago,
    }
}

fn app(st: &Arc<AppState>, claims: Option<Claims>) -> Router {
    let r = flow_insights::routes();
    let r = match claims {
        Some(c) => r.layer(axum::Extension(c)),
        None => r,
    };
    r.with_state(st.clone())
}

async fn get(app: Router, uri: &str) -> (StatusCode, Value) {
    let resp = app
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 4 << 20)
        .await
        .unwrap();
    let v = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, v)
}

fn scoped(ns: &[&str]) -> Claims {
    Claims {
        sub: "u".into(),
        exp: usize::MAX,
        iat: 0,
        role: "viewer".into(),
        namespaces: ns.iter().map(|s| s.to_string()).collect(),
    }
}

const WEB: (&str, &str) = ("shop", "web-7c9d8f6b5-x2k4p");
const DB: (&str, &str) = ("data", "pg-0");
const CACHE: (&str, &str) = ("shop", "redis-0");

fn sample(st: &AppState) {
    insert(
        st,
        &[
            f(WEB, DB, 5432, "FORWARDED", "", 5),
            f(WEB, DB, 5432, "FORWARDED", "", 6),
            f(WEB, DB, 5432, "DROPPED", "Policy denied", 7),
            f(WEB, CACHE, 6379, "DROPPED", "CT: Map insertion failed", 8),
            f(WEB, CACHE, 6379, "DROPPED", "CT: Map insertion failed", 9),
            f(WEB, DB, 5433, "AUDIT", "", 10),
            f(CACHE, DB, 5432, "AUDIT", "", 11),
        ],
    );
}

// ---- edges, cardinality, otlp -------------------------------------------

#[tokio::test]
async fn edges_lists_edges_and_validates_parameters() {
    let st = state(true);
    sample(&st);
    let (s, v) = get(app(&st, None), "/api/v1/flows/history/edges").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["total_flows"], 7);
    assert_eq!(v["granularity"], "workload");
    assert!(v["edges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["source"] == "shop/web" && e["destination"] == "data/pg"));
    assert!(v["coverage"]["stored_flows"].as_u64().unwrap() >= 7);
    for bad in [
        "?granularity=galaxy",
        "?max_edges=0",
        "?max_edges=1001",
        "?from=yesterday",
        "?verdict=MAYBE",
    ] {
        let (s, v) = get(app(&st, None), &format!("/api/v1/flows/history/edges{bad}")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
        assert!(v["error"].is_string());
    }
}

#[tokio::test]
async fn cardinality_reports_and_rejects_a_bad_budget() {
    let st = state(true);
    sample(&st);
    let (s, v) = get(
        app(&st, None),
        "/api/v1/flows/history/cardinality?budget=5000",
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v.get("coverage").is_some());
    for b in ["budget=5", "budget=99999999999"] {
        let (s, _) = get(
            app(&st, None),
            &format!("/api/v1/flows/history/cardinality?{b}"),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{b}");
    }
}

#[tokio::test]
async fn otlp_exports_records_and_bounds_the_limit() {
    let st = state(true);
    sample(&st);
    let (s, v) = get(app(&st, None), "/api/v1/flows/history/otlp?limit=3").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["records"], 3);
    assert_eq!(v["limit"], 3);
    assert!(v["otlp"]["resourceLogs"].is_array());
    for l in ["limit=0", "limit=1001"] {
        let (s, _) = get(app(&st, None), &format!("/api/v1/flows/history/otlp?{l}")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{l}");
    }
}

// ---- diff and fanout ------------------------------------------------------

#[tokio::test]
async fn diff_finds_a_new_edge_against_the_previous_window() {
    let st = state(true);
    // Baseline window (the hour before the default one): web -> db only.
    insert(&st, &[f(WEB, DB, 5432, "FORWARDED", "", 90)]);
    // Current window: web -> db again, plus a new edge to the cache.
    insert(
        &st,
        &[
            f(WEB, DB, 5432, "FORWARDED", "", 10),
            f(WEB, CACHE, 6379, "FORWARDED", "", 12),
        ],
    );
    let (s, v) = get(app(&st, None), "/api/v1/flows/history/diff").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let new_edges = v["new_edges"].as_array().unwrap();
    assert_eq!(new_edges.len(), 1, "{v}");
    assert_eq!(new_edges[0]["destination"], "shop/redis");
    assert_eq!(v["counts"]["new_edges"], 1);
    assert_eq!(v["baseline"]["truncated"], false);
}

#[tokio::test]
async fn diff_rejects_half_given_and_overlapping_baselines() {
    let st = state(true);
    let now = Utc::now();
    let ts = |m: i64| format_ts(now - Duration::minutes(m)).replace('+', "%2B");
    let (s, v) = get(
        app(&st, None),
        &format!("/api/v1/flows/history/diff?baseline_from={}", ts(120)),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("both"));
    let (s, v) = get(
        app(&st, None),
        &format!(
            "/api/v1/flows/history/diff?baseline_from={}&baseline_to={}",
            ts(120),
            ts(30)
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("overlap"));
    let (s, _) = get(app(&st, None), "/api/v1/flows/history/diff?max_items=0").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = get(app(&st, None), "/api/v1/flows/history/diff?granularity=x").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn diff_warns_when_the_baseline_predates_stored_history() {
    let st = state(true);
    insert(&st, &[f(WEB, DB, 5432, "FORWARDED", "", 10)]);
    let (s, v) = get(app(&st, None), "/api/v1/flows/history/diff").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["baseline"]["starts_before_oldest"], true);
    assert!(!v["warnings"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn fanout_flags_a_scanner_and_validates_thresholds() {
    let st = state(true);
    let pods: Vec<String> = (0..25).map(|i| format!("t-{i}")).collect();
    let flows: Vec<F> = pods
        .iter()
        .map(|p| {
            f(
                ("evil", "scanner"),
                ("prod", p.as_str()),
                22,
                "DROPPED",
                "Policy denied",
                5,
            )
        })
        .collect();
    insert(&st, &flows);
    let (s, v) = get(
        app(&st, None),
        "/api/v1/flows/history/fanout?granularity=pod&min_targets=20",
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["suspects"], 1);
    assert_eq!(v["sources"][0]["source"], "evil/scanner");
    assert_eq!(v["sources"][0]["suspect"], true);
    for q in [
        "min_targets=1",
        "min_targets=100001",
        "min_drop_ratio=1.5",
        "min_drop_ratio=-1",
        "limit=0",
        "limit=1001",
    ] {
        let (s, _) = get(app(&st, None), &format!("/api/v1/flows/history/fanout?{q}")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{q}");
    }
}

// ---- drops ----------------------------------------------------------------

#[tokio::test]
async fn drops_splits_policy_from_datapath_capacity() {
    let st = state(true);
    sample(&st);
    let (s, v) = get(app(&st, None), "/api/v1/flows/history/drops").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["total_dropped"], 3);
    assert_eq!(v["reasons"][0]["reason"], "CT: Map insertion failed");
    assert_eq!(v["reasons"][0]["class"], "datapath_capacity");
    assert_eq!(v["reasons"][0]["dropped"], 2);
    assert_eq!(v["classes"][0]["class"], "datapath_capacity");
    assert_eq!(v["reasons"][1]["class"], "policy");
    assert_eq!(v["reasons"][1]["top_edges"][0]["port"], 5432);
    for q in ["max_reasons=0", "max_reasons=201", "granularity=x"] {
        let (s, _) = get(app(&st, None), &format!("/api/v1/flows/history/drops?{q}")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{q}");
    }
    let (_, v) = get(app(&st, None), "/api/v1/flows/history/drops?max_reasons=1").await;
    assert_eq!(v["reasons"].as_array().unwrap().len(), 1);
    assert_eq!(v["omitted_reasons"], 1);
}

#[tokio::test]
async fn drops_on_an_empty_store_is_an_empty_report() {
    let st = state(true);
    let (s, v) = get(app(&st, None), "/api/v1/flows/history/drops").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["total_dropped"], 0);
    assert!(v["reasons"].as_array().unwrap().is_empty());
}

// ---- top ------------------------------------------------------------------

#[tokio::test]
async fn top_ranks_sources_destinations_and_ports() {
    let st = state(true);
    sample(&st);
    let (s, v) = get(app(&st, None), "/api/v1/flows/history/top?limit=2").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["total_flows"], 7);
    assert_eq!(v["sources"]["entries"][0]["name"], "shop/web");
    assert_eq!(v["sources"]["entries"][0]["flows"], 6);
    assert_eq!(v["destinations"]["entries"][0]["name"], "data/pg");
    assert_eq!(v["ports"]["entries"][0]["name"], "TCP/5432");
    assert_eq!(v["sources"]["entries"].as_array().unwrap().len(), 2);
    for q in ["limit=0", "limit=101", "granularity=x"] {
        let (s, _) = get(app(&st, None), &format!("/api/v1/flows/history/top?{q}")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{q}");
    }
}

// ---- audit ----------------------------------------------------------------

#[tokio::test]
async fn audit_lists_only_audit_flows_whatever_verdict_is_asked() {
    let st = state(true);
    sample(&st);
    for uri in [
        "/api/v1/flows/history/audit",
        "/api/v1/flows/history/audit?verdict=FORWARDED",
    ] {
        let (s, v) = get(app(&st, None), uri).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_eq!(v["audit_flows"], 2, "{uri}");
        assert_eq!(v["edges_total"], 2);
        assert_eq!(v["destination_namespaces"][0]["namespace"], "data");
    }
    let (_, v) = get(app(&st, None), "/api/v1/flows/history/audit").await;
    assert!(v["edges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["source"] == "shop/web" && e["ports"][0] == 5433));
    let (s, _) = get(app(&st, None), "/api/v1/flows/history/audit?max_items=0").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = get(app(&st, None), "/api/v1/flows/history/audit?verdict=MAYBE").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn audit_with_no_audit_flows_says_to_check_audit_mode() {
    let st = state(true);
    insert(&st, &[f(WEB, DB, 5432, "FORWARDED", "", 5)]);
    let (s, v) = get(app(&st, None), "/api/v1/flows/history/audit").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["audit_flows"], 0);
    assert!(v["guidance"].as_str().unwrap().contains("audit mode"));
}

// ---- spikes ---------------------------------------------------------------

#[tokio::test]
async fn spikes_finds_a_burst_of_drops() {
    let st = state(true);
    let burst: Vec<F> = (0..60)
        .map(|_| f(WEB, DB, 5432, "DROPPED", "Policy denied", 20))
        .collect();
    insert(&st, &burst);
    insert(&st, &[f(WEB, DB, 5432, "DROPPED", "Policy denied", 40)]);
    let (s, v) = get(app(&st, None), "/api/v1/flows/history/spikes").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "spikes", "{v}");
    assert_eq!(v["bucket_secs"], 60);
    assert!(v["episodes"][0]["peak_dropped"].as_u64().unwrap() >= 50);
}

#[tokio::test]
async fn spikes_is_ok_on_a_quiet_store_and_validates_parameters() {
    let st = state(true);
    let (s, v) = get(app(&st, None), "/api/v1/flows/history/spikes").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["status"], "ok");
    // Too few buckets to judge.
    let (_, v) = get(
        app(&st, None),
        "/api/v1/flows/history/spikes?bucket_secs=3600",
    )
    .await;
    assert_eq!(v["status"], "insufficient_data");
    for q in [
        "sensitivity=0.5",
        "sensitivity=21",
        "sensitivity=NaN",
        "min_drops=0",
        "min_drops=1000001",
        "bucket_secs=7",
    ] {
        let (s, _) = get(app(&st, None), &format!("/api/v1/flows/history/spikes?{q}")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{q}");
    }
    // A 30 day range at one minute buckets would be 43200 buckets.
    let (s, v) = get(
        app(&st, None),
        &format!(
            "/api/v1/flows/history/spikes?bucket_secs=60&from={}",
            format_ts(Utc::now() - Duration::days(20)).replace('+', "%2B")
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
}

// ---- namespace scoping ----------------------------------------------------

#[tokio::test]
async fn namespace_scoped_callers_only_see_their_namespace() {
    let st = state(false);
    sample(&st);
    insert(
        &st,
        &[f(
            ("other", "x"),
            ("other", "y"),
            80,
            "DROPPED",
            "Policy denied",
            5,
        )],
    );
    for (path, key) in [
        ("top", "total_flows"),
        ("drops", "total_flows"),
        ("edges", "total_flows"),
    ] {
        let (s, v) = get(
            app(&st, Some(scoped(&["other"]))),
            &format!("/api/v1/flows/history/{path}"),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{path}: {v}");
        assert_eq!(v[key], 1, "{path} must count only the caller's namespace");
    }
    let (_, v) = get(
        app(&st, Some(scoped(&["other"]))),
        "/api/v1/flows/history/audit",
    )
    .await;
    assert_eq!(
        v["audit_flows"], 0,
        "audit flows of other namespaces stay hidden"
    );
    let (_, v) = get(
        app(&st, Some(scoped(&["data"]))),
        "/api/v1/flows/history/audit",
    )
    .await;
    assert_eq!(v["audit_flows"], 2);
    // Admin sees everything.
    let mut admin = scoped(&["other"]);
    admin.role = "admin".into();
    let (_, v) = get(app(&st, Some(admin)), "/api/v1/flows/history/top").await;
    assert_eq!(v["total_flows"], 8);
}
