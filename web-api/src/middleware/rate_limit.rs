// Tiered per-endpoint rate limiter using token buckets keyed by (IP, tier).
//
// Rate tiers:
//   Exempt  – /health, /ready, /metrics, /swagger-ui  (no limit)
//   Heavy   – expensive read paths                     (20 req/s per IP)
//   Mutating– POST / PUT / DELETE on non-exempt paths  (10 req/s per IP)
//   Default – everything else                          (100 req/s per IP)

use axum::{
    extract::{ConnectInfo, Request},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::OnceLock;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Tier classification
// ---------------------------------------------------------------------------

/// The rate-limit tier a request belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RateTier {
    /// No limit applied.
    Exempt,
    /// Expensive read endpoints (20 req/s).
    Heavy,
    /// State-mutating methods (10 req/s).
    Mutating,
    /// Everything else (100 req/s).
    Default,
}

/// Path prefixes that belong to the Heavy tier.
const HEAVY_PREFIXES: &[&str] = &[
    "/api/v1/flows",
    "/api/v1/heatmap",
    "/api/v1/dependencies",
    "/api/v1/service-map",
    "/api/v1/servicemap",
    "/api/v1/security/findings",
];

/// Paths (exact) that are exempt from rate limiting.
/// The agent metrics ingest is authenticated by the agent key before any
/// decoding and is called once a second per node (more while catching up).
const EXEMPT_EXACT: &[&str] = &["/health", "/ready", "/metrics", "/api/v1/agents/metrics"];

/// Path prefixes that are exempt from rate limiting.
const EXEMPT_PREFIXES: &[&str] = &["/swagger-ui"];

fn classify(method: &Method, path: &str) -> RateTier {
    // Exempt check – exact matches and prefix matches.
    if EXEMPT_EXACT.contains(&path) {
        return RateTier::Exempt;
    }
    if EXEMPT_PREFIXES.iter().any(|pfx| path.starts_with(pfx)) {
        return RateTier::Exempt;
    }

    // Mutating methods take precedence over heavy-read classification so that
    // POST /api/v1/flows/exports is limited at the tighter mutating rate.
    if matches!(
        method,
        &Method::POST | &Method::PUT | &Method::DELETE | &Method::PATCH
    ) {
        return RateTier::Mutating;
    }

    // Heavy read paths.
    if HEAVY_PREFIXES.iter().any(|pfx| path.starts_with(pfx)) {
        return RateTier::Heavy;
    }

    RateTier::Default
}

// ---------------------------------------------------------------------------
// Token bucket
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    capacity: f64,
    refill_rate: f64,
}

impl TokenBucket {
    fn new(capacity: f64, refill_rate: f64) -> Self {
        Self {
            tokens: capacity,
            last_refill: Instant::now(),
            capacity,
            refill_rate,
        }
    }

    /// Try to consume one token. Refills based on elapsed time first.
    /// Returns `true` if the request is allowed.
    fn try_consume(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_refill = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Seconds until the next token is available (for Retry-After header).
    fn retry_after_secs(&self) -> u64 {
        if self.refill_rate <= 0.0 {
            return 1;
        }
        let deficit = 1.0 - self.tokens;
        let secs = (deficit / self.refill_rate).ceil() as u64;
        secs.max(1)
    }
}

// ---------------------------------------------------------------------------
// Per-IP bucket map
// ---------------------------------------------------------------------------

/// Per-IP state: one bucket per tier the IP has been seen on.
struct IpBuckets {
    buckets: HashMap<RateTier, TokenBucket>,
    /// Last activity timestamp – used for stale-entry eviction.
    last_seen: Instant,
}

/// Capacity and refill-rate for each tier.
fn tier_params(tier: RateTier) -> (f64, f64) {
    match tier {
        // (capacity / burst, sustained refill per second)
        RateTier::Heavy => (40.0, 20.0),
        RateTier::Mutating => (20.0, 10.0),
        RateTier::Default => (200.0, 100.0),
        RateTier::Exempt => (0.0, 0.0), // never used
    }
}

/// Global per-IP, per-tier rate limiter.
struct TieredRateLimiter {
    ips: DashMap<IpAddr, IpBuckets>,
    last_cleanup: std::sync::Mutex<Instant>,
}

impl TieredRateLimiter {
    fn new() -> Self {
        Self {
            ips: DashMap::new(),
            last_cleanup: std::sync::Mutex::new(Instant::now()),
        }
    }

