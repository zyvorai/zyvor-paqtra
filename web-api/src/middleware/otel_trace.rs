// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// OpenTelemetry distributed tracing middleware.
//
// Wraps each HTTP request in a trace span that records method, path, status
// code, duration, and correlation ID. Finished spans are submitted to the
// `SpanExporter` for asynchronous OTLP export.

use axum::{extract::Request, middleware::Next, response::Response};
use std::sync::Arc;

use crate::middleware::correlation::RequestId;
use crate::services::tracing_svc::{SpanBuilder, SpanExporter};

/// Axum middleware that wraps each request in an OTEL-compatible trace span.
///
/// Must run *after* the correlation-ID middleware so that `RequestId` is
/// available in request extensions.
pub async fn otel_trace_middleware(request: Request, next: Next) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();

    // Retrieve correlation ID set by the upstream middleware.
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .map(|r| r.0.clone())
        .unwrap_or_default();

    // Retrieve the exporter handle stashed in extensions by the app setup.
    let exporter: Option<Arc<SpanExporter>> =
        request.extensions().get::<Arc<SpanExporter>>().cloned();

    let span_builder = SpanBuilder::start(&method, &path, &request_id);

    let response = next.run(request).await;

    let status = response.status().as_u16();
    let finished = span_builder.finish(status);

    tracing::info!(
        otel.trace_id = %finished.trace_id,
        otel.span_id = %finished.span_id,
        http.method = %finished.http_method,
        http.target = %finished.http_path,
        http.status_code = finished.status_code,
        duration_ms = %format!("{:.2}", finished.duration_ms),
        request_id = %finished.request_id,
        "request completed"
    );

    if let Some(exporter) = exporter {
        exporter.export(finished);
    }

    response
}
