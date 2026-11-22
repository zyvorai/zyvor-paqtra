// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Scrubbing secrets from what `paqtra sysdump` collects. A support bundle is
//! meant to be attached to a ticket, so anything that looks like a credential
//! is replaced before it is written, and Kubernetes Secrets are never read.
//!
//! This is defence in depth, not a promise that nothing sensitive remains:
//! the bundle still holds pod names, IPs and policy specs, and the CLI tells the
//! user to review it before sharing.

use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

pub const REDACTED: &str = "[REDACTED]";

/// Object keys whose values are credentials. Deliberately not a bare `key`:
/// Kubernetes uses `key:` for label selectors and config-map item keys.
static SENSITIVE_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(secret|token|passw(or)?d|passphrase|credential|private[-_]?key|api[-_]?key|access[-_]?key|jwt|authorization|bearer)",
    )
    .unwrap()
});

/// `scheme://<credentials>@host` -> `scheme://[REDACTED]@host`.
static URL_USERINFO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?P<scheme>[a-zA-Z][a-zA-Z0-9+.-]*://)[^/\s@]+@").unwrap());
/// A JWT: three base64url segments, the first two starting with `eyJ`.
static JWT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"eyJ[A-Za-z0-9_-]{5,}\.eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}").unwrap()
});
/// `Bearer <token>`: matched on its own because in `Authorization: Bearer abc`
/// the generic assignment rule would take the word "Bearer" for the value.
static BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?P<b>bearer\s+)[A-Za-z0-9._~+/=-]{8,}").unwrap());
/// `Authorization: Bearer abc`, `password=abc`, `token: abc`, `"apiKey":"abc"`.
static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?P<k>(?:bearer|authorization|passw(?:or)?d|passphrase|secret|token|api[-_]?key|access[-_]?key)["']?\s*[:= ]\s*["']?)(?P<v>[^\s"',;}]{4,})"#,
    )
    .unwrap()
});

pub fn is_sensitive_key(key: &str) -> bool {
    SENSITIVE_KEY.is_match(key)
}

/// Scrub one line of free text (a log line, a `helm history` row).
pub fn redact_text(line: &str) -> String {
    let s = URL_USERINFO.replace_all(line, format!("${{scheme}}{REDACTED}@").as_str());
    let s = JWT.replace_all(&s, REDACTED);
    let s = BEARER.replace_all(&s, format!("${{b}}{REDACTED}").as_str());
    ASSIGNMENT
        .replace_all(&s, format!("${{k}}{REDACTED}").as_str())
        .into_owned()
}

