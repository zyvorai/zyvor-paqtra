// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Local user accounts.
//
// Users are stored in the cache under `cv:users:<username>` (durable when
// PAQTRA_DATA_DIR is set) with an Argon2id password hash. The account named in
// ADMIN_USERNAME is separate: it is configured through the environment, is
// never stored here, and always works as the break-glass admin.
//
// Tokens carry the username, but the *current* role and enabled state are read
// from the store on every request (see middleware/auth.rs), so disabling an
// account or changing its role takes effect immediately, and a password change
// invalidates tokens issued before it.

use crate::AppState;
use argon2::{
    password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier},
    Argon2,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::OnceLock;

pub const USERS_PREFIX: &str = "cv:users:";
pub const MIN_PASSWORD_LEN: usize = 12;
pub const MAX_PASSWORD_LEN: usize = 128;

/// `admin` can do everything. `editor` can do the writes listed in
/// `middleware::auth::EDITOR_WRITES` and nothing else. `viewer` is read-only.
/// Any other role name is treated as read-only by the auth middleware, so
/// unknown roles fail closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Editor,
    Viewer,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Editor => "editor",
            Role::Viewer => "viewer",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    /// Argon2id PHC string. Never returned by the API.
    pub password_hash: String,
    pub role: Role,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
    /// Unix seconds. Tokens issued before this are no longer accepted.
    pub password_changed_at: i64,
    /// Namespaces this user may see and change. Empty means all namespaces.
    /// Never set for admins, who are not scoped.
    #[serde(default)]
    pub namespaces: Vec<String>,
}

impl User {
    /// Representation safe to return from the API.
    pub fn public(&self) -> Value {
        json!({
            "username": self.username,
            "role": self.role,
            "enabled": self.enabled,
            "namespaces": self.namespaces,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
        })
    }

    /// Whether a token issued at `iat` (unix seconds) may still be used.
    pub fn accepts_token_issued_at(&self, iat: i64) -> bool {
        self.enabled && iat >= self.password_changed_at
    }
}

/// Lowercase and validate: 3-32 characters of a-z, 0-9, '.', '_' or '-',
/// starting with a letter or digit.
pub fn normalize_username(raw: &str) -> Result<String, &'static str> {
    let name = raw.trim().to_ascii_lowercase();
    let ok = (3..=32).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric());
    if ok {
        Ok(name)
    } else {
        Err("username must be 3-32 characters: letters, digits, '.', '_' or '-', starting with a letter or digit")
    }
}

/// Most namespaces one user can be limited to.
pub const MAX_NAMESPACES: usize = 50;

/// A Kubernetes DNS label: lowercase letters, digits and '-', up to 63
/// characters, not starting or ending with '-'.
pub fn valid_namespace_name(ns: &str) -> bool {
    !ns.is_empty()
        && ns.len() <= 63
        && !ns.starts_with('-')
        && !ns.ends_with('-')
        && ns
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Validate a namespace list: lowercased, de-duplicated and sorted. Empty means
/// "all namespaces"; `*` is refused so there is only one way to say that.
pub fn normalize_namespaces(raw: Vec<String>) -> Result<Vec<String>, &'static str> {
    let mut out: Vec<String> = Vec::new();
    for ns in raw {
        let ns = ns.trim().to_ascii_lowercase();
        if ns == "*" {
            return Err("use an empty namespace list for all namespaces, not \"*\"");
        }
        if !valid_namespace_name(&ns) {
            return Err(
                "each namespace must be a valid Kubernetes name (lowercase letters, digits, '-')",
            );
        }
        out.push(ns);
    }
    out.sort();
    out.dedup();
    if out.len() > MAX_NAMESPACES {
        return Err("at most 50 namespaces per user");
    }
    Ok(out)
}

/// Length only, deliberately: long passphrases beat composition rules.
pub fn validate_password(password: &str) -> Result<(), &'static str> {
    let n = password.chars().count();
    if n < MIN_PASSWORD_LEN {
        Err("password must be at least 12 characters")
    } else if n > MAX_PASSWORD_LEN {
        Err("password must be at most 128 characters")
    } else {
        Ok(())
    }
}

