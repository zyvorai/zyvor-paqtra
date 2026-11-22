// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Request correlation ID middleware
use axum::{extract::Request, http::HeaderValue, middleware::Next, response::Response};

/// Middleware that assigns a unique correlation ID to every request.
///
/// If the incoming request already contains an `X-Request-Id` header the value
/// is reused; otherwise a new UUID v4 is generated. The ID is:
///
/// 1. Stored in the request extensions as `RequestId` (accessible by handlers).
/// 2. Emitted in a tracing span so every log line within the request carries it.
/// 3. Echoed back in the `X-Request-Id` response header.
pub async fn correlation_id_middleware(mut request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Store in request extensions for handlers
    request
        .extensions_mut()
        .insert(RequestId(request_id.clone()));

    // Run the inner handler inside a tracing span.
    // We use `Instrument::instrument` rather than `Span::entered()` so the
    // span guard does not need to be held across the await point (entered
    // guards are !Send and cannot cross `.await`).
    use tracing::Instrument;
    let span = tracing::info_span!("request", request_id = %request_id);

    let mut response = next.run(request).instrument(span).await;

    // Add to response headers
    if let Ok(val) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", val);
    }

    response
}

/// Wrapper type stored in request extensions so handlers can retrieve the
/// correlation ID via `axum::Extension<RequestId>`.
#[derive(Clone, Debug)]
pub struct RequestId(pub String);
