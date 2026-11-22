// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Historical flow search over the durable flow store.

use super::{track_request, PaginationQuery};
use crate::services::flow_ingest::INGEST_BATCH;
use crate::services::flow_store::{format_ts, Coverage, FlowQuery, StoredFlow, TimelineBucket};
use crate::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;
type Claims = Option<axum::Extension<crate::middleware::auth::Claims>>;

/// Longest window one request may cover.
const MAX_RANGE_DAYS: i64 = 30;
const DEFAULT_RANGE_MINUTES: i64 = 60;
const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 500;
const MAX_OFFSET: usize = 100_000;
const MAX_BUCKETS: i64 = 1000;
/// Bucket sizes a timeline may use, in seconds: 1m, 5m, 15m, 1h, 6h, 1d.
const BUCKETS: [i64; 6] = [60, 300, 900, 3600, 21_600, 86_400];
const VERDICTS: [&str; 7] = [
    "FORWARDED",
    "DROPPED",
    "ERROR",
    "AUDIT",
    "REDIRECTED",
    "TRACED",
    "TRANSLATED",
];

fn bad(msg: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": msg })))
}

#[derive(Debug, Deserialize, Default)]
pub struct HistoryParams {
    /// Range start, RFC 3339 (default: one hour before `to`).
    pub from: Option<String>,
    /// Range end, exclusive, RFC 3339 (default: now).
    pub to: Option<String>,
    /// Either side is in this namespace.
    pub namespace: Option<String>,
    /// Either side's pod name contains this text.
    pub pod: Option<String>,
    pub port: Option<u16>,
    pub verdict: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    /// Timeline only: bucket size in seconds (60, 300, 900, 3600, 21600 or 86400).
    pub bucket_secs: Option<i64>,
}

/// The requested window, validated. `from` and `to` are in the stored
/// timestamp form so they compare correctly as text.
#[derive(Debug, PartialEq)]
pub struct Range {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

pub fn parse_range(
    from: Option<&str>,
    to: Option<&str>,
    now: DateTime<Utc>,
) -> Result<Range, String> {
    let parse = |label: &str, v: &str| {
        DateTime::parse_from_rfc3339(v.trim())
            .map(|t| t.with_timezone(&Utc))
            .map_err(|_| {
                format!("{label} must be an RFC 3339 timestamp such as 2026-09-24T04:00:00Z")
            })
    };
    let to = match to.filter(|s| !s.trim().is_empty()) {
        Some(v) => parse("to", v)?,
        None => now,
    };
    let from = match from.filter(|s| !s.trim().is_empty()) {
        Some(v) => parse("from", v)?,
        None => to - Duration::minutes(DEFAULT_RANGE_MINUTES),
    };
    if from >= to {
        return Err("from must be earlier than to".into());
    }
    if to - from > Duration::days(MAX_RANGE_DAYS) {
        return Err(format!("the range can cover at most {MAX_RANGE_DAYS} days"));
    }
    Ok(Range { from, to })
}

/// Smallest allowed bucket that keeps a range to about 120 buckets.
pub fn auto_bucket(range_secs: i64) -> i64 {
    BUCKETS
        .iter()
        .copied()
        .find(|b| range_secs / b <= 120)
        .unwrap_or(BUCKETS[BUCKETS.len() - 1])
}

/// Add empty buckets for the quiet stretches, so a chart shows them as zero.
pub fn fill_buckets(
    found: &[TimelineBucket],
    from: i64,
    to: i64,
    bucket: i64,
) -> Vec<TimelineBucket> {
    let by_start: std::collections::HashMap<i64, &TimelineBucket> =
        found.iter().map(|b| (b.start, b)).collect();
    let mut out = Vec::new();
    let mut start = from.div_euclid(bucket) * bucket;
    while start < to {
        out.push(match by_start.get(&start) {
            Some(b) => (*b).clone(),
            None => TimelineBucket {
                start,
                forwarded: 0,
                dropped: 0,
                other: 0,
            },
        });
        start += bucket;
    }
    out
}

/// The namespace limit to enforce inside the query, if the caller has one.
fn scope_of(state: &AppState, claims: &Claims) -> Option<Vec<String>> {
    if state.config.auth_disabled {
        return None;
    }
    match claims.as_ref() {
        Some(c) if c.role != "admin" && !c.namespaces.is_empty() => Some(c.namespaces.clone()),
        _ => None,
    }
}

fn build_query(
    p: &HistoryParams,
    range: &Range,
    scope: Option<Vec<String>>,
) -> Result<FlowQuery, (StatusCode, Json<Value>)> {
    let text =
        |label: &str, v: &Option<String>| -> Result<Option<String>, (StatusCode, Json<Value>)> {
            match v.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                None => Ok(None),
                Some(s) if s.len() > 253 || s.chars().any(char::is_control) => {
                    Err(bad(&format!("{label} is not valid")))
                }
                Some(s) => Ok(Some(s.to_string())),
            }
        };
    let verdict = match p
        .verdict
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(v) => {
            let up = v.to_ascii_uppercase();
            if !VERDICTS.contains(&up.as_str()) {
                return Err(bad("verdict must be one of: FORWARDED, DROPPED, ERROR, AUDIT, REDIRECTED, TRACED, TRANSLATED"));
            }
            Some(up)
        }
    };
    if p.port == Some(0) {
        return Err(bad("port must be between 1 and 65535"));
    }
    Ok(FlowQuery {
        namespace: text("namespace", &p.namespace)?,
        pod: text("pod", &p.pod)?,
        port: p.port,
        verdict,
        since_rfc3339: Some(format_ts(range.from)),
        until_rfc3339: Some(format_ts(range.to)),
        scope,
        ..Default::default()
    })
}

