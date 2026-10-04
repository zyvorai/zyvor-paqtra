// Authentication middleware
use axum::{
    extract::{Request, State},
    http::{header, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
    pub iat: usize,
    pub role: String,
    #[serde(default)]
    pub namespaces: Vec<String>, // Empty = all namespaces (for backwards compat)
}

/// Authenticated users of any role may change their own password.
const SELF_SERVICE_PATH: &str = "/api/v1/auth/password";

/// Turn a validly signed token into the caller's *current* identity.
///
/// The configured admin account (ADMIN_USERNAME) is trusted as issued. Any other
/// subject must be an existing, enabled local user whose password has not been
/// changed since the token was issued; the role comes from the store, not the
/// token, so a role change or a disabled account applies immediately.
pub async fn resolve_claims(state: &AppState, mut claims: Claims) -> Result<Claims, &'static str> {
    if claims.sub == state.config.admin_username {
        return Ok(claims);
    }
    match crate::services::users::get(state, &claims.sub).await {
        Some(user) if user.accepts_token_issued_at(claims.iat as i64) => {
            claims.role = user.role.as_str().to_string();
            // The namespace limit, like the role, is read from the store, so an
            // admin narrowing or widening it applies to existing tokens at once.
            claims.namespaces = user.namespaces.clone();
            Ok(claims)
        }
        _ => Err("Account is disabled, changed or no longer exists"),
    }
}

/// The writes an `editor` may perform: operations, network policy changes, and
/// diagnostics. This list is the single place that defines the editor role, and
/// it is deny-by-default: a new mutating route is admin-only until it is added
/// here. `{x}` matches one path segment. Everything else that mutates
/// (users, notification channels, flow exports, node drain, rollback, chaos,
/// cluster sync) stays admin-only.
pub const EDITOR_WRITES: &[(&str, &str)] = &[
    // Incidents
    ("POST", "/api/v1/incidents"),
    ("POST", "/api/v1/incidents/{id}/ack"),
    ("POST", "/api/v1/incidents/{id}/resolve"),
    ("POST", "/api/v1/incidents/{id}/notes"),
    // Alert rules and silences
    ("POST", "/api/v1/alerts/rules"),
    ("PUT", "/api/v1/alerts/rules/{id}"),
    ("PUT", "/api/v1/alerts/rules/{id}/definition"),
    ("DELETE", "/api/v1/alerts/rules/{id}"),
    ("POST", "/api/v1/alerts/silences"),
    ("DELETE", "/api/v1/alerts/silences/{id}"),
    // Metric alert acks and silences
    ("POST", "/api/v1/metrics/alerts/{id}/ack"),
    ("POST", "/api/v1/metrics/silences"),
    ("DELETE", "/api/v1/metrics/silences/{id}"),
    // SLOs
    ("POST", "/api/v1/slo/targets"),
    ("DELETE", "/api/v1/slo/targets/{id}"),
    // Read-only analysis that happens to be a POST
    ("POST", "/api/v1/policies/simulate"),
    ("POST", "/api/v1/policies/validate"),
    ("POST", "/api/v1/modules/rootcause/analyze"),
    ("POST", "/api/v1/investigate/path"),
    ("POST", "/api/v1/investigate/flow"),
    ("POST", "/api/v1/investigate/bundles/{id}/share"),
    ("POST", "/api/v1/connectivity/paths"),
    ("DELETE", "/api/v1/connectivity/paths/{id}"),
    ("POST", "/api/v1/connectivity/alerts/{id}/silence"),
    ("POST", "/api/v1/compliance/audit"),
    // Network policies
    ("POST", "/api/v1/policies"),
    ("PUT", "/api/v1/policies/{id}"),
    ("DELETE", "/api/v1/policies/{id}"),
    ("POST", "/api/v1/policies/{id}/rules"),
    ("PUT", "/api/v1/policies/{id}/rules"),
    ("DELETE", "/api/v1/policies/{id}/rules"),
    ("POST", "/api/v1/policies/templates/{id}/apply"),
    ("POST", "/api/v1/modules/autopolicy/generate"),
    ("POST", "/api/v1/modules/healer/{id}/fix"),
    // Diagnostics, packet capture and replay
    ("POST", "/api/v1/diagnostics/run"),
    ("POST", "/api/v1/diagnostics/connectivity"),
    ("POST", "/api/v1/troubleshoot/run"),
    ("POST", "/api/v1/modules/capture/start"),
    ("POST", "/api/v1/modules/capture/{id}/stop"),
    ("POST", "/api/v1/modules/replay/start"),
    ("POST", "/api/v1/modules/replay/{id}/stop"),
];