    /// Check whether a request from `ip` in the given `tier` is allowed.
    /// Returns `Ok(())` if allowed, or `Err(retry_after_secs)` if denied.
    fn check(&self, ip: IpAddr, tier: RateTier) -> Result<(), u64> {
        if tier == RateTier::Exempt {
            return Ok(());
        }

        // Periodic cleanup – every 60 s, evict IPs not seen for 5 min.
        {
            if let Ok(mut last) = self.last_cleanup.try_lock() {
                if last.elapsed().as_secs() >= 60 {
                    *last = Instant::now();
                    let cutoff = Instant::now() - std::time::Duration::from_secs(300);
                    self.ips.retain(|_, v| v.last_seen > cutoff);
                }
            }
        }

        let (capacity, refill) = tier_params(tier);

        let mut entry = self.ips.entry(ip).or_insert_with(|| IpBuckets {
            buckets: HashMap::new(),
            last_seen: Instant::now(),
        });
        let ip_state = entry.value_mut();
        ip_state.last_seen = Instant::now();

        let bucket = ip_state
            .buckets
            .entry(tier)
            .or_insert_with(|| TokenBucket::new(capacity, refill));

        if bucket.try_consume() {
            Ok(())
        } else {
            Err(bucket.retry_after_secs())
        }
    }
}

// ---------------------------------------------------------------------------
// Axum middleware
// ---------------------------------------------------------------------------

/// Global singleton.
fn limiter() -> &'static TieredRateLimiter {
    static INSTANCE: OnceLock<TieredRateLimiter> = OnceLock::new();
    INSTANCE.get_or_init(TieredRateLimiter::new)
}

/// Axum middleware that applies tiered per-IP rate limiting.
///
/// Tier selection is based on the request path and HTTP method:
///   - Exempt: health / readiness / metrics / swagger-ui
///   - Heavy (20 req/s): flows, heatmap, dependencies, service-map, security findings
///   - Mutating (10 req/s): POST / PUT / DELETE / PATCH
///   - Default (100 req/s): everything else
pub async fn rate_limit_middleware(request: Request, next: Next) -> Response {
    let ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

    let tier = classify(request.method(), request.uri().path());

    match limiter().check(ip, tier) {
        Ok(()) => next.run(request).await,
        Err(retry_after) => {
            let retry_val = retry_after.to_string();
            (
                StatusCode::TOO_MANY_REQUESTS,
                [("retry-after", retry_val.as_str())],
                "Rate limit exceeded",
            )
                .into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_exempt_paths() {
        assert_eq!(classify(&Method::GET, "/health"), RateTier::Exempt);
        assert_eq!(classify(&Method::GET, "/ready"), RateTier::Exempt);
        assert_eq!(classify(&Method::GET, "/metrics"), RateTier::Exempt);
        assert_eq!(
            classify(&Method::GET, "/swagger-ui/index.html"),
            RateTier::Exempt
        );
    }

    #[test]
    fn classify_heavy_paths() {
        assert_eq!(classify(&Method::GET, "/api/v1/flows"), RateTier::Heavy);
        assert_eq!(
            classify(&Method::GET, "/api/v1/flows/stats"),
            RateTier::Heavy
        );
        assert_eq!(classify(&Method::GET, "/api/v1/heatmap"), RateTier::Heavy);
        assert_eq!(
            classify(&Method::GET, "/api/v1/dependencies"),
            RateTier::Heavy
        );
        assert_eq!(
            classify(&Method::GET, "/api/v1/servicemap"),
            RateTier::Heavy
        );
        assert_eq!(
            classify(&Method::GET, "/api/v1/security/findings"),
            RateTier::Heavy
        );
    }

    #[test]
    fn classify_mutating_methods() {
        // Mutating takes priority over heavy for write paths.
        assert_eq!(
            classify(&Method::POST, "/api/v1/flows/exports"),
            RateTier::Mutating
        );
        assert_eq!(
            classify(&Method::PUT, "/api/v1/policies/123"),
            RateTier::Mutating
        );
        assert_eq!(
            classify(&Method::DELETE, "/api/v1/policies/123"),
            RateTier::Mutating
        );
        assert_eq!(
            classify(&Method::PATCH, "/api/v1/nodes/1"),
            RateTier::Mutating
        );
    }

    #[test]
    fn classify_default() {
        assert_eq!(
            classify(&Method::GET, "/api/v1/policies"),
            RateTier::Default
        );
        assert_eq!(classify(&Method::GET, "/api/v1/nodes"), RateTier::Default);
    }

    #[test]
    fn token_bucket_basics() {
        let mut b = TokenBucket::new(5.0, 10.0);
        // Should allow 5 requests immediately (burst).
        for _ in 0..5 {
            assert!(b.try_consume());
        }
        // 6th should be denied (no time elapsed to refill).
        assert!(!b.try_consume());
    }

    #[test]
    fn tiered_limiter_respects_tiers() {
        let limiter = TieredRateLimiter::new();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();

        // Exempt always passes.
        for _ in 0..1000 {
            assert!(limiter.check(ip, RateTier::Exempt).is_ok());
        }

        // Mutating bucket: capacity 20 → 20 quick requests should succeed.
        for _ in 0..20 {
            assert!(limiter.check(ip, RateTier::Mutating).is_ok());
        }
        // 21st should fail.
        assert!(limiter.check(ip, RateTier::Mutating).is_err());

        // Default bucket is independent: capacity 200 → 200 quick requests.
        for _ in 0..200 {
            assert!(limiter.check(ip, RateTier::Default).is_ok());
        }
        assert!(limiter.check(ip, RateTier::Default).is_err());
    }
}
