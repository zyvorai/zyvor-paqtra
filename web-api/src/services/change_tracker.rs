// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Background GitOps change tracker
//
// Runs every 120 seconds, queries K8s events for recent changes to
// network-related resources, and stores them in the cache (durable across restarts
// when PAQTRA_DATA_DIR is set) under the `cv:changes:` prefix. Detects ArgoCD/Flux annotations to mark
// GitOps-managed resources.

use crate::AppState;
use std::sync::Arc;

const CHANGES_PREFIX: &str = "cv:changes:";
const CHANGES_TTL: u64 = 604800; // 7 days
pub const ROLLBACKS_PREFIX: &str = "cv:change_rollbacks:";

/// Spawn the background change tracker as a detached tokio task.
pub fn spawn_change_tracker(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(120));
        loop {
            interval.tick().await;
            if let Err(e) = track_changes(&state).await {
                tracing::debug!("Change tracking cycle failed: {}", e);
            }
        }
    });
}

/// Resource kinds we care about for change tracking.
const RELEVANT_KINDS: &[&str] = &[
    "CiliumNetworkPolicy",
    "CiliumClusterwideNetworkPolicy",
    "ConfigMap",
    "Service",
    "Deployment",
];

/// Run a single change-tracking cycle: fetch K8s events, filter to
/// relevant network resources, check for GitOps annotations, and
/// persist new changes to the in-memory cache.
async fn track_changes(state: &AppState) -> anyhow::Result<()> {
    let data = state
        .k8s
        .kubectl_json(&[
            "get",
            "events",
            "--all-namespaces",
            "--field-selector",
            "reason!=Pulled,reason!=Scheduled,reason!=Started",
            "-o",
            "json",
            "--sort-by=.lastTimestamp",
        ])
        .await;

    let items = match data.get("items").and_then(|v| v.as_array()) {
        Some(items) => items,
        None => {
            tracing::debug!("Change tracker: no events found");
            return Ok(());
        }
    };

    let mut new_count: u64 = 0;

    for item in items {
        let involved = match item.get("involvedObject") {
            Some(v) => v,
            None => continue,
        };

        let kind = involved.get("kind").and_then(|v| v.as_str()).unwrap_or("");

        // Filter: only relevant resource kinds
        if !is_relevant_resource(kind, involved) {
            continue;
        }

        let meta = item.get("metadata").unwrap_or(item);
        let uid = meta.get("uid").and_then(|v| v.as_str()).unwrap_or("");
        if uid.is_empty() {
            continue;
        }

        // Short UID for the change ID
        let uid_short = &uid[..uid.len().min(8)];
        let cache_key = format!("{}{}", CHANGES_PREFIX, uid);

        // Check if we already stored this event
        if let Ok(Some(_)) = state.cache.get::<serde_json::Value>(&cache_key).await {
            continue;
        }

        let resource_name = involved.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let namespace = meta
            .get("namespace")
            .and_then(|v| v.as_str())
            .or_else(|| involved.get("namespace").and_then(|v| v.as_str()))
            .unwrap_or("");
        let reason = item.get("reason").and_then(|v| v.as_str()).unwrap_or("");
        let message = item.get("message").and_then(|v| v.as_str()).unwrap_or("");
        let timestamp = item
            .get("lastTimestamp")
            .or_else(|| item.get("firstTimestamp"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Check for ArgoCD / Flux GitOps annotations on the involved resource
        let gitops_managed = check_gitops_annotations(state, kind, resource_name, namespace).await;

        let change = serde_json::json!({
            "id": format!("chg-{}", uid_short),
            "timestamp": timestamp,
            "type": kind,
            "resource": resource_name,
            "namespace": namespace,
            "reason": reason,
            "message": message,
            "gitops_managed": gitops_managed,
        });

        if let Err(e) = state
            .cache
            .set_durable(&cache_key, &change, CHANGES_TTL)
            .await
        {
            tracing::debug!("Failed to store change {}: {}", uid_short, e);
            continue;
        }

        new_count += 1;
    }

    if new_count > 0 {
        tracing::debug!("Change tracker: stored {} new change(s)", new_count);
    }

    Ok(())
}

/// Recent change events for investigation correlation (newest first).
pub async fn recent_changes(state: &AppState, limit: usize) -> Vec<serde_json::Value> {
    let mut vals = state
        .cache
        .list_values(CHANGES_PREFIX)
        .await
        .unwrap_or_default();
    vals.sort_by(|a, b| {
        let ta = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        let tb = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    vals.truncate(limit);
    vals
}

/// Look up a single stored change by its `chg-…` id.
pub async fn get_change(state: &AppState, id: &str) -> Option<serde_json::Value> {
    let vals = state
        .cache
        .list_values(CHANGES_PREFIX)
        .await
        .unwrap_or_default();
    vals.into_iter().find(|c| c.get("id").and_then(|v| v.as_str()) == Some(id))
}

/// Check whether the event kind is relevant. For ConfigMaps, we only care
/// about the `cilium-config` ConfigMap.
fn is_relevant_resource(kind: &str, involved: &serde_json::Value) -> bool {
    if !RELEVANT_KINDS.contains(&kind) {
        return false;
    }
    if kind == "ConfigMap" {
        let name = involved.get("name").and_then(|v| v.as_str()).unwrap_or("");
        return name == "cilium-config";
    }
    true
}

/// Query the actual K8s resource to check for ArgoCD or Flux annotations/labels.
async fn check_gitops_annotations(
    state: &AppState,
    kind: &str,
    name: &str,
    namespace: &str,
) -> bool {
    if name.is_empty() {
        return false;
    }

    // Map resource kind to kubectl resource type
    let resource_type = match kind {
        "CiliumNetworkPolicy" => "ciliumnetworkpolicy",
        "CiliumClusterwideNetworkPolicy" => "ciliumclusterwidenetworkpolicy",
        "ConfigMap" => "configmap",
        "Service" => "service",
        "Deployment" => "deployment",
        _ => return false,
    };

    let mut args = vec!["get", resource_type, name, "-o", "json"];
    if !namespace.is_empty() {
        args.push("-n");
        args.push(namespace);
    }

    let resource = state.k8s.kubectl_json(&args).await;

    let meta = match resource.get("metadata") {
        Some(m) => m,
        None => return false,
    };

    // Check ArgoCD annotations
    if let Some(annotations) = meta.get("annotations").and_then(|v| v.as_object()) {
        if annotations
            .keys()
            .any(|k| k.starts_with("argocd.argoproj.io/"))
        {
            return true;
        }
    }

    // Check Flux labels
    if let Some(labels) = meta.get("labels").and_then(|v| v.as_object()) {
        if labels.keys().any(|k| k.starts_with("toolkit.fluxcd.io/")) {
            return true;
        }
    }

    false
}

/// Why a change cannot be rolled back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollbackBlock {
    /// Only Deployments can be reverted: Kubernetes keeps their previous
    /// revision. For other kinds, events carry no earlier manifest to restore.
    UnsupportedKind,
    /// A GitOps controller owns the resource and would undo the rollback.
    GitopsManaged,
    /// Already rolled back.
    AlreadyRolledBack,
    /// `rollout undo` reverts the latest revision, so a newer change to the
    /// same resource has to be rolled back first.
    NewerChange,
}

impl RollbackBlock {
    pub fn message(self) -> &'static str {
        match self {
            RollbackBlock::UnsupportedKind => {
                "Only Deployment changes can be rolled back: other resource kinds have no previous version stored"
            }
            RollbackBlock::GitopsManaged => {
                "This resource is managed by GitOps: revert the change in Git, or the controller will undo the rollback"
            }
            RollbackBlock::AlreadyRolledBack => "This change was already rolled back",
            RollbackBlock::NewerChange => {
                "A newer change exists for this resource: roll that one back first"
            }
        }
    }
}

fn str_field<'a>(change: &'a serde_json::Value, key: &str) -> &'a str {
    change.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// Instant of a change, for ordering. Falls back to zero for unparseable text.
fn change_time(change: &serde_json::Value) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(str_field(change, "timestamp"))
        .map(|t| t.with_timezone(&chrono::Utc))
        .unwrap_or(chrono::DateTime::<chrono::Utc>::MIN_UTC)
}

