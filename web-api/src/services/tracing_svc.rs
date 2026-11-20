// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Lightweight OpenTelemetry-compatible span exporter.
//
// When OTEL_EXPORTER_ENDPOINT is configured, completed request spans are
// serialised into the OTLP JSON format and POSTed to the collector
// asynchronously via `tokio::spawn` so that request latency is unaffected.
//
// No heavy OTEL SDK is required — we build the JSON payload ourselves and
// ship it with `reqwest`.

use serde::Serialize;
use std::time::Instant;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A completed request span ready for export.
#[derive(Debug, Clone)]
pub struct FinishedSpan {
    pub trace_id: String,
    pub span_id: String,
    pub name: String,
    pub start_time_unix_nano: u64,
    pub end_time_unix_nano: u64,
    pub status_code: u16,
    pub http_method: String,
    pub http_path: String,
    pub duration_ms: f64,
    pub request_id: String,
}

/// Handle used by the middleware to submit finished spans.
#[derive(Clone)]
pub struct SpanExporter {
    tx: mpsc::Sender<FinishedSpan>,
}

impl SpanExporter {
    /// Submit a finished span for asynchronous export.
    /// Drops the span silently if the channel is full (back-pressure).
    pub fn export(&self, span: FinishedSpan) {
        // Use try_send to avoid blocking the request path.
        let _ = self.tx.try_send(span);
    }
}

/// A lightweight helper to measure request duration and produce a `FinishedSpan`.
pub struct SpanBuilder {
    trace_id: String,
    span_id: String,
    name: String,
    http_method: String,
    http_path: String,
    request_id: String,
    start: Instant,
    start_unix_nano: u64,
}

impl SpanBuilder {
    pub fn start(http_method: &str, http_path: &str, request_id: &str) -> Self {
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            trace_id: generate_trace_id(),
            span_id: generate_span_id(),
            name: format!("{} {}", http_method, http_path),
            http_method: http_method.to_string(),
            http_path: http_path.to_string(),
            request_id: request_id.to_string(),
            start: Instant::now(),
            start_unix_nano: now_unix.as_nanos() as u64,
        }
    }

    /// Finish the span and return the completed record.
    pub fn finish(self, status_code: u16) -> FinishedSpan {
        let elapsed = self.start.elapsed();
        let end_unix_nano = self.start_unix_nano + elapsed.as_nanos() as u64;
        FinishedSpan {
            trace_id: self.trace_id,
            span_id: self.span_id,
            name: self.name,
            start_time_unix_nano: self.start_unix_nano,
            end_time_unix_nano: end_unix_nano,
            status_code,
            http_method: self.http_method,
            http_path: self.http_path,
            duration_ms: elapsed.as_secs_f64() * 1000.0,
            request_id: self.request_id,
        }
    }
}

// ---------------------------------------------------------------------------
// Background export task
// ---------------------------------------------------------------------------

/// Start the span export pipeline. Returns a `SpanExporter` handle that the
/// trace middleware uses to submit spans.
///
/// If `otel_endpoint` is `None`, spans are consumed from the channel and
/// discarded (the middleware still records them for structured logging).
pub fn start_exporter(otel_endpoint: Option<String>, service_name: String) -> SpanExporter {
    // Bounded channel — if the consumer falls behind, newest spans are dropped.
    let (tx, rx) = mpsc::channel::<FinishedSpan>(4096);

    tokio::spawn(export_loop(rx, otel_endpoint, service_name));

    SpanExporter { tx }
}