/// A user limited to namespaces (a non-admin with a non-empty list) may call only
/// these endpoints, each of which filters or checks by namespace. Everything else
/// (nodes, eBPF, heatmap, DNS, anomalies, audit, incidents, ...) returns
/// cluster-wide data and is refused. Deny-by-default, like `EDITOR_WRITES`.
///
/// Only exact paths and method-unique `{id}` routes are listed. `GET
/// /flows/{id}` and `GET /policies/{id}` are deliberately absent: the pattern
/// would also match static routes such as `/flows/stats` and `/flows/exports`.
/// Scoped users find single items in the list endpoints instead.
pub const SCOPED_ACCESS: &[(&str, &str)] = &[
    ("GET", "/api/v1/flows"),
    // Both filter by the caller's namespaces inside the query itself.
    ("GET", "/api/v1/flows/history"),
    ("GET", "/api/v1/flows/history/timeline"),
    ("GET", "/api/v1/policies"),
    ("GET", "/api/v1/events"),
    ("GET", "/api/v1/endpoints"),
    // Changes to policies still need the editor role (EDITOR_WRITES) and a
    // namespace check in the handler.
    ("POST", "/api/v1/policies"),
    ("POST", "/api/v1/policies/simulate"),
    ("PUT", "/api/v1/policies/{id}"),
    ("DELETE", "/api/v1/policies/{id}"),
    ("POST", "/api/v1/policies/{id}/rules"),
    ("PUT", "/api/v1/policies/{id}/rules"),
    ("DELETE", "/api/v1/policies/{id}/rules"),
];

/// Paths every signed-in user may call, whatever their namespace limit.
const ALWAYS_ALLOWED: &[&str] = &["/api/v1/auth/me", SELF_SERVICE_PATH];

fn is_scoped(claims: &Claims) -> bool {
    claims.role != "admin" && !claims.namespaces.is_empty()
}

fn scoped_may_call(method: &Method, path: &str) -> bool {
    ALWAYS_ALLOWED.contains(&path)
        || SCOPED_ACCESS
            .iter()
            .any(|(m, pattern)| *m == method.as_str() && path_matches(pattern, path))
}

/// Whether `path` matches a route pattern where `{name}` matches one segment.
fn path_matches(pattern: &str, path: &str) -> bool {
    let pat: Vec<&str> = pattern.trim_matches('/').split('/').collect();
    let got: Vec<&str> = path.trim_matches('/').split('/').collect();
    pat.len() == got.len()
        && pat
            .iter()
            .zip(&got)
            .all(|(p, g)| (p.starts_with('{') && !g.is_empty()) || p == g)
}

fn editor_may_write(method: &Method, path: &str) -> bool {
    EDITOR_WRITES
        .iter()
        .any(|(m, pattern)| *m == method.as_str() && path_matches(pattern, path))
}

/// What a role may do with a request. Unknown roles fail closed.
#[derive(Debug, PartialEq, Eq)]
enum Access {
    Allowed,
    /// The role is read-only.
    ReadOnly,
    /// The role may write, but not this.
    NotPermitted,
}

fn access(role: &str, method: &Method, path: &str) -> Access {
    if is_read_method(method) || path == SELF_SERVICE_PATH {
        return Access::Allowed;
    }
    match role {
        "admin" => Access::Allowed,
        "editor" if editor_may_write(method, path) => Access::Allowed,
        "editor" => Access::NotPermitted,
        _ => Access::ReadOnly,
    }
}

