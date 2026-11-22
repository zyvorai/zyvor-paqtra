// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Background alerting engine
//
// Evaluates alert rules stored in the cache (default every 60 seconds) and fires alerts
// when conditions are met. Alert history is persisted back to the cache, and
// firing/resolved events are delivered through the notifier (see notifier.rs).

use crate::services::{incidents, notifier};
use crate::AppState;
use std::sync::Arc;

const ALERT_RULES_PREFIX: &str = "cv:alert_rules:";
const ALERT_HISTORY_PREFIX: &str = "cv:alert_history:";
const ALERT_STATE_PREFIX: &str = "cv:alert_state:";

/// Per-rule firing state, used to notify once per incident instead of on every
/// evaluation cycle, and to send a "resolved" message when the rule clears.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct RuleState {
    firing: bool,
    last_notified_epoch: i64,
}

/// Start the background alerting engine.
/// Evaluates alert rules from the cache every `alert_eval_interval_secs`.
pub fn spawn_alerting_engine(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            state.config.alert_eval_interval_secs,
        ));
        loop {
            interval.tick().await;
            if let Err(e) = evaluate_rules(&state).await {
                tracing::warn!("Alert evaluation failed: {}", e);
            }
        }
    });
}

/// Load all alert rules from the cache, evaluate each enabled rule against
/// live Hubble flows, and fire alerts when thresholds are breached.
async fn evaluate_rules(state: &AppState) -> anyhow::Result<()> {
    let rules = state.cache.list_values(ALERT_RULES_PREFIX).await?;
    if rules.is_empty() {
        tracing::debug!("No alert rules found, skipping evaluation cycle");
        return Ok(());
    }

    // Fetch flows once for all rules that need them
    let flows = state.hubble.get_flows(500, None).await.unwrap_or_default();

    for rule in &rules {
        let enabled = rule
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !enabled {
            continue;
        }

        let rule_id = rule.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
        let rule_name = rule
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unnamed");
        let severity = rule
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("warning");
        let condition = rule.get("condition").and_then(|v| v.as_str()).unwrap_or("");

        let result = evaluate_condition(condition, &flows, state).await;

        match result {
            ConditionResult::Fired(message) => {
                tracing::info!(
                    rule_id = rule_id,
                    rule_name = rule_name,
                    "Alert fired: {}",
                    message
                );
                handle_fired(state, rule_id, rule_name, severity, &message).await;
            }
            ConditionResult::Ok => {
                handle_ok(state, rule_id, rule_name, severity).await;
            }
            ConditionResult::Skipped(reason) => {
                tracing::debug!(rule_id = rule_id, "Rule skipped: {}", reason);
            }
        }
    }

    Ok(())
}

enum ConditionResult {
    /// Condition threshold was breached; contains a human-readable message.
    Fired(String),
    /// Condition evaluated normally and is within threshold.
    Ok,
    /// Condition could not be evaluated (unsupported, missing data, etc.).
    Skipped(String),
}