async fn export_loop(
    mut rx: mpsc::Receiver<FinishedSpan>,
    endpoint: Option<String>,
    service_name: String,
) {
    let client = endpoint.as_ref().map(|_| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("failed to build reqwest client for OTEL export")
    });

    // Batch spans to reduce HTTP overhead.
    let mut batch: Vec<FinishedSpan> = Vec::with_capacity(64);
    let flush_interval = tokio::time::interval(std::time::Duration::from_secs(2));
    tokio::pin!(flush_interval);

    loop {
        tokio::select! {
            span = rx.recv() => {
                match span {
                    Some(s) => {
                        batch.push(s);
                        // Flush immediately if batch is large enough.
                        if batch.len() >= 64 {
                            flush_batch(&client, &endpoint, &service_name, &mut batch).await;
                        }
                    }
                    None => {
                        // Channel closed — final flush and exit.
                        if !batch.is_empty() {
                            flush_batch(&client, &endpoint, &service_name, &mut batch).await;
                        }
                        break;
                    }
                }
            }
            _ = flush_interval.tick() => {
                if !batch.is_empty() {
                    flush_batch(&client, &endpoint, &service_name, &mut batch).await;
                }
            }
        }
    }
}

async fn flush_batch(
    client: &Option<reqwest::Client>,
    endpoint: &Option<String>,
    service_name: &str,
    batch: &mut Vec<FinishedSpan>,
) {
    let spans: Vec<FinishedSpan> = std::mem::take(batch);

    let (Some(client), Some(endpoint)) = (client, endpoint) else {
        // No endpoint configured — spans have been drained, nothing to send.
        return;
    };

    let payload = build_otlp_payload(service_name, &spans);

    let url = format!("{}/v1/traces", endpoint.trim_end_matches('/'));

    match client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            tracing::debug!(
                spans_exported = spans.len(),
                "OTEL spans exported successfully"
            );
        }
        Ok(resp) => {
            tracing::warn!(
                status = %resp.status(),
                spans = spans.len(),
                "OTEL collector returned non-success status"
            );
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                spans = spans.len(),
                "Failed to export OTEL spans"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// OTLP JSON payload construction
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OtlpExportRequest<'a> {
    resource_spans: Vec<ResourceSpans<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResourceSpans<'a> {
    resource: Resource<'a>,
    scope_spans: Vec<ScopeSpans>,
}

#[derive(Serialize)]
struct Resource<'a> {
    attributes: Vec<KeyValue<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeSpans {
    scope: Scope,
    spans: Vec<OtlpSpan>,
}

#[derive(Serialize)]
struct Scope {
    name: String,
    version: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OtlpSpan {
    trace_id: String,
    span_id: String,
    name: String,
    kind: u32,
    start_time_unix_nano: String,
    end_time_unix_nano: String,
    status: SpanStatus,
    attributes: Vec<KeyValue<'static>>,
}

#[derive(Serialize)]
struct SpanStatus {
    code: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct KeyValue<'a> {
    key: &'a str,
    value: AttributeValue<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum AttributeValue<'a> {
    StringValue(&'a str),
    IntValue(i64),
}

fn build_otlp_payload<'a>(service_name: &'a str, spans: &[FinishedSpan]) -> OtlpExportRequest<'a> {
    let otlp_spans: Vec<OtlpSpan> = spans
        .iter()
        .map(|s| {
            // OTLP status: 0 = Unset, 1 = Ok, 2 = Error
            let status_code = if s.status_code < 400 { 1 } else { 2 };

            OtlpSpan {
                trace_id: s.trace_id.clone(),
                span_id: s.span_id.clone(),
                name: s.name.clone(),
                kind: 2, // SPAN_KIND_SERVER
                start_time_unix_nano: s.start_time_unix_nano.to_string(),
                end_time_unix_nano: s.end_time_unix_nano.to_string(),
                status: SpanStatus { code: status_code },
                attributes: vec![
                    KeyValue {
                        key: "http.method",
                        value: AttributeValue::StringValue(
                            // Leak is acceptable here: method strings are a small fixed set.
                            Box::leak(s.http_method.clone().into_boxed_str()),
                        ),
                    },
                    KeyValue {
                        key: "http.target",
                        value: AttributeValue::StringValue(Box::leak(
                            s.http_path.clone().into_boxed_str(),
                        )),
                    },
                    KeyValue {
                        key: "http.status_code",
                        value: AttributeValue::IntValue(s.status_code as i64),
                    },
                    KeyValue {
                        key: "request.id",
                        value: AttributeValue::StringValue(Box::leak(
                            s.request_id.clone().into_boxed_str(),
                        )),
                    },
                    KeyValue {
                        key: "duration_ms",
                        value: AttributeValue::IntValue(s.duration_ms as i64),
                    },
                ],
            }
        })
        .collect();