fn resource_key(change: &serde_json::Value) -> (String, String, String) {
    (
        str_field(change, "type").to_string(),
        str_field(change, "namespace").to_string(),
        str_field(change, "resource").to_string(),
    )
}

/// Decides rollback eligibility for changes, given the full change list and
/// the ids already rolled back.
pub struct RollbackIndex {
    newest: std::collections::HashMap<(String, String, String), chrono::DateTime<chrono::Utc>>,
    rolled_back: std::collections::HashSet<String>,
}

impl RollbackIndex {
    pub fn new(
        changes: &[serde_json::Value],
        rolled_back: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut newest = std::collections::HashMap::new();
        for c in changes {
            let t = change_time(c);
            let e = newest.entry(resource_key(c)).or_insert(t);
            if t > *e {
                *e = t;
            }
        }
        Self {
            newest,
            rolled_back: rolled_back.into_iter().collect(),
        }
    }

    pub fn check(&self, change: &serde_json::Value) -> Result<(), RollbackBlock> {
        if str_field(change, "type") != "Deployment" {
            return Err(RollbackBlock::UnsupportedKind);
        }
        if change
            .get("gitops_managed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return Err(RollbackBlock::GitopsManaged);
        }
        if self.rolled_back.contains(str_field(change, "id")) {
            return Err(RollbackBlock::AlreadyRolledBack);
        }
        match self.newest.get(&resource_key(change)) {
            Some(newest) if change_time(change) < *newest => Err(RollbackBlock::NewerChange),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod rollback_tests {
    use super::*;
    use serde_json::json;

    fn change(id: &str, kind: &str, name: &str, ts: &str, gitops: bool) -> serde_json::Value {
        json!({"id": id, "type": kind, "resource": name, "namespace": "ns", "timestamp": ts, "gitops_managed": gitops})
    }

    #[test]
    fn newest_deployment_change_is_eligible() {
        let c = change("a", "Deployment", "web", "2026-01-01T00:00:00Z", false);
        let idx = RollbackIndex::new(std::slice::from_ref(&c), []);
        assert_eq!(idx.check(&c), Ok(()));
    }

    #[test]
    fn only_deployments_can_be_rolled_back() {
        for kind in ["CiliumNetworkPolicy", "ConfigMap", "Service"] {
            let c = change("a", kind, "x", "2026-01-01T00:00:00Z", false);
            let idx = RollbackIndex::new(std::slice::from_ref(&c), []);
            assert_eq!(idx.check(&c), Err(RollbackBlock::UnsupportedKind), "{kind}");
        }
    }

    #[test]
    fn gitops_managed_is_blocked() {
        let c = change("a", "Deployment", "web", "2026-01-01T00:00:00Z", true);
        let idx = RollbackIndex::new(std::slice::from_ref(&c), []);
        assert_eq!(idx.check(&c), Err(RollbackBlock::GitopsManaged));
    }

    #[test]
    fn older_change_is_blocked_by_newer_one_on_same_resource_only() {
        let old = change("old", "Deployment", "web", "2026-01-01T00:00:00Z", false);
        let new = change("new", "Deployment", "web", "2026-01-02T00:00:00Z", false);
        let other = change("other", "Deployment", "api", "2026-01-01T00:00:00Z", false);
        let all = vec![old.clone(), new.clone(), other.clone()];
        let idx = RollbackIndex::new(&all, []);
        assert_eq!(idx.check(&old), Err(RollbackBlock::NewerChange));
        assert_eq!(idx.check(&new), Ok(()));
        assert_eq!(
            idx.check(&other),
            Ok(()),
            "different resource is unaffected"
        );
    }

    #[test]
    fn already_rolled_back_is_blocked() {
        let c = change("a", "Deployment", "web", "2026-01-01T00:00:00Z", false);
        let idx = RollbackIndex::new(std::slice::from_ref(&c), ["a".to_string()]);
        assert_eq!(idx.check(&c), Err(RollbackBlock::AlreadyRolledBack));
    }

    #[test]
    fn timestamps_compare_as_instants_not_text() {
        // Same instant written with different offsets must not block each other.
        let a = change("a", "Deployment", "web", "2026-01-01T01:00:00+01:00", false);
        let b = change("b", "Deployment", "web", "2026-01-01T00:00:00Z", false);
        let idx = RollbackIndex::new(&[a.clone(), b.clone()], []);
        assert_eq!(idx.check(&a), Ok(()));
        assert_eq!(idx.check(&b), Ok(()));
    }
}
