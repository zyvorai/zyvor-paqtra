// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Alert notification delivery.
//
// Sends firing/resolved alert events to admin-configured channels (generic
// webhook, Slack incoming webhook, PagerDuty Events v2). Channels and silences
// are stored in the cache under `cv:notify_channels:` / `cv:silences:`, so they
// survive restarts when PAQTRA_DATA_DIR is set. Delivery is concurrent, has a
// per-request timeout, and retries transient failures (network errors, 429,
// 5xx) with backoff. Client errors (4xx) are not retried.

use crate::AppState;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::Duration;

pub const CHANNELS_PREFIX: &str = "cv:notify_channels:";
pub const SILENCES_PREFIX: &str = "cv:silences:";

const PAGERDUTY_EVENTS_URL: &str = "https://events.pagerduty.com/v2/enqueue";
const MAX_ATTEMPTS: u32 = 3;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelKind {
    Webhook,
    Slack,
    Pagerduty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub kind: ChannelKind,
    /// Webhook/Slack: destination URL. PagerDuty: Events v2 routing key.
    /// Contains a credential, so it is masked in API responses.
    pub target: String,
    /// Only deliver alerts at or above this severity (critical/high/warning/info).
    #[serde(default)]
    pub min_severity: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub created_at: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Firing,
    Resolved,
    Test,
}

impl EventKind {
    fn as_str(self) -> &'static str {
        match self {
            EventKind::Firing => "firing",
            EventKind::Resolved => "resolved",
            EventKind::Test => "test",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AlertEvent {
    pub kind: EventKind,
    pub rule_id: String,
    pub rule_name: String,
    pub severity: String,
    pub message: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeliveryResult {
    pub channel_id: String,
    pub channel_name: String,
    pub delivered: bool,
    pub attempts: u32,
    pub error: Option<String>,
}

/// Numeric rank of a severity name; unknown values rank as warning.
pub fn severity_rank(severity: &str) -> u8 {
    match severity.to_ascii_lowercase().as_str() {
        "critical" => 4,
        "high" | "error" => 3,
        "warning" | "warn" => 2,
        "info" => 1,
        _ => 2,
    }
}

pub fn is_valid_severity(severity: &str) -> bool {
    matches!(
        severity.to_ascii_lowercase().as_str(),
        "critical" | "high" | "warning" | "info"
    )
}

impl Channel {
    /// Whether this channel wants an event of the given severity.
    pub fn accepts(&self, severity: &str) -> bool {
        self.enabled
            && self
                .min_severity
                .as_deref()
                .is_none_or(|min| severity_rank(severity) >= severity_rank(min))
    }

    /// Representation safe to return from the API: the credential is masked.
    pub fn masked(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "kind": self.kind,
            "target": mask_target(self.kind, &self.target),
            "min_severity": self.min_severity,
            "enabled": self.enabled,
            "created_at": self.created_at,
        })
    }
}

/// Webhook/Slack URLs keep scheme and host (paths often embed tokens);
/// PagerDuty keys keep only the first four characters.
pub fn mask_target(kind: ChannelKind, target: &str) -> String {
    match kind {
        ChannelKind::Pagerduty => {
            let head: String = target.chars().take(4).collect();
            format!("{head}…")
        }
        _ => match target.split_once("://") {
            Some((scheme, rest)) => {
                let host = rest.split('/').next().unwrap_or("");
                format!("{scheme}://{host}/…")
            }
            None => "…".to_string(),
        },
    }
}

fn pagerduty_severity(severity: &str) -> &'static str {
    match severity_rank(severity) {
        4 => "critical",
        3 => "error",
        2 => "warning",
        _ => "info",
    }
}

fn slack_text(event: &AlertEvent) -> String {
    let head = match event.kind {
        EventKind::Firing => format!("[{}] {}", event.severity.to_uppercase(), event.rule_name),
        EventKind::Resolved => format!("[RESOLVED] {}", event.rule_name),
        EventKind::Test => format!("[TEST] {}", event.rule_name),
    };
    format!("{head}\n{}", event.message)
}

/// Request body for a channel and event.
pub fn build_payload(channel: &Channel, event: &AlertEvent) -> serde_json::Value {
    match channel.kind {
        ChannelKind::Webhook => serde_json::json!({
            "event": event.kind.as_str(),
            "alert": {
                "rule_id": event.rule_id,
                "rule_name": event.rule_name,
                "severity": event.severity,
                "message": event.message,
                "timestamp": event.at,
            },
            "source": "paqtra",
        }),
        ChannelKind::Slack => serde_json::json!({ "text": slack_text(event) }),
        ChannelKind::Pagerduty => serde_json::json!({
            "routing_key": channel.target,
            // dedup_key ties trigger and resolve events for one rule together.
            "dedup_key": format!("paqtra-{}", event.rule_id),
            "event_action": if event.kind == EventKind::Resolved { "resolve" } else { "trigger" },
            "payload": {
                "summary": format!("{}: {}", event.rule_name, event.message),
                "source": "paqtra",
                "severity": pagerduty_severity(&event.severity),
                "timestamp": event.at,
            },
        }),
    }
}