    OtlpExportRequest {
        resource_spans: vec![ResourceSpans {
            resource: Resource {
                attributes: vec![KeyValue {
                    key: "service.name",
                    value: AttributeValue::StringValue(service_name),
                }],
            },
            scope_spans: vec![ScopeSpans {
                scope: Scope {
                    name: "paqtra-api".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
                spans: otlp_spans,
            }],
        }],
    }
}

// ---------------------------------------------------------------------------
// ID generation (hex-encoded random bytes)
// ---------------------------------------------------------------------------

fn generate_trace_id() -> String {
    use rand::RngExt;
    let bytes: [u8; 16] = rand::rng().random();
    hex_encode(&bytes)
}

fn generate_span_id() -> String {
    use rand::RngExt;
    let bytes: [u8; 8] = rand::rng().random();
    hex_encode(&bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_id_length() {
        let id = generate_trace_id();
        assert_eq!(id.len(), 32, "trace ID should be 32 hex chars");
    }

    #[test]
    fn span_id_length() {
        let id = generate_span_id();
        assert_eq!(id.len(), 16, "span ID should be 16 hex chars");
    }

    #[test]
    fn span_builder_produces_valid_span() {
        let builder = SpanBuilder::start("GET", "/api/v1/flows", "req-123");
        let span = builder.finish(200);

        assert_eq!(span.http_method, "GET");
        assert_eq!(span.http_path, "/api/v1/flows");
        assert_eq!(span.status_code, 200);
        assert_eq!(span.request_id, "req-123");
        assert!(span.duration_ms >= 0.0);
        assert_eq!(span.trace_id.len(), 32);
        assert_eq!(span.span_id.len(), 16);
        assert!(span.end_time_unix_nano >= span.start_time_unix_nano);
    }

    #[test]
    fn otlp_payload_structure() {
        let span = FinishedSpan {
            trace_id: "a".repeat(32),
            span_id: "b".repeat(16),
            name: "GET /test".to_string(),
            start_time_unix_nano: 1000,
            end_time_unix_nano: 2000,
            status_code: 200,
            http_method: "GET".to_string(),
            http_path: "/test".to_string(),
            duration_ms: 1.0,
            request_id: "req-1".to_string(),
        };

        let payload = build_otlp_payload("test-service", &[span]);

        assert_eq!(payload.resource_spans.len(), 1);
        let rs = &payload.resource_spans[0];
        assert_eq!(rs.resource.attributes.len(), 1);
        assert_eq!(rs.resource.attributes[0].key, "service.name");

        assert_eq!(rs.scope_spans.len(), 1);
        assert_eq!(rs.scope_spans[0].spans.len(), 1);

        let otlp_span = &rs.scope_spans[0].spans[0];
        assert_eq!(otlp_span.kind, 2); // SERVER
        assert_eq!(otlp_span.status.code, 1); // OK for 200

        // Verify JSON serialisation does not panic.
        let json = serde_json::to_string_pretty(&payload).unwrap_or_default();
        assert!(json.contains("\"service.name\""));
        assert!(json.contains("\"http.method\""));
    }

    #[test]
    fn error_status_code_maps_to_otel_error() {
        let span = FinishedSpan {
            trace_id: "a".repeat(32),
            span_id: "b".repeat(16),
            name: "POST /fail".to_string(),
            start_time_unix_nano: 1000,
            end_time_unix_nano: 2000,
            status_code: 500,
            http_method: "POST".to_string(),
            http_path: "/fail".to_string(),
            duration_ms: 5.0,
            request_id: "req-2".to_string(),
        };

        let payload = build_otlp_payload("test-service", &[span]);
        let otlp_span = &payload.resource_spans[0].scope_spans[0].spans[0];
        assert_eq!(otlp_span.status.code, 2); // ERROR for 500
    }
}