/// Parse and evaluate a single rule condition string against the flow data.
async fn evaluate_condition(
    condition: &str,
    flows: &[crate::models::flow::Flow],
    state: &AppState,
) -> ConditionResult {
    let cond = condition.trim();

    // ── drop_rate > X% for Ym ───────────────────────────────
    if cond.starts_with("drop_rate >") {
        if let Some(threshold) = parse_percent_threshold(cond) {
            let total = flows.len();
            if total == 0 {
                return ConditionResult::Ok;
            }
            let dropped = flows.iter().filter(|f| f.verdict == "DROPPED").count();
            let rate = (dropped as f64 / total as f64) * 100.0;
            if rate > threshold {
                return ConditionResult::Fired(format!(
                    "Drop rate {:.1}% exceeds threshold {:.1}% ({} dropped out of {} flows)",
                    rate, threshold, dropped, total
                ));
            }
            return ConditionResult::Ok;
        }
        return ConditionResult::Skipped(format!("Could not parse drop_rate condition: {}", cond));
    }

    // ── dns_servfail > N/min ────────────────────────────────
    if cond.starts_with("dns_servfail >") {
        if let Some(threshold) = parse_rate_threshold(cond) {
            // Count DNS (port 53) flows that were DROPPED as a proxy for SERVFAIL
            let dns_drops = flows
                .iter()
                .filter(|f| f.port == 53 && f.verdict == "DROPPED")
                .count();
            if dns_drops as f64 > threshold {
                return ConditionResult::Fired(format!(
                    "DNS failures {} exceed threshold {}/min",
                    dns_drops, threshold as u64
                ));
            }
            return ConditionResult::Ok;
        }
        return ConditionResult::Skipped(format!(
            "Could not parse dns_servfail condition: {}",
            cond
        ));
    }

    // ── policy_denied > N/min ───────────────────────────────
    if cond.starts_with("policy_denied >") {
        if let Some(threshold) = parse_rate_threshold(cond) {
            let denied = flows.iter().filter(|f| f.verdict == "DROPPED").count();
            if denied as f64 > threshold {
                return ConditionResult::Fired(format!(
                    "Policy denies {} exceed threshold {}/min",
                    denied, threshold as u64
                ));
            }
            return ConditionResult::Ok;
        }
        return ConditionResult::Skipped(format!(
            "Could not parse policy_denied condition: {}",
            cond
        ));
    }

    // ── endpoint_status != ready ────────────────────────────
    if cond.starts_with("endpoint_status") && cond.contains("!= ready") {
        let data = state
            .k8s
            .kubectl_json(&["get", "ciliumendpoints", "--all-namespaces", "-o", "json"])
            .await;

        let not_ready: Vec<String> = data
            .get("items")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let ep_state = item
                            .get("status")
                            .and_then(|s| s.get("state"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("ready");
                        if ep_state != "ready" {
                            let name = item
                                .get("metadata")
                                .and_then(|m| m.get("name"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            Some(format!("{} ({})", name, ep_state))
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        if !not_ready.is_empty() {
            let summary = if not_ready.len() <= 5 {
                not_ready.join(", ")
            } else {
                format!(
                    "{} and {} more",
                    not_ready[..5].join(", "),
                    not_ready.len() - 5
                )
            };
            return ConditionResult::Fired(format!(
                "{} endpoint(s) not ready: {}",
                not_ready.len(),
                summary
            ));
        }
        return ConditionResult::Ok;
    }

    // ── ct_entries > X% max ─────────────────────────────────
    if cond.starts_with("ct_entries >") {
        return ConditionResult::Skipped(
            "ct_entries evaluation requires eBPF reader (not available in web-api)".to_string(),
        );
    }

    // ── Unrecognized condition ──────────────────────────────
    ConditionResult::Skipped(format!("Unrecognized alert condition: {}", cond))
}

/// Check that a condition is one the engine can evaluate, so a saved rule
/// never sits enabled-but-skipped. Mirrors the forms in [`evaluate_condition`].
pub fn validate_condition(condition: &str) -> Result<(), String> {
    let cond = condition.trim();
    if cond.is_empty() {
        return Err("condition must not be empty".into());
    }
    if cond.starts_with("drop_rate >") {
        return parse_percent_threshold(cond)
            .map(|_| ())
            .ok_or_else(|| "drop_rate needs a percentage, e.g. `drop_rate > 5% for 5m`".into());
    }
    if cond.starts_with("dns_servfail >") || cond.starts_with("policy_denied >") {
        return parse_rate_threshold(cond)
            .map(|_| ())
            .ok_or_else(|| "expected a rate, e.g. `dns_servfail > 10/min`".into());
    }
    if cond.starts_with("endpoint_status") && cond.contains("!= ready") {
        return Ok(());
    }
    if cond.starts_with("ct_entries >") {
        return Err("ct_entries cannot be evaluated by the web API (needs the eBPF reader)".into());
    }
    Err(
        "unsupported condition; use drop_rate > N%, dns_servfail > N/min, policy_denied > N/min or endpoint_status != ready"
            .into(),
    )
}

/// Parse a threshold percentage from conditions like "drop_rate > 5% for 5m".
/// Returns the numeric threshold (e.g. 5.0).
fn parse_percent_threshold(condition: &str) -> Option<f64> {
    // Look for a pattern like "> 5%" or ">5%"
    let after_gt = condition.split('>').nth(1)?;
    let pct_pos = after_gt.find('%')?;
    let num_str = after_gt[..pct_pos].trim();
    num_str.parse::<f64>().ok()
}

/// Parse a rate threshold from conditions like "dns_servfail > 10/min".
/// Returns the numeric threshold (e.g. 10.0).
fn parse_rate_threshold(condition: &str) -> Option<f64> {
    let after_gt = condition.split('>').nth(1)?;
    let num_str = after_gt.split('/').next()?.trim();
    num_str.parse::<f64>().ok()
}

fn now_epoch() -> i64 {
    chrono::Utc::now().timestamp()
}

async fn load_state(state: &AppState, rule_id: &str) -> RuleState {
    state
        .cache
        .get(&format!("{}{}", ALERT_STATE_PREFIX, rule_id))
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

async fn save_state(state: &AppState, rule_id: &str, st: &RuleState) {
    if let Err(e) = state
        .cache
        .set_persistent(&format!("{}{}", ALERT_STATE_PREFIX, rule_id), st)
        .await
    {
        tracing::warn!("Failed to save alert state for {}: {}", rule_id, e);
    }
}

/// A rule's condition is breached. Records history and notifies once per
/// incident; while the rule keeps firing, repeats only after the cooldown.
async fn handle_fired(
    state: &AppState,
    rule_id: &str,
    rule_name: &str,
    severity: &str,
    message: &str,
) {
    let mut st = load_state(state, rule_id).await;
    let now = now_epoch();
    let cooldown = state.config.alert_cooldown_secs as i64;
    if st.firing && now - st.last_notified_epoch < cooldown {
        return;
    }

    let silenced = notifier::is_silenced(state, rule_id).await;
    fire_alert(state, rule_id, rule_name, severity, message, silenced).await;
    update_rule_trigger(state, rule_id).await;

    if !silenced {
        // Silenced alerts are deliberately ignored: no incident, no notification.
        incidents::open_for_alert(state, rule_id, rule_name, severity, message).await;
        notifier::notify(
            state,
            notifier::AlertEvent {
                kind: notifier::EventKind::Firing,
                rule_id: rule_id.to_string(),
                rule_name: rule_name.to_string(),
                severity: severity.to_string(),
                message: message.to_string(),
                at: chrono::Utc::now().to_rfc3339(),
            },
        )
        .await;
    }

    st.firing = true;
    st.last_notified_epoch = now;
    save_state(state, rule_id, &st).await;
}

/// A rule evaluated within threshold. If it was firing, the incident is over:
/// send a resolved notification and clear the state.
async fn handle_ok(state: &AppState, rule_id: &str, rule_name: &str, severity: &str) {
    let st = load_state(state, rule_id).await;
    if !st.firing {
        return;
    }
    save_state(state, rule_id, &RuleState::default()).await;
    incidents::resolve_for_alert(state, rule_id).await;

    if !notifier::is_silenced(state, rule_id).await {
        notifier::notify(
            state,
            notifier::AlertEvent {
                kind: notifier::EventKind::Resolved,
                rule_id: rule_id.to_string(),
                rule_name: rule_name.to_string(),
                severity: severity.to_string(),
                message: "Condition is back within threshold".to_string(),
                at: chrono::Utc::now().to_rfc3339(),
            },
        )
        .await;
    }
}

/// Write a fired alert to cache alert history.
async fn fire_alert(
    state: &AppState,
    rule_id: &str,
    rule_name: &str,
    severity: &str,
    message: &str,
    silenced: bool,
) {
    let alert_id = format!("alert-{}", uuid::Uuid::new_v4());
    let now = chrono::Utc::now().to_rfc3339();

    let alert = serde_json::json!({
        "id": alert_id,
        "rule_id": rule_id,
        "rule_name": rule_name,
        "severity": severity,
        "message": message,
        "fired_at": now,
        "status": "firing",
        "silenced": silenced,
    });

    let key = format!("{}{}", ALERT_HISTORY_PREFIX, alert_id);
    if let Err(e) = state.cache.set_persistent(&key, &alert).await {
        tracing::warn!("Failed to persist alert {}: {}", alert_id, e);
    }
}

/// Update the rule's last_triggered timestamp and increment trigger_count.
async fn update_rule_trigger(state: &AppState, rule_id: &str) {
    let key = format!("{}{}", ALERT_RULES_PREFIX, rule_id);
    if let Ok(Some(mut rule)) = state.cache.get::<serde_json::Value>(&key).await {
        rule["last_triggered"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
        let count = rule
            .get("trigger_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        rule["trigger_count"] = serde_json::json!(count + 1);
        if let Err(e) = state.cache.set_persistent(&key, &rule).await {
            tracing::warn!("Failed to update rule {}: {}", rule_id, e);
        }
    }
}

#[cfg(test)]
mod validate_condition_tests {
    use super::validate_condition;

    #[test]
    fn accepts_evaluable_forms() {
        for c in [
            "drop_rate > 5% for 5m",
            "drop_rate >2.5%",
            "dns_servfail > 10/min",
            "policy_denied > 100/min",
            "endpoint_status != ready",
        ] {
            assert!(validate_condition(c).is_ok(), "{c}");
        }
    }

    #[test]
    fn rejects_unparseable_or_unsupported() {
        for c in [
            "",
            "   ",
            "drop_rate > lots",
            "drop_rate > 5",
            "dns_servfail > x/min",
            "ct_entries > 90% max",
            "cpu > 90%",
        ] {
            assert!(validate_condition(c).is_err(), "{c}");
        }
    }
}