fn flow_json(f: &StoredFlow) -> Value {
    json!({
        "id": f.id,
        "timestamp": f.ts,
        "cluster": f.cluster,
        "verdict": f.verdict,
        "drop_reason": f.drop_reason,
        "protocol": f.protocol,
        "port": f.port,
        "source": { "namespace": f.src_namespace, "pod": f.src_pod, "ip": f.src_ip },
        "destination": { "namespace": f.dst_namespace, "pod": f.dst_pod, "ip": f.dst_ip },
    })
}

/// What a reader must know to interpret the numbers.
fn context(state: &AppState, range: &Range, cov: &Coverage) -> Value {
    let from_ts = format_ts(range.from);
    // What is known for certain is where the stored flows begin, not whether
    // anything earlier was missed: a quiet cluster and a late start look the
    // same. So say only whether the range reaches before the oldest stored flow.
    let starts_before_oldest = cov.oldest.as_deref().map(|o| from_ts.as_str() < o);
    let stats = state.flow_store.stats();
    json!({
        "range": { "from": from_ts, "to": format_ts(range.to) },
        "coverage": {
            "oldest": cov.oldest,
            "newest": cov.newest,
            "stored_flows": cov.total,
            "retention_days": cov.retention_days,
            "durable": cov.durable,
            "range_starts_before_oldest": starts_before_oldest,
        },
        "capture": {
            "interval_secs": state.config.flow_ingest_interval_secs,
            "batch": INGEST_BATCH,
            "last_capture_ok": stats.last_ingest_ok,
            "last_capture_at": stats.last_ingest_at,
            // Flows Hubble sent without a usable timestamp: not stored, so not counted.
            "skipped_no_time": stats.skipped_no_time,
            "note": format!(
                "Flows are captured by polling Hubble every {}s and keeping the most recent {} it returns, \
                 so a busy cluster can produce more flows between captures than are stored. Counts are \
                 lower bounds, and a quiet stretch may be a gap in capture.",
                state.config.flow_ingest_interval_secs, INGEST_BATCH
            ),
        },
    })
}

async fn on_pool<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, (StatusCode, Json<Value>)> {
    let internal = |m: &str| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": m })),
        )
    };
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| internal("flow query failed"))?
        .map_err(|e| {
            tracing::warn!("flow history query failed: {e}");
            internal("flow query failed")
        })
}

/// Stored flows in a time range, newest first, with the total that matches.
pub async fn flow_history(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Query(p): Query<HistoryParams>,
    Query(page): Query<PaginationQuery>,
) -> ApiResult {
    track_request(&state, |_| {}).await;
    let range = parse_range(p.from.as_deref(), p.to.as_deref(), Utc::now()).map_err(|m| bad(&m))?;
    let mut q = build_query(&p, &range, scope_of(&state, &claims))?;
    let limit = p
        .limit
        .or(page.limit)
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);
    let offset = p.offset.or(page.offset).unwrap_or(0);
    if offset > MAX_OFFSET {
        return Err(bad(
            "offset is too large: narrow the range or filters instead",
        ));
    }
    q.limit = limit;
    q.offset = offset;

    let store = state.flow_store.clone();
    let (flows, total, coverage) = on_pool(move || {
        Ok((
            store.query(&q)?,
            store.count(&q)?,
            store.coverage(q.scope.as_deref())?,
        ))
    })
    .await?;

    let mut body = context(&state, &range, &coverage);
    body["flows"] = Value::Array(flows.iter().map(flow_json).collect());
    body["total"] = json!(total);
    body["limit"] = json!(limit);
    body["offset"] = json!(offset);
    Ok(Json(body))
}

