// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Incident records.
//
// Incidents are opened manually or automatically from the alert lifecycle: a
// firing rule opens one incident, further firings of the same rule add to its
// timeline, and the rule clearing resolves it. Alerts suppressed by a silence
// do not open incidents. Read-modify-write operations are serialized with a
// process-wide lock so concurrent updates cannot lose timeline entries.

use crate::AppState;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

pub const INCIDENTS_PREFIX: &str = "cv:incidents:";
const ALERT_ACTOR: &str = "alert-engine";

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

pub fn is_resolved(incident: &Value) -> bool {
    s(incident, "status") == "resolved"
}

pub fn new_incident(
    title: &str,
    severity: &str,
    summary: &str,
    services: &[String],
    rule_id: Option<&str>,
    actor: &str,
    now: DateTime<Utc>,
) -> Value {
    let created = now.to_rfc3339();
    json!({
        "id": format!("inc-{}", &uuid::Uuid::new_v4().to_string()[..8]),
        "title": title,
        "severity": severity,
        "status": "open",
        "summary": summary,
        "started_at": created,
        "resolved_at": "",
        "duration": "",
        "duration_minutes": 0,
        "affected_services": services,
        "root_cause": "",
        "source": if rule_id.is_some() { "alert" } else { "manual" },
        "rule_id": rule_id,
        "timeline": [{"time": created, "event": format!("Incident opened: {summary}"), "actor": actor}],
    })
}

pub fn add_event(incident: &mut Value, now: DateTime<Utc>, event: &str, actor: &str) {
    if let Some(timeline) = incident.get_mut("timeline").and_then(|t| t.as_array_mut()) {
        timeline.push(json!({"time": now.to_rfc3339(), "event": event, "actor": actor}));
    }
}

/// Human-readable duration: `45m`, `2h 5m`, `1d 3h`.
pub fn duration_str(minutes: i64) -> String {
    let m = minutes.max(0);
    match (m / 1440, (m % 1440) / 60, m % 60) {
        (0, 0, mm) => format!("{mm}m"),
        (0, h, mm) => format!("{h}h {mm}m"),
        (d, h, _) => format!("{d}d {h}h"),
    }
}

/// open -> acknowledged. Errors with the reason when not allowed.
pub fn acknowledge(
    incident: &mut Value,
    now: DateTime<Utc>,
    actor: &str,
) -> Result<(), &'static str> {
    match s(incident, "status") {
        "open" => {
            incident["status"] = json!("acknowledged");
            add_event(incident, now, "Incident acknowledged", actor);
            Ok(())
        }
        "resolved" => Err("Incident is already resolved"),
        _ => Err("Incident is already acknowledged"),
    }
}

/// open|acknowledged -> resolved, recording duration and optional root cause.
pub fn resolve(
    incident: &mut Value,
    now: DateTime<Utc>,
    actor: &str,
    root_cause: Option<&str>,
    event: &str,
) -> Result<(), &'static str> {
    if is_resolved(incident) {
        return Err("Incident is already resolved");
    }
    let started = DateTime::parse_from_rfc3339(s(incident, "started_at"))
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or(now);
    let minutes = (now - started).num_minutes().max(0);
    incident["status"] = json!("resolved");
    incident["resolved_at"] = json!(now.to_rfc3339());
    incident["duration_minutes"] = json!(minutes);
    incident["duration"] = json!(duration_str(minutes));
    if let Some(rc) = root_cause.filter(|r| !r.trim().is_empty()) {
        incident["root_cause"] = json!(rc.trim());
    }
    add_event(incident, now, event, actor);
    Ok(())
}

pub async fn list(state: &AppState) -> Vec<Value> {
    let mut items = state
        .cache
        .list_values(INCIDENTS_PREFIX)
        .await
        .unwrap_or_default();
    // Newest first.
    items.sort_by(|a, b| s(b, "started_at").cmp(s(a, "started_at")));
    items
}

pub async fn get(state: &AppState, id: &str) -> Option<Value> {
    state
        .cache
        .get(&format!("{INCIDENTS_PREFIX}{id}"))
        .await
        .ok()
        .flatten()
}

pub async fn save(state: &AppState, incident: &Value) -> anyhow::Result<()> {
    state
        .cache
        .set_persistent(
            &format!("{INCIDENTS_PREFIX}{}", s(incident, "id")),
            incident,
        )
        .await
}

/// Read-modify-write one incident under the lock. `f` returns Err(reason) to
/// abort without saving. Returns None when the incident does not exist.
pub async fn update<F>(state: &AppState, id: &str, f: F) -> Option<Result<Value, &'static str>>
where
    F: FnOnce(&mut Value) -> Result<(), &'static str>,
{
    let _guard = LOCK.lock().await;
    let mut incident = get(state, id).await?;
    if let Err(reason) = f(&mut incident) {
        return Some(Err(reason));
    }
    if let Err(e) = save(state, &incident).await {
        tracing::warn!("Failed to save incident {id}: {e}");
        return Some(Err("Failed to save incident"));
    }
    Some(Ok(incident))
}

