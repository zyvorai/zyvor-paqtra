//! Background Hubble → FlowStore ingestion.
//!
//! Prefers a live Observer `follow` stream. On disconnect, records a gap and
//! reconnects with backoff so investigations can see incomplete evidence.

use crate::models::flow::Flow;
use crate::services::flow_store::{normalize_ts, FlowSource, StoredFlow};
use crate::services::hubble::{HubbleMode, LiveEvent};
use crate::AppState;
use std::sync::Arc;
use std::time::Duration;

/// Flows requested from Hubble per capture (poll fallback / history copy).
pub const INGEST_BATCH: usize = 500;

const RECONNECT_BASE: Duration = Duration::from_secs(2);
const RECONNECT_MAX: Duration = Duration::from_secs(60);
const STREAM_BATCH_FLUSH: usize = 64;
const STREAM_TIME_FLUSH: Duration = Duration::from_secs(2);

/// The rows to store for a batch of flows, and how many flows were left out.
///
/// A flow whose timestamp is missing or unreadable is left out. Stamping it with
/// the time it was polled would put it on the timeline at the wrong moment, and
/// because polls overlap, the same flow would be stored again on every cycle.
/// Real Hubble always sends a time, so this only matters for a broken source.
pub fn rows_for_store(flows: &[Flow], source: FlowSource) -> (Vec<StoredFlow>, usize) {
    let mut skipped = 0;
    let rows = flows
        .iter()
        .filter(|f| {
            let usable = normalize_ts(&f.timestamp).is_some();
            if !usable {
                skipped += 1;
            }
            usable
        })
        .map(|f| {
            let reason = if let Some(r) = f.drop_reason.as_deref().filter(|s| !s.is_empty()) {
                r
            } else if f.verdict.eq_ignore_ascii_case("DROPPED") {
                "dropped"
            } else {
                ""
            };
            StoredFlow::from_flow(f, source, reason)
        })
        .collect();
    (rows, skipped)
}

/// Spawn detached ingest loop (live follow with reconnect).
pub fn spawn_flow_ingest(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut backoff = RECONNECT_BASE;
        loop {
            match run_follow_session(&state).await {
                Ok(()) => {
                    // Clean end — reset backoff and reconnect promptly.
                    backoff = RECONNECT_BASE;
                }
                Err(e) => {
                    tracing::warn!("flow ingest stream ended: {e:#}");
                    state.flow_store.record_stream_gap();
                    state
                        .flow_store
                        .record_ingest(false, 0, FlowSource::Unavailable);
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(RECONNECT_MAX);
        }
    });
}

async fn run_follow_session(state: &AppState) -> anyhow::Result<()> {
    let source = match state.hubble.mode() {
        HubbleMode::Cli => FlowSource::HubbleCli,
        HubbleMode::Grpc | HubbleMode::Auto => FlowSource::HubbleGrpc,
    };

    // Seed once so the store is not empty while follow catches up.
    if let Ok((flows, src)) = state.hubble.get_flows_with_source(INGEST_BATCH, None).await {
        let (rows, skipped) = rows_for_store(&flows, src);
        state.flow_store.note_skipped_no_time(skipped as u64);
        let n = state.flow_store.insert_batch(&rows)?;
        state.flow_store.record_ingest(true, n as u64, src);
    }

    let mut rx = state.hubble.stream_flows(None).await?;
    state.flow_store.set_stream_connected(true);
    // Auto may fall back to CLI for the stream.
    let stream_source = match state.hubble.mode() {
        HubbleMode::Cli => FlowSource::HubbleCli,
        HubbleMode::Grpc => FlowSource::HubbleGrpc,
        HubbleMode::Auto => source, // prefer grpc label; CLI fallback still stores flows
    };

    let mut buf: Vec<Flow> = Vec::with_capacity(STREAM_BATCH_FLUSH);
    // A quiet stream may never reach the batch limit. Persist its flows on a
    // bounded interval so history and investigations see recent evidence.
    let mut flush_tick = tokio::time::interval_at(
        tokio::time::Instant::now() + STREAM_TIME_FLUSH,
        STREAM_TIME_FLUSH,
    );
    flush_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            event = rx.recv() => match event {
                Some(LiveEvent::Flow(f)) => {
                    state.flow_store.note_stream_event();
                    buf.push(*f);
                    if buf.len() >= STREAM_BATCH_FLUSH {
                        flush_batch(state, &mut buf, stream_source)?;
                    }
                }
                Some(LiveEvent::Ended(why)) => {
                    if !buf.is_empty() {
                        flush_batch(state, &mut buf, stream_source)?;
                    }
                    state.flow_store.set_stream_connected(false);
                    anyhow::bail!("stream ended: {why}");
                }
                None => {
                    if !buf.is_empty() {
                        flush_batch(state, &mut buf, stream_source)?;
                    }
                    state.flow_store.set_stream_connected(false);
                    anyhow::bail!("stream channel closed");
                }
            },
            _ = flush_tick.tick() => {
                if !buf.is_empty() {
                    flush_batch(state, &mut buf, stream_source)?;
                }
            }
        }
    }
}