pub fn hash_password(password: &str) -> Result<String, String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| format!("password hashing failed: {e}"))
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Hash on the blocking pool: Argon2 is deliberately expensive and would stall
/// the async runtime.
pub async fn hash_password_async(password: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|e| format!("hashing task failed: {e}"))?
}

pub async fn verify_password_async(hash: String, password: String) -> bool {
    tokio::task::spawn_blocking(move || verify_password(&hash, &password))
        .await
        .unwrap_or(false)
}

/// Verify against a fixed hash and discard the result. Used when the account
/// does not exist or is disabled, so those logins take as long as real ones and
/// response time does not reveal which usernames exist.
pub async fn burn_verify(password: String) {
    static DUMMY: OnceLock<String> = OnceLock::new();
    let hash = DUMMY
        .get_or_init(|| hash_password("paqtra-timing-equalizer").unwrap_or_default())
        .clone();
    let _ = verify_password_async(hash, password).await;
}

pub fn now_epoch() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Cutoff for revoking existing tokens after a password change: one second
/// ahead. Token `iat` has one-second resolution and a token is accepted when
/// `iat >= cutoff`, so a cutoff of "now" would let a token issued earlier in the
/// same second survive the change. Login stamps new tokens with at least the
/// cutoff, so signing in again straight after a change still works.
pub fn revocation_cutoff() -> i64 {
    now_epoch() + 1
}

pub async fn get(state: &AppState, username: &str) -> Option<User> {
    state
        .cache
        .get(&format!("{USERS_PREFIX}{username}"))
        .await
        .ok()
        .flatten()
}

pub async fn list(state: &AppState) -> Vec<User> {
    let mut users: Vec<User> = state
        .cache
        .list_values(USERS_PREFIX)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();
    users.sort_by(|a, b| a.username.cmp(&b.username));
    users
}

pub async fn save(state: &AppState, user: &User) -> anyhow::Result<()> {
    state
        .cache
        .set_persistent(&format!("{USERS_PREFIX}{}", user.username), user)
        .await
}

pub async fn delete(state: &AppState, username: &str) -> anyhow::Result<()> {
    state
        .cache
        .delete(&format!("{USERS_PREFIX}{username}"))
        .await
}