fn destination(channel: &Channel, pagerduty_url: &str) -> String {
    match channel.kind {
        ChannelKind::Pagerduty => pagerduty_url.to_string(),
        _ => channel.target.clone(),
    }
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("failed to build reqwest client for notifications")
    })
}

/// Deliver one event to one channel, retrying transient failures.
pub async fn deliver(channel: &Channel, event: &AlertEvent) -> DeliveryResult {
    deliver_to(
        channel,
        event,
        PAGERDUTY_EVENTS_URL,
        Duration::from_millis(500),
    )
    .await
}

async fn deliver_to(
    channel: &Channel,
    event: &AlertEvent,
    pagerduty_url: &str,
    backoff: Duration,
) -> DeliveryResult {
    let url = destination(channel, pagerduty_url);
    let body = build_payload(channel, event);
    let mut result = DeliveryResult {
        channel_id: channel.id.clone(),
        channel_name: channel.name.clone(),
        delivered: false,
        attempts: 0,
        error: None,
    };

    for attempt in 1..=MAX_ATTEMPTS {
        result.attempts = attempt;
        let retryable = match http_client().post(&url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                result.delivered = true;
                result.error = None;
                return result;
            }
            Ok(resp) => {
                let status = resp.status();
                result.error = Some(format!("HTTP {status}"));
                status.as_u16() == 429 || status.is_server_error()
            }
            Err(e) => {
                // Do not include the URL: it may embed a credential.
                result.error = Some(format!("request failed: {}", e.without_url()));
                true
            }
        };
        if !retryable || attempt == MAX_ATTEMPTS {
            break;
        }
        tokio::time::sleep(backoff * attempt).await;
    }
    result
}

pub async fn list_channels(state: &AppState) -> Vec<Channel> {
    state
        .cache
        .list_values(CHANNELS_PREFIX)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect()
}

/// Whether alerts for `rule_id` are currently silenced. A silence with no
/// `rule_id` silences every rule. Expired silences are dropped by the cache.
pub async fn is_silenced(state: &AppState, rule_id: &str) -> bool {
    state
        .cache
        .list_values(SILENCES_PREFIX)
        .await
        .unwrap_or_default()
        .iter()
        .any(|s| match s.get("rule_id").and_then(|v| v.as_str()) {
            Some(id) => id == rule_id,
            None => true,
        })
}