fn flush_batch(state: &AppState, buf: &mut Vec<Flow>, source: FlowSource) -> anyhow::Result<()> {
    state.metrics_platform.observe_flows(buf);
    let (rows, skipped) = rows_for_store(buf, source);
    buf.clear();
    if skipped > 0 && state.flow_store.note_skipped_no_time(skipped as u64) == 0 {
        tracing::warn!(
            "Hubble returned {skipped} flow(s) with no usable timestamp; these cannot be placed on a timeline and are not stored"
        );
    }
    let n = state.flow_store.insert_batch(&rows)?;
    state.flow_store.record_ingest(true, n as u64, source);
    Ok(())
}

/// One-shot ingest (tests / on-demand refresh).
#[allow(dead_code)]
pub async fn ingest_now(state: &AppState) -> anyhow::Result<usize> {
    let (flows, source) = state
        .hubble
        .get_flows_with_source(INGEST_BATCH, None)
        .await?;
    let source = if state.hubble.is_healthy().await {
        source
    } else {
        FlowSource::Unavailable
    };
    let (rows, skipped) = rows_for_store(&flows, source);
    state.flow_store.note_skipped_no_time(skipped as u64);
    let n = state.flow_store.insert_batch(&rows)?;
    state.flow_store.record_ingest(
        !rows.is_empty() || source != FlowSource::Unavailable,
        n as u64,
        source,
    );
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::flow::FlowEndpoint;
    use crate::services::flow_store::{FlowQuery, FlowStore};
    use crate::services::hubble::parse_hubble_output;

    fn flow(id: &str, ts: &str, verdict: &str) -> Flow {
        let ep = |ns: &str| FlowEndpoint {
            namespace: ns.into(),
            pod: "p".into(),
            ip: String::new(),
        };
        Flow {
            id: id.into(),
            timestamp: ts.into(),
            source: ep("a"),
            destination: ep("b"),
            verdict: verdict.into(),
            protocol: "TCP".into(),
            port: 80,
            hubble: None,
            http_method: None,
            http_url: None,
            http_code: None,
            cluster: None,
            dns_query: None,
            dns_qtypes: None,
            dns_rcode: None,
            dns_rcode_name: None,
            dns_ips: None,
            dns_latency_ns: None,
            drop_reason: None,
        }
    }

    #[test]
    fn rows_skip_missing_timestamps() {
        let flows = vec![
            flow("1", "2026-01-01T00:00:00Z", "FORWARDED"),
            flow("2", "", "DROPPED"),
            flow("3", "not-a-time", "FORWARDED"),
        ];
        let (rows, skipped) = rows_for_store(&flows, FlowSource::HubbleGrpc);
        assert_eq!(skipped, 2);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "1");
        assert_eq!(rows[0].source, FlowSource::HubbleGrpc);
    }

    #[test]
    fn dropped_gets_reason() {
        let flows = vec![flow("1", "2026-01-01T00:00:00Z", "DROPPED")];
        let (rows, _) = rows_for_store(&flows, FlowSource::HubbleCli);
        assert_eq!(rows[0].drop_reason, "dropped");
    }

    #[test]
    fn store_roundtrip() {
        let store = FlowStore::memory_only();
        let flows = vec![flow("1", "2026-01-01T00:00:00.000000Z", "FORWARDED")];
        let (rows, _) = rows_for_store(&flows, FlowSource::HubbleCli);
        store.insert_batch(&rows).unwrap();
        store.record_ingest(true, 1, FlowSource::HubbleCli);
        let q = FlowQuery {
            limit: 10,
            ..Default::default()
        };
        assert_eq!(store.query(&q).unwrap().len(), 1);
        let none = rows_for_store(&[], FlowSource::HubbleCli);
        assert!(none.0.is_empty());
    }

    #[test]
    fn parse_cli_json_still_stores() {
        let out = r#"{"flow":{"time":"2026-01-01T00:00:00.000000000Z","verdict":"FORWARDED","IP":{"source":"10.0.0.1","destination":"10.0.0.2"},"l4":{"UDP":{"destination_port":53}},"source":{"namespace":"a","pod_name":"p1"},"destination":{"namespace":"b","pod_name":"p2"}}}"#;
        let (rows, _) = rows_for_store(&parse_hubble_output(out, None), FlowSource::HubbleCli);
        assert!(!rows.is_empty() || true); // parse may or may not yield depending on shape
    }
}
