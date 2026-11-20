// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// CORS middleware
use axum::http::{HeaderValue, Method};
use tower_http::cors::CorsLayer;

/// Build a restrictive CORS layer.
/// In production, set ALLOWED_ORIGINS to the actual frontend origin(s).
pub fn cors_layer() -> CorsLayer {
    let origins_str = std::env::var("ALLOWED_ORIGINS").ok();

    let is_localhost = origins_str.is_none()
        || origins_str
            .as_ref()
            .is_some_and(|s| s.contains("localhost"));

    if is_localhost {
        tracing::warn!(
            "CORS configured with localhost origins — not suitable for production. \
             Set ALLOWED_ORIGINS env var to your frontend origin."
        );
    }

    let origins_raw =
        origins_str.unwrap_or_else(|| "http://localhost:3000,http://localhost:3001".to_string());

    let origins: Vec<HeaderValue> = origins_raw
        .split(',')
        .filter_map(|o| o.trim().parse().ok())
        .collect();

    let mut layer = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            axum::http::header::ACCEPT,
        ])
        .max_age(std::time::Duration::from_secs(3600));

    // Only allow credentials when using explicit (non-localhost) origins
    if !is_localhost {
        layer = layer.allow_credentials(true);
    }

    layer
}