/// Flow counts per time bucket over the range, quiet stretches included.
pub async fn flow_timeline(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Query(p): Query<HistoryParams>,
) -> ApiResult {
    track_request(&state, |_| {}).await;
    let range = parse_range(p.from.as_deref(), p.to.as_deref(), Utc::now()).map_err(|m| bad(&m))?;
    let q = build_query(&p, &range, scope_of(&state, &claims))?;
    let range_secs = (range.to - range.from).num_seconds();
    let bucket = match p.bucket_secs {
        None => auto_bucket(range_secs),
        Some(b) if BUCKETS.contains(&b) => b,
        Some(_) => {
            return Err(bad(
                "bucket_secs must be one of 60, 300, 900, 3600, 21600, 86400",
            ))
        }
    };
    if range_secs / bucket > MAX_BUCKETS {
        return Err(bad(
            "too many buckets for this range: use a larger bucket_secs",
        ));
    }

    let store = state.flow_store.clone();
    let (found, coverage) = on_pool(move || {
        Ok((
            store.timeline(&q, bucket)?,
            store.coverage(q.scope.as_deref())?,
        ))
    })
    .await?;

    let buckets = fill_buckets(&found, range.from.timestamp(), range.to.timestamp(), bucket);
    let total: u64 = buckets
        .iter()
        .map(|b| b.forwarded + b.dropped + b.other)
        .sum();
    let mut body = context(&state, &range, &coverage);
    body["bucket_secs"] = json!(bucket);
    body["total"] = json!(total);
    body["buckets"] = Value::Array(
        buckets
            .iter()
            .map(|b| {
                json!({
                    "start": DateTime::from_timestamp(b.start, 0).map(format_ts).unwrap_or_default(),
                    "forwarded": b.forwarded,
                    "dropped": b.dropped,
                    "other": b.other,
                })
            })
            .collect(),
    );
    Ok(Json(body))
}

#[derive(Debug, Deserialize)]
pub struct PurgeRequest {
    pub namespace: Option<String>,
    pub older_than_days: Option<i64>,
}

/// GET /api/v1/flows/store — retention + coverage + recent ingest gaps.
pub async fn flow_store_info(
    State(state): State<Arc<AppState>>,
    claims: Claims,
) -> ApiResult {
    track_request(&state, |_| {}).await;
    let scope = scope_of(&state, &claims);
    let store = state.flow_store.clone();
    let scope_owned = scope.clone();
    let (coverage, purged_hint) = on_pool(move || {
        Ok((
            store.coverage(scope_owned.as_deref())?,
            store.retention_days(),
        ))
    })
    .await?;
    let stats = state.flow_store.stats();
    Ok(Json(json!({
        "retention_days": purged_hint,
        "coverage": coverage,
        "ingest": {
            "connected": stats.stream_connected,
            "gaps": stats.gap_count,
            "last_gap_at": stats.last_gap_at,
            "recent_gaps": state.flow_store.recent_gaps(),
            "events_per_sec": stats.events_per_sec,
            "lag_secs": stats.lag_secs,
            "source": stats.ingest_source,
        },
        "observe_only": true,
    })))
}