/// Send an event to every enabled channel that accepts its severity.
/// Channels are delivered concurrently; failures are logged, never raised.
pub async fn notify(state: &AppState, event: AlertEvent) -> Vec<DeliveryResult> {
    let channels: Vec<Channel> = list_channels(state)
        .await
        .into_iter()
        .filter(|c| c.accepts(&event.severity))
        .collect();

    let mut set = tokio::task::JoinSet::new();
    for channel in channels {
        let event = event.clone();
        set.spawn(async move { deliver(&channel, &event).await });
    }

    let mut results = Vec::new();
    while let Some(joined) = set.join_next().await {
        if let Ok(r) = joined {
            if r.delivered {
                tracing::info!(channel = %r.channel_name, attempts = r.attempts, "Alert notification delivered");
            } else {
                tracing::warn!(channel = %r.channel_name, attempts = r.attempts, error = ?r.error, "Alert notification failed");
            }
            results.push(r);
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn channel(kind: ChannelKind, target: &str) -> Channel {
        Channel {
            id: "ch-1".into(),
            name: "test".into(),
            kind,
            target: target.into(),
            min_severity: None,
            enabled: true,
            created_at: String::new(),
        }
    }

    fn event(kind: EventKind) -> AlertEvent {
        AlertEvent {
            kind,
            rule_id: "rule-001".into(),
            rule_name: "High Drop Rate".into(),
            severity: "critical".into(),
            message: "Drop rate 9.0% exceeds 5.0%".into(),
            at: "2026-09-24T00:00:00Z".into(),
        }
    }

    /// Serve `responses` (status codes) to successive connections and return
    /// the request bodies received.
    async fn mock_server(responses: Vec<u16>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/hook", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            let mut bodies = Vec::new();
            for status in responses {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 8192];
                let mut read = 0;
                // Read headers, then the body up to Content-Length.
                loop {
                    let n = sock.read(&mut buf[read..]).await.unwrap();
                    read += n;
                    let text = String::from_utf8_lossy(&buf[..read]).to_string();
                    if let Some(idx) = text.find("\r\n\r\n") {
                        let len = text
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if read >= idx + 4 + len || n == 0 {
                            bodies.push(text[idx + 4..].to_string());
                            break;
                        }
                    } else if n == 0 {
                        break;
                    }
                }
                let resp = format!(
                    "HTTP/1.1 {status} X\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                );
                sock.write_all(resp.as_bytes()).await.unwrap();
            }
            bodies
        });
        (url, handle)
    }

    #[test]
    fn severity_filter() {
        let mut c = channel(ChannelKind::Slack, "https://hooks.slack.com/x");
        assert!(c.accepts("info"));
        c.min_severity = Some("high".into());
        assert!(c.accepts("critical"));
        assert!(c.accepts("high"));
        assert!(!c.accepts("warning"));
        c.enabled = false;
        assert!(!c.accepts("critical"));
    }

    #[test]
    fn masking_hides_credentials() {
        let slack = mask_target(
            ChannelKind::Slack,
            "https://hooks.slack.com/services/T0/B0/SECRET",
        );
        assert_eq!(slack, "https://hooks.slack.com/…");
        assert!(!slack.contains("SECRET"));
        assert_eq!(
            mask_target(ChannelKind::Pagerduty, "abcd1234secret"),
            "abcd…"
        );
        let masked = channel(ChannelKind::Webhook, "https://h.example/path?token=SECRET").masked();
        assert!(!masked.to_string().contains("SECRET"));
    }

    #[test]
    fn pagerduty_payload_triggers_and_resolves_on_same_dedup_key() {
        let c = channel(ChannelKind::Pagerduty, "routingkey");
        let fire = build_payload(&c, &event(EventKind::Firing));
        let resolve = build_payload(&c, &event(EventKind::Resolved));
        assert_eq!(fire["event_action"], "trigger");
        assert_eq!(resolve["event_action"], "resolve");
        assert_eq!(fire["dedup_key"], resolve["dedup_key"]);
        assert_eq!(fire["routing_key"], "routingkey");
        assert_eq!(fire["payload"]["severity"], "critical");
    }

    #[test]
    fn slack_and_webhook_payloads() {
        let s = build_payload(&channel(ChannelKind::Slack, "u"), &event(EventKind::Firing));
        assert!(s["text"]
            .as_str()
            .unwrap()
            .starts_with("[CRITICAL] High Drop Rate"));
        let r = build_payload(
            &channel(ChannelKind::Slack, "u"),
            &event(EventKind::Resolved),
        );
        assert!(r["text"].as_str().unwrap().starts_with("[RESOLVED]"));
        let w = build_payload(
            &channel(ChannelKind::Webhook, "u"),
            &event(EventKind::Firing),
        );
        assert_eq!(w["event"], "firing");
        assert_eq!(w["alert"]["rule_id"], "rule-001");
    }

    #[tokio::test]
    async fn delivers_webhook_payload() {
        let (url, server) = mock_server(vec![200]).await;
        let r = deliver_to(
            &channel(ChannelKind::Webhook, &url),
            &event(EventKind::Firing),
            "",
            Duration::ZERO,
        )
        .await;
        assert!(r.delivered, "{:?}", r.error);
        assert_eq!(r.attempts, 1);
        let bodies = server.await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&bodies[0]).unwrap();
        assert_eq!(v["alert"]["rule_name"], "High Drop Rate");
    }

    #[tokio::test]
    async fn retries_server_errors_then_succeeds() {
        let (url, server) = mock_server(vec![503, 502, 200]).await;
        let r = deliver_to(
            &channel(ChannelKind::Webhook, &url),
            &event(EventKind::Firing),
            "",
            Duration::ZERO,
        )
        .await;
        assert!(r.delivered);
        assert_eq!(r.attempts, 3);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn does_not_retry_client_errors() {
        let (url, server) = mock_server(vec![404]).await;
        let r = deliver_to(
            &channel(ChannelKind::Webhook, &url),
            &event(EventKind::Firing),
            "",
            Duration::ZERO,
        )
        .await;
        assert!(!r.delivered);
        assert_eq!(r.attempts, 1);
        assert_eq!(r.error.as_deref(), Some("HTTP 404 Not Found"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn gives_up_after_max_attempts() {
        let (url, server) = mock_server(vec![500, 500, 500]).await;
        let r = deliver_to(
            &channel(ChannelKind::Webhook, &url),
            &event(EventKind::Firing),
            "",
            Duration::ZERO,
        )
        .await;
        assert!(!r.delivered);
        assert_eq!(r.attempts, MAX_ATTEMPTS);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn pagerduty_posts_to_events_url_not_target() {
        let (url, server) = mock_server(vec![202]).await;
        let r = deliver_to(
            &channel(ChannelKind::Pagerduty, "key"),
            &event(EventKind::Firing),
            &url,
            Duration::ZERO,
        )
        .await;
        assert!(r.delivered, "{:?}", r.error);
        let bodies = server.await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&bodies[0]).unwrap();
        assert_eq!(v["routing_key"], "key");
    }

    #[tokio::test]
    async fn connection_errors_do_not_leak_the_url() {
        // Nothing listens on port 1.
        let c = channel(
            ChannelKind::Slack,
            "http://127.0.0.1:1/services/SECRET-TOKEN",
        );
        let r = deliver_to(&c, &event(EventKind::Firing), "", Duration::ZERO).await;
        assert!(!r.delivered);
        assert!(!r.error.unwrap().contains("SECRET-TOKEN"));
    }
}