pub async fn create(state: &AppState, incident: Value) -> anyhow::Result<Value> {
    let _guard = LOCK.lock().await;
    save(state, &incident).await?;
    Ok(incident)
}

async fn open_incident_for_rule(state: &AppState, rule_id: &str) -> Option<Value> {
    list(state)
        .await
        .into_iter()
        .find(|i| s(i, "rule_id") == rule_id && !is_resolved(i))
}

/// An alert rule is firing: open an incident, or add to the one already open.
pub async fn open_for_alert(
    state: &AppState,
    rule_id: &str,
    rule_name: &str,
    severity: &str,
    message: &str,
) {
    let _guard = LOCK.lock().await;
    let now = Utc::now();
    let result = match open_incident_for_rule(state, rule_id).await {
        Some(mut existing) => {
            add_event(
                &mut existing,
                now,
                &format!("Alert still firing: {message}"),
                ALERT_ACTOR,
            );
            save(state, &existing).await
        }
        None => {
            save(
                state,
                &new_incident(
                    rule_name,
                    severity,
                    message,
                    &[],
                    Some(rule_id),
                    ALERT_ACTOR,
                    now,
                ),
            )
            .await
        }
    };
    if let Err(e) = result {
        tracing::warn!("Failed to record incident for rule {rule_id}: {e}");
    }
}

/// An alert rule cleared: resolve its open incident, if any.
pub async fn resolve_for_alert(state: &AppState, rule_id: &str) {
    let _guard = LOCK.lock().await;
    if let Some(mut incident) = open_incident_for_rule(state, rule_id).await {
        let _ = resolve(
            &mut incident,
            Utc::now(),
            ALERT_ACTOR,
            None,
            "Alert cleared: condition is back within threshold",
        );
        if let Err(e) = save(state, &incident).await {
            tracing::warn!("Failed to resolve incident for rule {rule_id}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(min: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap() + chrono::Duration::minutes(min)
    }

    fn incident() -> Value {
        new_incident(
            "High Drop Rate",
            "critical",
            "9% dropped",
            &["shop".to_string()],
            Some("rule-001"),
            "alert-engine",
            t(0),
        )
    }

    #[test]
    fn new_incident_shape() {
        let i = incident();
        assert_eq!(i["status"], "open");
        assert_eq!(i["source"], "alert");
        assert_eq!(i["rule_id"], "rule-001");
        assert_eq!(i["timeline"].as_array().unwrap().len(), 1);
        assert!(i["id"].as_str().unwrap().starts_with("inc-"));
        let manual = new_incident("t", "high", "s", &[], None, "admin", t(0));
        assert_eq!(manual["source"], "manual");
        assert!(manual["rule_id"].is_null());
    }

    #[test]
    fn lifecycle_open_ack_resolve() {
        let mut i = incident();
        assert_eq!(acknowledge(&mut i, t(5), "admin"), Ok(()));
        assert_eq!(i["status"], "acknowledged");
        assert_eq!(
            acknowledge(&mut i, t(6), "admin"),
            Err("Incident is already acknowledged")
        );
        assert_eq!(
            resolve(&mut i, t(125), "admin", Some("  bad policy  "), "Resolved"),
            Ok(())
        );
        assert_eq!(i["status"], "resolved");
        assert_eq!(i["duration_minutes"], 125);
        assert_eq!(i["duration"], "2h 5m");
        assert_eq!(i["root_cause"], "bad policy");
        assert_eq!(i["timeline"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn resolved_incidents_reject_further_transitions() {
        let mut i = incident();
        resolve(&mut i, t(1), "a", None, "done").unwrap();
        assert_eq!(
            resolve(&mut i, t(2), "a", None, "again"),
            Err("Incident is already resolved")
        );
        assert_eq!(
            acknowledge(&mut i, t(2), "a"),
            Err("Incident is already resolved")
        );
        assert_eq!(
            i["timeline"].as_array().unwrap().len(),
            2,
            "rejected calls add nothing"
        );
    }

    #[test]
    fn blank_root_cause_does_not_overwrite() {
        let mut i = incident();
        i["root_cause"] = json!("known");
        resolve(&mut i, t(1), "a", Some("   "), "done").unwrap();
        assert_eq!(i["root_cause"], "known");
    }

    #[test]
    fn duration_formatting() {
        assert_eq!(duration_str(0), "0m");
        assert_eq!(duration_str(59), "59m");
        assert_eq!(duration_str(60), "1h 0m");
        assert_eq!(duration_str(125), "2h 5m");
        assert_eq!(duration_str(1440), "1d 0h");
        assert_eq!(duration_str(1440 + 3 * 60 + 7), "1d 3h");
        assert_eq!(duration_str(-5), "0m");
    }
}