/// Constant-time string comparison, for the environment-configured secrets.
pub fn secrets_equal(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;
    // Length is not secret here, and ct_eq on differing lengths returns false.
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(role: Role, enabled: bool, changed: i64) -> User {
        User {
            username: "alice".into(),
            password_hash: String::new(),
            role,
            enabled,
            created_at: String::new(),
            updated_at: String::new(),
            password_changed_at: changed,
            namespaces: Vec::new(),
        }
    }

    #[test]
    fn usernames_are_normalized_and_validated() {
        assert_eq!(
            normalize_username("  Alice.B-1 ").as_deref(),
            Ok("alice.b-1")
        );
        for bad in [
            "",
            "ab",
            "-abc",
            ".abc",
            "a b c",
            "abc!",
            "ü-user",
            &"x".repeat(33),
        ] {
            assert!(normalize_username(bad).is_err(), "{bad:?}");
        }
        assert!(normalize_username(&"x".repeat(32)).is_ok());
    }

    #[test]
    fn password_length_rules() {
        assert!(validate_password("short").is_err());
        assert!(validate_password("exactly12chr").is_ok());
        assert!(validate_password(&"a".repeat(128)).is_ok());
        assert!(validate_password(&"a".repeat(129)).is_err());
        // Counts characters, not bytes.
        assert!(validate_password("ééééééééééé").is_err());
        assert!(validate_password("éééééééééééé").is_ok());
    }

    #[test]
    fn hash_verifies_and_is_salted_argon2id() {
        let h1 = hash_password("correct horse battery").unwrap();
        let h2 = hash_password("correct horse battery").unwrap();
        assert!(h1.starts_with("$argon2id$"), "{h1}");
        assert_ne!(h1, h2, "each hash gets its own salt");
        assert!(verify_password(&h1, "correct horse battery"));
        assert!(!verify_password(&h1, "correct horse batterY"));
        assert!(!verify_password(&h1, ""));
    }

    #[test]
    fn malformed_hashes_never_verify() {
        for bad in [
            "",
            "plaintext",
            "$argon2id$garbage",
            "$2b$12$abcdefghijklmnopqrstuv",
        ] {
            assert!(!verify_password(bad, "anything"), "{bad:?}");
        }
    }

    #[test]
    fn public_view_never_contains_the_hash() {
        let mut u = user(Role::Viewer, true, 0);
        u.password_hash = "$argon2id$SECRET".into();
        let v = u.public().to_string();
        assert!(!v.contains("SECRET") && !v.contains("password"), "{v}");
        assert!(v.contains("\"role\":\"viewer\""));
    }

    #[test]
    fn tokens_are_refused_for_disabled_users_and_after_password_change() {
        let u = user(Role::Admin, true, 1000);
        assert!(u.accepts_token_issued_at(1000), "same second is accepted");
        assert!(u.accepts_token_issued_at(2000));
        assert!(
            !u.accepts_token_issued_at(999),
            "issued before the password change"
        );
        assert!(
            !user(Role::Admin, false, 0).accepts_token_issued_at(5000),
            "disabled"
        );
    }

    #[test]
    fn cutoff_revokes_tokens_issued_in_the_same_second() {
        let now = now_epoch();
        let u = user(Role::Admin, true, revocation_cutoff());
        assert!(
            !u.accepts_token_issued_at(now),
            "a token issued this second is revoked"
        );
        assert!(!u.accepts_token_issued_at(now - 60));
        // A token stamped max(now, cutoff) at the next login is accepted at once.
        assert!(u.accepts_token_issued_at(now.max(u.password_changed_at)));
    }

    #[test]
    fn namespace_lists_are_normalized() {
        let got = normalize_namespaces(vec![" Team-B ".into(), "team-a".into(), "TEAM-A".into()])
            .unwrap();
        assert_eq!(got, vec!["team-a", "team-b"]);
        assert_eq!(normalize_namespaces(vec![]).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn namespace_lists_reject_bad_input() {
        for bad in [
            "*",
            "",
            " ",
            "-a",
            "a-",
            "a_b",
            "a b",
            "a/b",
            "ns.x",
            &"x".repeat(64),
        ] {
            assert!(
                normalize_namespaces(vec![bad.to_string()]).is_err(),
                "{bad:?}"
            );
        }
        assert!(
            normalize_namespaces(vec!["ok".into(), "*".into()]).is_err(),
            "one bad entry fails the list"
        );
        let many: Vec<String> = (0..=MAX_NAMESPACES).map(|i| format!("ns-{i}")).collect();
        assert!(normalize_namespaces(many).is_err());
        assert!(
            normalize_namespaces((0..MAX_NAMESPACES).map(|i| format!("ns-{i}")).collect()).is_ok()
        );
        assert!(normalize_namespaces(vec!["x".repeat(63)]).is_ok());
    }

    #[test]
    fn public_view_lists_namespaces_and_old_records_default_to_all() {
        let mut u = user(Role::Viewer, true, 0);
        u.namespaces = vec!["team-a".into()];
        assert_eq!(u.public()["namespaces"], serde_json::json!(["team-a"]));
        // A record stored before this field existed must load as unscoped.
        let old = r#"{"username":"x","password_hash":"h","role":"viewer","enabled":true,"created_at":"","updated_at":"","password_changed_at":0}"#;
        let loaded: User = serde_json::from_str(old).unwrap();
        assert!(loaded.namespaces.is_empty());
    }

    #[test]
    fn secret_comparison() {
        assert!(secrets_equal("Admin@321", "Admin@321"));
        assert!(!secrets_equal("Admin@321", "Admin@322"));
        assert!(!secrets_equal("Admin@321", "Admin@3210"));
        assert!(!secrets_equal("", "x"));
        assert!(secrets_equal("", ""));
    }

    #[test]
    fn roles_serialize_lowercase_and_reject_unknown() {
        assert_eq!(serde_json::to_string(&Role::Viewer).unwrap(), "\"viewer\"");
        assert_eq!(
            serde_json::from_str::<Role>("\"admin\"").unwrap(),
            Role::Admin
        );
        assert!(serde_json::from_str::<Role>("\"root\"").is_err());
        assert_eq!(
            serde_json::from_str::<Role>("\"editor\"").unwrap(),
            Role::Editor
        );
        assert_eq!(Role::Editor.as_str(), "editor");
        assert!(serde_json::from_str::<Role>("\"Editor\"").is_err());
    }
}