fn is_read_method(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// JWT authentication middleware that validates Bearer tokens.
/// Skips authentication for health and metrics endpoints.
/// Injects decoded Claims into request extensions for downstream RBAC checks.
pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Response {
    // Skip auth for health/metrics endpoints and WebSocket upgrades
    // (WebSocket handlers validate tokens via query parameter instead)
    let path = request.uri().path();
    if path == "/health"
        || path == "/ready"
        || path == "/metrics"
        || path == "/api/v1/auth/login"
        // Agent metrics ingest checks X-Paqtra-Agent-Key in the handler.
        || path == "/api/v1/agents/metrics"
        || path.starts_with("/api/v1/ws/")
        || path.starts_with("/api-docs/")
        || path == "/swagger-ui"
    {
        return next.run(request).await;
    }

    // Skip auth when disabled at startup (dev/demo mode only)
    if state.config.auth_disabled {
        return next.run(request).await;
    }

    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());

    let token = match auth_header {
        Some(header) if header.starts_with("Bearer ") => &header[7..],
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Missing or invalid Authorization header"})),
            )
                .into_response();
        }
    };

    let decoding_key = DecodingKey::from_secret(state.config.jwt_secret.as_bytes());
    let validation = Validation::new(Algorithm::HS256);

    match decode::<Claims>(token, &decoding_key, &validation) {
        Ok(token_data) => {
            let claims = match resolve_claims(&state, token_data.claims).await {
                Ok(c) => c,
                Err(msg) => {
                    return (StatusCode::UNAUTHORIZED, Json(json!({ "error": msg })))
                        .into_response();
                }
            };
            if is_scoped(&claims) && !scoped_may_call(request.method(), request.uri().path()) {
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "error": format!(
                            "Your account is limited to the namespaces {}: this endpoint is not available to it",
                            claims.namespaces.join(", ")
                        )
                    })),
                )
                    .into_response();
            }
            match access(&claims.role, request.method(), request.uri().path()) {
                Access::Allowed => {}
                denied => {
                    let msg = if denied == Access::ReadOnly {
                        "Your role is read-only"
                    } else {
                        "Your role is not permitted to do this"
                    };
                    return (StatusCode::FORBIDDEN, Json(json!({ "error": msg }))).into_response();
                }
            }
            // Inject claims into request extensions for RBAC checks
            request.extensions_mut().insert(claims);
            next.run(request).await
        }
        Err(e) => {
            tracing::warn!("JWT validation failed: {}", e);
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Invalid or expired token"})),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADMIN_ONLY_WRITES: &[(&str, &str)] = &[
        ("POST", "/api/v1/users"),
        ("PUT", "/api/v1/users/bob"),
        ("DELETE", "/api/v1/users/bob"),
        ("POST", "/api/v1/alerts/channels"),
        ("DELETE", "/api/v1/alerts/channels/ch-1"),
        ("POST", "/api/v1/alerts/channels/ch-1/test"),
        ("POST", "/api/v1/flows/exports"),
        ("DELETE", "/api/v1/flows/exports/x"),
        ("POST", "/api/v1/nodes/drain"),
        ("POST", "/api/v1/nodes/uncordon"),
        ("POST", "/api/v1/changes/chg-1/rollback"),
        ("POST", "/api/v1/modules/chaos/run"),
        ("POST", "/api/v1/modules/multicluster/east/sync"),
        ("POST", "/api/v1/clustermesh/connect"),
        ("POST", "/api/v1/modules/mirror/rules"),
        ("DELETE", "/api/v1/modules/mirror/rules/m1"),
        ("POST", "/api/v1/anomalies/a1/remediate"),
        // A route nobody has classified yet: admin-only by default.
        ("POST", "/api/v1/brand-new-endpoint"),
    ];

    #[test]
    fn admin_may_do_anything() {
        for (m, p) in ADMIN_ONLY_WRITES.iter().chain(EDITOR_WRITES) {
            let method: Method = m.parse().unwrap();
            assert_eq!(
                access("admin", &method, &p.replace("{id}", "x")),
                Access::Allowed,
                "{m} {p}"
            );
        }
    }

    #[test]
    fn editor_may_do_exactly_the_listed_writes() {
        for (m, p) in EDITOR_WRITES {
            let method: Method = m.parse().unwrap();
            assert_eq!(
                access("editor", &method, &p.replace("{id}", "abc")),
                Access::Allowed,
                "{m} {p}"
            );
        }
        for (m, p) in ADMIN_ONLY_WRITES {
            let method: Method = m.parse().unwrap();
            assert_eq!(
                access("editor", &method, p),
                Access::NotPermitted,
                "{m} {p}"
            );
        }
    }

    #[test]
    fn editor_writes_are_method_specific() {
        // POST /policies is allowed; PATCH and an unlisted verb are not.
        assert_eq!(
            access("editor", &Method::POST, "/api/v1/policies"),
            Access::Allowed
        );
        assert_eq!(
            access("editor", &Method::PATCH, "/api/v1/policies"),
            Access::NotPermitted
        );
        assert_eq!(
            access("editor", &Method::DELETE, "/api/v1/incidents/x/ack"),
            Access::NotPermitted
        );
    }

    #[test]
    fn rule_edits_need_editor_and_stay_namespace_scoped() {
        for m in [Method::POST, Method::PUT, Method::DELETE] {
            let path = "/api/v1/policies/team-a%2Fdb/rules";
            assert_eq!(access("editor", &m, path), Access::Allowed, "{m}");
            assert_eq!(access("viewer", &m, path), Access::ReadOnly, "{m}");
            assert!(scoped_may_call(&m, path), "{m} scoped");
        }
        assert_eq!(
            access("editor", &Method::PATCH, "/api/v1/policies/x/rules"),
            Access::NotPermitted
        );
    }

    #[test]
    fn viewers_and_unknown_roles_are_read_only() {
        for role in ["viewer", "", "root", "Admin", "EDITOR"] {
            assert_eq!(
                access(role, &Method::POST, "/api/v1/incidents"),
                Access::ReadOnly,
                "{role:?}"
            );
            assert_eq!(
                access(role, &Method::DELETE, "/api/v1/policies/x"),
                Access::ReadOnly,
                "{role:?}"
            );
        }
    }

    #[test]
    fn everyone_can_read_and_change_their_own_password() {
        for role in ["admin", "editor", "viewer", "weird"] {
            assert_eq!(access(role, &Method::GET, "/api/v1/users"), Access::Allowed);
            assert_eq!(
                access(role, &Method::POST, SELF_SERVICE_PATH),
                Access::Allowed
            );
        }
    }

    #[test]
    fn path_matching() {
        assert!(path_matches(
            "/api/v1/incidents/{id}/ack",
            "/api/v1/incidents/inc-1/ack"
        ));
        assert!(path_matches(
            "/api/v1/incidents/{id}/ack",
            "/api/v1/incidents/inc-1/ack/"
        ));
        assert!(!path_matches(
            "/api/v1/incidents/{id}/ack",
            "/api/v1/incidents//ack"
        ));
        assert!(!path_matches(
            "/api/v1/incidents/{id}/ack",
            "/api/v1/incidents/inc-1/resolve"
        ));
        assert!(!path_matches(
            "/api/v1/incidents/{id}",
            "/api/v1/incidents/a/b"
        ));
        assert!(!path_matches(
            "/api/v1/incidents",
            "/api/v1/incidents/extra"
        ));
    }

    #[test]
    fn every_editor_write_is_a_real_route() {
        // Catches typos and drift: an entry that matches no route would silently
        // grant nothing (or be renamed away from the handler that enforces it).
        let main = include_str!("../main.rs");
        for (_, pattern) in EDITOR_WRITES {
            assert!(
                main.contains(&format!("\"{pattern}\"")),
                "{pattern} is not routed in main.rs"
            );
        }
    }

    fn scoped(role: &str) -> Claims {
        Claims {
            sub: "u".into(),
            exp: 0,
            iat: 0,
            role: role.into(),
            namespaces: vec!["team-a".into()],
        }
    }

    #[test]
    fn only_non_admins_with_a_list_are_scoped() {
        assert!(is_scoped(&scoped("viewer")));
        assert!(is_scoped(&scoped("editor")));
        assert!(!is_scoped(&scoped("admin")), "admins are never scoped");
        let mut unlimited = scoped("viewer");
        unlimited.namespaces.clear();
        assert!(!is_scoped(&unlimited), "an empty list means all namespaces");
    }

    #[test]
    fn scoped_users_reach_only_the_namespace_aware_endpoints() {
        for (m, p) in SCOPED_ACCESS {
            let method: Method = m.parse().unwrap();
            assert!(
                scoped_may_call(&method, &p.replace("{id}", "team-a")),
                "{m} {p}"
            );
        }
        assert!(scoped_may_call(&Method::GET, "/api/v1/auth/me"));
        assert!(scoped_may_call(&Method::POST, SELF_SERVICE_PATH));
        // Cluster-wide data and the routes that `{id}` patterns would shadow.
        for (m, p) in [
            ("GET", "/api/v1/flows/stats"),
            ("GET", "/api/v1/flows/exports"),
            ("GET", "/api/v1/flows/abc"),
            ("GET", "/api/v1/policies/abc"),
            ("GET", "/api/v1/policies/templates"),
            ("GET", "/api/v1/nodes"),
            ("GET", "/api/v1/ebpf/summary"),
            ("GET", "/api/v1/heatmap"),
            ("GET", "/api/v1/dns/queries"),
            ("GET", "/api/v1/audit/log"),
            ("GET", "/api/v1/incidents"),
            ("GET", "/api/v1/users"),
            ("POST", "/api/v1/incidents"),
            ("POST", "/api/v1/policies/validate"),
            ("POST", "/api/v1/modules/capture/start"),
            ("PATCH", "/api/v1/policies/abc"),
        ] {
            let method: Method = m.parse().unwrap();
            assert!(
                !scoped_may_call(&method, p),
                "{m} {p} must be refused to scoped users"
            );
        }
    }

    #[test]
    fn every_scoped_entry_is_a_real_route() {
        let main = include_str!("../main.rs");
        for (_, pattern) in SCOPED_ACCESS {
            assert!(
                main.contains(&format!("\"{pattern}\"")),
                "{pattern} is not routed in main.rs"
            );
        }
    }

    #[test]
    fn scoped_writes_are_a_subset_of_editor_writes() {
        // A scoped user can never write something an unscoped editor cannot.
        for (m, p) in SCOPED_ACCESS.iter().filter(|(m, _)| *m != "GET") {
            assert!(
                EDITOR_WRITES.contains(&(*m, *p)),
                "{m} {p} is scoped-allowed but not an editor write"
            );
        }
    }
}