/// Scrub a JSON/YAML document in place:
///  - any value under a sensitive key is replaced,
///  - `{name: DB_PASSWORD, value: x}` env entries are replaced by name,
///  - every remaining string is scrubbed like free text (URLs with credentials,
///    JWTs).
///
/// `managedFields` are dropped as well: they are large, useless to a reader,
/// and repeat every field name that was ever set.
pub fn redact_value(v: &mut Value) {
    match v {
        Value::Object(map) => {
            map.remove("managedFields");
            // env / args style pair: {name: ..., value: ...}
            let name_is_sensitive = map
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(is_sensitive_key);
            if name_is_sensitive {
                if let Some(val) = map.get_mut("value") {
                    if val.is_string() {
                        *val = Value::String(REDACTED.into());
                    }
                }
            }
            for (k, child) in map.iter_mut() {
                if is_sensitive_key(k) && is_leaf(child) {
                    *child = Value::String(REDACTED.into());
                } else {
                    redact_value(child);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact_value),
        Value::String(s) => {
            let cleaned = redact_text(s);
            if cleaned != *s {
                *s = cleaned;
            }
        }
        _ => {}
    }
}

fn is_leaf(v: &Value) -> bool {
    matches!(v, Value::String(_) | Value::Number(_) | Value::Bool(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Test data that looks like credentials is assembled at run time, so secret
    // scanners do not flag literals in the source (nothing here is a real secret).
    fn pw() -> String {
        ["hun", "ter2"].concat()
    }
    fn url_with_login(host: &str) -> String {
        format!("http://{}:{}@{host}", "admin", pw())
    }
    fn fake_jwt() -> String {
        [
            "eyJhbGciOiJIUzI1NiJ9",
            "eyJzdWIiOiJhZG1pbiJ9",
            "c2lnbmF0dXJl",
        ]
        .join(".")
    }

    #[test]
    fn sensitive_keys_are_recognised_but_selector_keys_are_not() {
        for k in [
            "jwtSecret",
            "ADMIN_PASSWORD",
            "apiKey",
            "api-key",
            "accessKey",
            "authToken",
            "Authorization",
            "privateKey",
            "db_passwd",
        ] {
            assert!(is_sensitive_key(k), "{k}");
        }
        for k in [
            "key",
            "name",
            "hubbleAddress",
            "replicas",
            "operator",
            "matchLabels",
            "image",
            "port",
        ] {
            assert!(!is_sensitive_key(k), "{k}");
        }
    }

    #[test]
    fn urls_lose_their_credentials_but_keep_host_and_path() {
        assert_eq!(
            redact_text(&url_with_login("prom:9090/api")),
            "http://[REDACTED]@prom:9090/api"
        );
        assert_eq!(
            redact_text("dial https://tok@example.com/x failed"),
            "dial https://[REDACTED]@example.com/x failed"
        );
        assert_eq!(
            redact_text("http://prom:9090/api"),
            "http://prom:9090/api",
            "no userinfo, no change"
        );
    }

    #[test]
    fn jwts_in_log_lines_are_scrubbed() {
        let jwt = fake_jwt();
        let out = redact_text(&format!("auth ok {jwt} for admin"));
        assert!(
            !out.contains("eyJhbGci") && !out.contains("c2lnbmF0dXJl"),
            "{out}"
        );
        assert!(out.contains("for admin"));
    }

    #[test]
    fn bearer_tokens_and_assignments_are_scrubbed() {
        for line in [
            "Authorization: Bearer abcdef123456",
            "authorization=Bearer abcdef123456",
            "password=abcdef123456",
            "PASSWORD: abcdef123456",
            "{\"apiKey\":\"abcdef123456\"}",
            "token = abcdef123456 next",
        ] {
            let out = redact_text(line);
            assert!(!out.contains("abcdef123456"), "{line} -> {out}");
            assert!(out.contains(REDACTED), "{line} -> {out}");
        }
        assert_eq!(
            redact_text("nothing sensitive here: 42 pods"),
            "nothing sensitive here: 42 pods"
        );
    }

    #[test]
    fn values_under_sensitive_keys_are_replaced_but_structure_stays() {
        let mut v = json!({
            "api": {"env": {"jwtSecret": "s3cr3t", "hubbleMode": "grpc", "rustLog": "info"}},
            "adminPassword": pw(),
            "replicas": 2,
            "secret": {"secretName": "paqtra-secret", "items": [1, 2]},
        });
        redact_value(&mut v);
        assert_eq!(v["api"]["env"]["jwtSecret"], REDACTED);
        assert_eq!(v["api"]["env"]["hubbleMode"], "grpc");
        assert_eq!(v["adminPassword"], REDACTED);
        assert_eq!(v["replicas"], 2);
        // A sensitive key holding an object is descended into, not blanked wholesale.
        assert_eq!(v["secret"]["secretName"], REDACTED);
        assert_eq!(v["secret"]["items"], json!([1, 2]));
    }

    #[test]
    fn env_entries_are_redacted_by_name() {
        let mut v = json!({"env": [
            {"name": "DB_PASSWORD", "value": pw()},
            {"name": "HUBBLE_ADDRESS", "value": "relay:80"},
            {"name": "JWT_SECRET", "valueFrom": {"secretKeyRef": {"name": "s", "key": "k"}}},
        ]});
        redact_value(&mut v);
        assert_eq!(v["env"][0]["value"], REDACTED);
        assert_eq!(v["env"][1]["value"], "relay:80");
        // A reference is kept: it names a Secret, it does not hold one.
        assert_eq!(v["env"][2]["valueFrom"]["secretKeyRef"]["key"], "k");
    }

    #[test]
    fn selector_keys_survive() {
        let mut v =
            json!({"matchExpressions": [{"key": "app", "operator": "In", "values": ["web"]}]});
        let before = v.clone();
        redact_value(&mut v);
        assert_eq!(v, before);
    }

    #[test]
    fn strings_anywhere_are_scrubbed_and_managed_fields_dropped() {
        let mut v = json!({
            "metadata": {"name": "x", "managedFields": [{"manager": "kubectl"}]},
            "data": {"PROMETHEUS_URL": "http://u:p@prom:9090"},
        });
        redact_value(&mut v);
        assert!(v["metadata"].get("managedFields").is_none());
        assert_eq!(v["data"]["PROMETHEUS_URL"], "http://[REDACTED]@prom:9090");
        assert_eq!(v["metadata"]["name"], "x");
    }
}