/// POST /api/v1/flows/store/purge — namespace-scoped or global age purge (admin).
pub async fn flow_store_purge(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Json(req): Json<PurgeRequest>,
) -> ApiResult {
    super::check_admin(&state, &claims)?;
    track_request(&state, |_| {}).await;
    if let Some(ns) = req.namespace.as_deref() {
        if !super::has_namespace_access(&state, &claims, ns) {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "namespace not in scope" })),
            ));
        }
    }
    let store = state.flow_store.clone();
    let ns = req.namespace.clone();
    let days = req.older_than_days;
    let deleted = on_pool(move || store.purge(ns.as_deref(), days)).await?;
    Ok(Json(json!({
        "deleted": deleted,
        "namespace": req.namespace,
        "older_than_days": req.older_than_days.unwrap_or_else(|| state.flow_store.retention_days()),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::flow_store::normalize_ts;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 24, 12, 0, 0).unwrap()
    }

    #[test]
    fn range_defaults_to_the_last_hour() {
        let r = parse_range(None, None, now()).unwrap();
        assert_eq!(r.to, now());
        assert_eq!(r.from, now() - Duration::hours(1));
        assert_eq!(
            parse_range(Some(" "), Some(""), now()).unwrap(),
            r,
            "blank means default"
        );
    }

    #[test]
    fn range_accepts_offsets_and_only_from() {
        let r = parse_range(Some("2026-09-24T15:30:00+05:30"), None, now()).unwrap();
        assert_eq!(r.from, Utc.with_ymd_and_hms(2026, 9, 24, 10, 0, 0).unwrap());
        assert_eq!(r.to, now());
    }

    #[test]
    fn range_rejects_bad_input() {
        for (f, t, why) in [
            (Some("yesterday"), None, "from"),
            (None, Some("2026-09-24"), "to"),
            (
                Some("2026-09-24T12:00:00Z"),
                Some("2026-09-24T12:00:00Z"),
                "empty",
            ),
            (
                Some("2026-09-24T13:00:00Z"),
                Some("2026-09-24T12:00:00Z"),
                "reversed",
            ),
            (
                Some("2026-08-01T00:00:00Z"),
                Some("2026-09-24T12:00:00Z"),
                "too long",
            ),
        ] {
            assert!(parse_range(f, t, now()).is_err(), "{why}");
        }
        assert!(
            parse_range(
                Some("2026-08-25T12:00:00Z"),
                Some("2026-09-24T12:00:00Z"),
                now()
            )
            .is_ok(),
            "exactly 30 days is allowed"
        );
        assert!(
            parse_range(
                Some("2026-08-25T11:59:59Z"),
                Some("2026-09-24T12:00:00Z"),
                now()
            )
            .is_err(),
            "one second over is not"
        );
    }

    #[test]
    fn auto_bucket_keeps_about_120_buckets() {
        assert_eq!(auto_bucket(3600), 60);
        assert_eq!(auto_bucket(2 * 3600), 60);
        assert_eq!(auto_bucket(6 * 3600), 300);
        assert_eq!(auto_bucket(24 * 3600), 900);
        assert_eq!(auto_bucket(7 * 86_400), 21_600); // 21600
                                                     // 30 days is exactly 120 six-hour buckets, so one-day buckets are not needed.
        assert_eq!(auto_bucket(30 * 86_400), 21_600);
        assert_eq!(
            auto_bucket(365 * 86_400),
            86_400,
            "beyond that, the largest bucket"
        );
        for secs in [60, 3600, 86_400, 7 * 86_400, 30 * 86_400] {
            assert!(BUCKETS.contains(&auto_bucket(secs)));
            assert!(secs / auto_bucket(secs) <= MAX_BUCKETS);
        }
    }

    #[test]
    fn fill_buckets_adds_zero_buckets_for_quiet_stretches() {
        let found = vec![
            TimelineBucket {
                start: 120,
                forwarded: 3,
                dropped: 1,
                other: 0,
            },
            TimelineBucket {
                start: 300,
                forwarded: 0,
                dropped: 0,
                other: 2,
            },
        ];
        let got = fill_buckets(&found, 100, 400, 60);
        assert_eq!(
            got.iter().map(|b| b.start).collect::<Vec<_>>(),
            vec![60, 120, 180, 240, 300, 360]
        );
        assert_eq!((got[1].forwarded, got[1].dropped), (3, 1));
        assert_eq!(
            got[0],
            TimelineBucket {
                start: 60,
                forwarded: 0,
                dropped: 0,
                other: 0
            }
        );
        assert_eq!(got[4].other, 2);
        // The bucket containing `from` is always included, even for a tiny range.
        assert_eq!(
            fill_buckets(&[], 100, 101, 60)
                .iter()
                .map(|b| b.start)
                .collect::<Vec<_>>(),
            vec![60]
        );
    }

    fn params() -> HistoryParams {
        HistoryParams::default()
    }

    fn range() -> Range {
        parse_range(None, None, now()).unwrap()
    }

    #[test]
    fn query_carries_the_range_scope_and_uppercased_verdict() {
        let mut p = params();
        p.verdict = Some("dropped".into());
        p.namespace = Some(" shop ".into());
        p.port = Some(443);
        let q = build_query(&p, &range(), Some(vec!["shop".into()])).unwrap();
        assert_eq!(q.verdict.as_deref(), Some("DROPPED"));
        assert_eq!(q.namespace.as_deref(), Some("shop"));
        assert_eq!(q.scope, Some(vec!["shop".to_string()]));
        assert_eq!(
            q.since_rfc3339.as_deref(),
            Some("2026-09-24T11:00:00.000000Z")
        );
        assert_eq!(
            q.until_rfc3339.as_deref(),
            Some("2026-09-24T12:00:00.000000Z")
        );
    }

    #[test]
    fn query_rejects_bad_filters() {
        let mut p = params();
        p.verdict = Some("MAYBE".into());
        assert!(build_query(&p, &range(), None).is_err());
        let mut p = params();
        p.port = Some(0);
        assert!(build_query(&p, &range(), None).is_err());
        let mut p = params();
        p.pod = Some("a\nb".into());
        assert!(build_query(&p, &range(), None).is_err());
        let mut p = params();
        p.namespace = Some("x".repeat(254));
        assert!(build_query(&p, &range(), None).is_err());
        assert!(normalize_ts("2026-09-24T11:00:00.000000Z").is_some());
    }

    #[test]
    fn blank_filters_are_ignored() {
        let mut p = params();
        p.namespace = Some("  ".into());
        p.verdict = Some("".into());
        let q = build_query(&p, &range(), None).unwrap();
        assert!(q.namespace.is_none() && q.verdict.is_none() && q.scope.is_none());
    }
}
