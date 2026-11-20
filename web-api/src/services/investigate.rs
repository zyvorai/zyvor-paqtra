// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Path investigation and evidence-backed policy preview.
//!
//! Every claim carries confidence: observed | inferred | unavailable.
//! Unsupported policy constructs are reported as unknown — never a confident pass.

use crate::services::change_tracker;
use crate::services::flow_store::{FlowQuery, StoredFlow};
use crate::AppState;
use chrono::{Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Observed,
    Inferred,
    Unavailable,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkloadRef {
    pub namespace: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub pod: String,
    #[serde(default)]
    pub labels: Option<Value>,
}

impl WorkloadRef {
    pub fn pod_hint(&self) -> &str {
        if !self.pod.is_empty() {
            &self.pod
        } else {
            &self.name
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct InvestigatePathRequest {
    pub source: WorkloadRef,
    pub destination: WorkloadRef,
    pub port: u16,
    #[serde(default = "default_proto")]
    pub protocol: String,
    /// Minutes to look back (default 60).
    #[serde(default = "default_window")]
    pub time_window_minutes: i64,
}

fn default_proto() -> String {
    "TCP".into()
}
fn default_window() -> i64 {
    60
}

#[derive(Debug, Clone, Serialize)]
pub struct EvidenceRef {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InvestigateStep {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub confidence: Confidence,
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InvestigateResult {
    pub id: String,
    pub request: Value,
    pub steps: Vec<InvestigateStep>,
    pub likely_owner: String,
    pub next_actions: Vec<String>,
    pub flow_ingest: Value,
    pub created_at: String,
}

pub async fn investigate_path(state: &AppState, req: &InvestigatePathRequest) -> InvestigateResult {
    let id = format!("inv-{}", Uuid::new_v4());
    let since = (Utc::now() - ChronoDuration::minutes(req.time_window_minutes.max(1))).to_rfc3339();
    let mut steps = Vec::new();
    let stats = state.flow_store.stats();

    // 1. Resolve workloads
    let src_hint = req.source.pod_hint().to_string();
    let dst_hint = req.destination.pod_hint().to_string();
    let k8s_ok = state.k8s.is_healthy().await;

    let (src_pods, dst_eps) = if k8s_ok {
        let src = state
            .k8s
            .kubectl_json(&["get", "pods", "-n", &req.source.namespace, "-o", "json"])
            .await;
        let svc = state
            .k8s
            .kubectl_json(&["get", "svc", "-n", &req.destination.namespace, "-o", "json"])
            .await;
        let eps = state
            .k8s
            .kubectl_json(&[
                "get",
                "endpoints",
                "-n",
                &req.destination.namespace,
                "-o",
                "json",
            ])
            .await;
        (src, (svc, eps))
    } else {
        (json!({}), (json!({}), json!({})))
    };

    let matching_src = count_name_matches(&src_pods, &src_hint);
    let (svc_json, eps_json) = dst_eps;
    let matching_svc = count_name_matches(&svc_json, &dst_hint);
    let backend_count = endpoint_address_count(&eps_json, &dst_hint);

    steps.push(InvestigateStep {
        id: "resolve".into(),
        title: "Resolve workloads".into(),
        detail: if k8s_ok {
            format!(
                "Source '{}': {} pod match(es). Destination '{}': {} Service match(es), {} endpoint address(es).",
                src_hint, matching_src, dst_hint, matching_svc, backend_count
            )
        } else {
            "Kubernetes API unavailable — workload resolution skipped.".into()
        },
        confidence: if k8s_ok {
            Confidence::Observed
        } else {
            Confidence::Unavailable
        },
        evidence: if k8s_ok {
            vec![EvidenceRef {
                kind: "k8s".into(),
                id: format!("{}/{}", req.source.namespace, src_hint),
            }]
        } else {
            vec![]
        },
    });

    // 2. Identities (best-effort from flows / unavailable)
    let path_flows = state
        .flow_store
        .query(&FlowQuery {
            src_namespace: Some(req.source.namespace.clone()),
            src_pod: Some(src_hint.clone()),
            dst_namespace: Some(req.destination.namespace.clone()),
            dst_pod: Some(dst_hint.clone()),
            port: Some(req.port),
            since_rfc3339: Some(since.clone()),
            limit: 100,
            ..Default::default()
        })
        .unwrap_or_default();

    let any_identity = path_flows
        .iter()
        .any(|f| f.src_identity > 0 || f.dst_identity > 0);
    steps.push(InvestigateStep {
        id: "identity".into(),
        title: "Cilium identities".into(),
        detail: if any_identity {
            format!(
                "Saw identity fields on {} stored flow(s) for this path.",
                path_flows.len()
            )
        } else if !path_flows.is_empty() {
            format!(
                "{} flow(s) indexed for this path; identity IDs not present in ingested metadata.",
                path_flows.len()
            )
        } else {
            "No indexed flows with identities for this path in the window.".into()
        },
        confidence: if any_identity {
            Confidence::Observed
        } else if !path_flows.is_empty() {
            Confidence::Inferred
        } else {
            Confidence::Unavailable
        },
        evidence: path_flows
            .iter()
            .take(5)
            .map(|f| EvidenceRef {
                kind: "flow".into(),
                id: f.id.clone(),
            })
            .collect(),
    });

    // 3. Policy / verdict from flows
    let dropped: Vec<_> = path_flows
        .iter()
        .filter(|f| f.verdict.eq_ignore_ascii_case("DROPPED"))
        .cloned()
        .collect();
    let forwarded: Vec<_> = path_flows
        .iter()
        .filter(|f| f.verdict.eq_ignore_ascii_case("FORWARDED"))
        .cloned()
        .collect();

    let policies = if k8s_ok {
        state.k8s.list_policies().await.unwrap_or_default()
    } else {
        Vec::new()
    };
    let ns_policies: Vec<_> = policies
        .iter()
        .filter(|p| p.namespace == req.source.namespace || p.namespace == req.destination.namespace)
        .collect();

    let denying_guess = ns_policies
        .iter()
        .find(|p| {
            let n = p.name.to_lowercase();
            n.contains("deny") || n.contains("default-deny")
        })
        .map(|p| p.name.clone());

    steps.push(InvestigateStep {
        id: "policy".into(),
        title: "Policy verdict".into(),
        detail: if !dropped.is_empty() {
            format!(
                "{} dropped / {} forwarded flow(s) in window. {} CNP(s) in related namespaces.{}",
                dropped.len(),
                forwarded.len(),
                ns_policies.len(),
                denying_guess
                    .as_ref()
                    .map(|n| format!(" Candidate deny policy name: {n}."))
                    .unwrap_or_default()
            )
        } else if !forwarded.is_empty() {
            format!(
                "{} forwarded flow(s); no drops for this path in the window.",
                forwarded.len()
            )
        } else {
            format!(
                "No path flows in store. {} CNP(s) visible in related namespaces.",
                ns_policies.len()
            )
        },
        confidence: if !path_flows.is_empty() {
            Confidence::Observed
        } else if k8s_ok {
            Confidence::Inferred
        } else {
            Confidence::Unavailable
        },
        evidence: {
            let mut e: Vec<_> = dropped
                .iter()
                .chain(forwarded.iter())
                .take(8)
                .map(|f| EvidenceRef {
                    kind: "flow".into(),
                    id: f.id.clone(),
                })
                .collect();
            if let Some(n) = &denying_guess {
                e.push(EvidenceRef {
                    kind: "policy".into(),
                    id: n.clone(),
                });
            }
            e
        },
    });

    // 4. DNS
    let dns_flows = state
        .flow_store
        .query(&FlowQuery {
            src_namespace: Some(req.source.namespace.clone()),
            port: Some(53),
            since_rfc3339: Some(since.clone()),
            limit: 50,
            ..Default::default()
        })
        .unwrap_or_default();
    let dns_drops = dns_flows
        .iter()
        .filter(|f| f.verdict.eq_ignore_ascii_case("DROPPED"))
        .count();
    steps.push(InvestigateStep {
        id: "dns".into(),
        title: "DNS signals".into(),
        detail: if req.port == 53 || dns_drops > 0 {
            format!(
                "{} DNS (port 53) flow(s) from source namespace; {} dropped.",
                dns_flows.len(),
                dns_drops
            )
        } else if dns_flows.is_empty() {
            "No DNS flows indexed for source namespace in window.".into()
        } else {
            format!(
                "{} DNS flow(s) observed from source namespace (port 53).",
                dns_flows.len()
            )
        },
        confidence: if !dns_flows.is_empty() {
            Confidence::Observed
        } else {
            Confidence::Unavailable
        },
        evidence: dns_flows
            .iter()
            .take(3)
            .map(|f| EvidenceRef {
                kind: "flow".into(),
                id: f.id.clone(),
            })
            .collect(),
    });

    // 5. Node path
    let cross_node = path_flows.iter().any(|f| {
        !f.src_ip.is_empty()
            && !f.dst_ip.is_empty()
            && f.src_ip.split('.').take(3).collect::<Vec<_>>()
                != f.dst_ip.split('.').take(3).collect::<Vec<_>>()
    });
    steps.push(InvestigateStep {
        id: "path".into(),
        title: "Node path".into(),
        detail: if path_flows.is_empty() {
            "Insufficient flow evidence to infer same-node vs cross-node.".into()
        } else if cross_node {
            "IP prefixes differ across observed flows — likely cross-node (inferred).".into()
        } else {
            "Observed flows share address prefixes — possibly same-node (inferred).".into()
        },
        confidence: if path_flows.is_empty() {
            Confidence::Unavailable
        } else {
            Confidence::Inferred
        },
        evidence: path_flows
            .iter()
            .take(2)
            .map(|f| EvidenceRef {
                kind: "flow".into(),
                id: f.id.clone(),
            })
            .collect(),
    });

    // 6. Correlated changes (thin timeline join)
    let changes = change_tracker::recent_changes(state, 20).await;
    steps.push(InvestigateStep {
        id: "changes".into(),
        title: "Correlated changes".into(),
        detail: if changes.is_empty() {
            "No recent network-related change events in tracker cache.".into()
        } else {
            format!(
                "{} recent change event(s); top: {}",
                changes.len(),
                changes
                    .iter()
                    .take(3)
                    .filter_map(|c| c.get("resource").and_then(|n| n.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
        confidence: if changes.is_empty() {
            Confidence::Unavailable
        } else {
            Confidence::Observed
        },
        evidence: changes
            .iter()
            .take(5)
            .filter_map(|c| {
                Some(EvidenceRef {
                    kind: "change".into(),
                    id: c.get("id")?.as_str()?.to_string(),
                })
            })
            .collect(),
    });

    // Owner
    let likely_owner = if dns_drops > 0 && (req.port == 53 || dropped.is_empty()) {
        "dns"
    } else if k8s_ok && matching_svc > 0 && backend_count == 0 {
        "no_backend"
    } else if !dropped.is_empty() {
        "policy"
    } else {
        // Forwarded flows, no k8s data, or nothing observed: no root cause to name.
        "unknown"
    };

    let mut next_actions = Vec::new();
    match likely_owner {
        "policy" => {
            next_actions.push(
                "Preview a narrowly scoped CiliumNetworkPolicy allow for this port (POST /api/v1/policies/simulate)."
                    .into(),
            );
            if let Some(n) = denying_guess {
                next_actions.push(format!(
                    "Review CNP '{n}' selectors against source/destination identities."
                ));
            }
        }
        "dns" => {
            next_actions
                .push("Allow DNS egress (UDP/TCP 53) to kube-dns / node-local-dns via CNP.".into());
        }
        "no_backend" => {
            next_actions.push(
                "Check Deployment/EndpointSlice readiness for the destination Service.".into(),
            );
        }
        _ => {
            next_actions.push(
                "Widen the time window or confirm Hubble ingest is healthy, then re-run.".into(),
            );
        }
    }
    next_actions.push(
        "Enforcement stays in Cilium CRDs — Paqtra will not attach or rewrite BPF programs.".into(),
    );

    // Persist bundle snapshot in cache
    let result = InvestigateResult {
        id: id.clone(),
        request: json!({
            "source": { "namespace": req.source.namespace, "name": src_hint },
            "destination": { "namespace": req.destination.namespace, "name": dst_hint },
            "port": req.port,
            "protocol": req.protocol,
            "time_window_minutes": req.time_window_minutes,
        }),
        steps: steps.clone(),
        likely_owner: likely_owner.into(),
        next_actions: next_actions.clone(),
        flow_ingest: json!({
            "total": stats.total,
            "last_ok": stats.last_ingest_ok,
            "last_at": stats.last_ingest_at,
            "source": stats.ingest_source,
            "confidence": if stats.last_ingest_ok { "observed" } else { "unavailable" },
        }),
        created_at: Utc::now().to_rfc3339(),
    };

    let bundle = build_bundle(
        &result,
        &path_flows,
        &dropped,
        &ns_policies,
        &changes,
        &stats,
        req.time_window_minutes,
    );
    let _ = state
        .cache
        .set_persistent(&format!("cv:investigate_bundle:{}", id), &bundle)
        .await;

    result
}


/// Explain a single stored (or snapshot) flow — especially DROPPED connections.
#[derive(Debug, Clone, Deserialize)]
pub struct InvestigateFlowRequest {
    #[serde(default)]
    pub flow_id: String,
    #[serde(default)]
    pub flow: Option<Value>,
    #[serde(default = "default_window")]
    pub time_window_minutes: i64,
}

pub async fn investigate_flow(state: &AppState, req: &InvestigateFlowRequest) -> InvestigateResult {
    let id = format!("inv-flow-{}", Uuid::new_v4());
    let stats = state.flow_store.stats();
    let mut steps = Vec::new();

    let stored = if !req.flow_id.is_empty() {
        state
            .flow_store
            .query(&FlowQuery {
                limit: 200,
                ..Default::default()
            })
            .ok()
            .and_then(|rows| rows.into_iter().find(|f| f.id == req.flow_id))
    } else {
        None
    };

    let (src_ns, src_pod, dst_ns, dst_pod, port, protocol, verdict, drop_reason, flow_ts) =
        if let Some(f) = &stored {
            (
                f.src_namespace.clone(),
                f.src_pod.clone(),
                f.dst_namespace.clone(),
                f.dst_pod.clone(),
                f.port,
                f.protocol.clone(),
                f.verdict.clone(),
                f.drop_reason.clone(),
                f.ts.clone(),
            )
        } else if let Some(v) = &req.flow {
            let src = v.get("source").cloned().unwrap_or(json!({}));
            let dst = v.get("destination").cloned().unwrap_or(json!({}));
            (
                src.get("namespace").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                src.get("pod").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                dst.get("namespace").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                dst.get("pod").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                v.get("port").and_then(|x| x.as_u64()).unwrap_or(0) as u16,
                v.get("protocol").and_then(|x| x.as_str()).unwrap_or("TCP").to_string(),
                v.get("verdict").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                v.get("drop_reason").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                v.get("timestamp").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            )
        } else {
            return InvestigateResult {
                id: id.clone(),
                request: json!({"flow_id": req.flow_id}),
                steps: vec![InvestigateStep {
                    id: "resolve".into(),
                    title: "Resolve flow".into(),
                    detail: "No flow_id match in the store and no flow snapshot provided.".into(),
                    confidence: Confidence::Unavailable,
                    evidence: vec![],
                }],
                likely_owner: "unknown".into(),
                next_actions: vec!["Select a DROPPED flow from Flows or pass a flow snapshot.".into()],
                flow_ingest: json!({
                    "total": stats.total,
                    "last_ok": stats.last_ingest_ok,
                    "source": stats.ingest_source,
                    "gaps": stats.gap_count,
                    "confidence": if stats.last_ingest_ok { "observed" } else { "unavailable" },
                }),
                created_at: Utc::now().to_rfc3339(),
            };
        };

    steps.push(InvestigateStep {
        id: "flow".into(),
        title: "Selected flow".into(),
        detail: format!(
            "{src_ns}/{src_pod} → {dst_ns}/{dst_pod} {protocol}/{port} verdict={verdict} drop_reason={}",
            if drop_reason.is_empty() { "(none)" } else { drop_reason.as_str() }
        ),
        confidence: Confidence::Observed,
        evidence: vec![EvidenceRef {
            kind: "flow".into(),
            id: if req.flow_id.is_empty() {
                format!("{src_pod}-{dst_pod}-{port}")
            } else {
                req.flow_id.clone()
            },
        }],
    });

    let k8s_ok = state.k8s.is_healthy().await;
    let identity_detail = if k8s_ok {
        let ids = state.k8s.kubectl_json(&["get", "ciliumendpoints", "-A", "-o", "json"]).await;
        let count = ids.get("items").and_then(|i| i.as_array()).map(|a| a.len()).unwrap_or(0);
        format!("Cluster has {count} CiliumEndpoint(s). Source {src_ns}/{src_pod}, dest {dst_ns}/{dst_pod}.")
    } else {
        "Kubernetes unavailable — cannot resolve CiliumEndpoint identities.".into()
    };
    steps.push(InvestigateStep {
        id: "identity".into(),
        title: "Endpoint identities".into(),
        detail: identity_detail,
        confidence: if k8s_ok { Confidence::Inferred } else { Confidence::Unavailable },
        evidence: vec![],
    });

    let mut matching_policies: Vec<String> = Vec::new();
    let mut denying_guess: Option<String> = None;
    if k8s_ok {
        for ns in [&src_ns, &dst_ns] {
            if ns.is_empty() { continue; }
            let data = state.k8s.kubectl_json(&["get", "cnp", "-n", ns, "-o", "json"]).await;
            if let Some(items) = data.get("items").and_then(|i| i.as_array()) {
                for it in items {
                    let name = it.pointer("/metadata/name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                    if name.is_empty() { continue; }
                    matching_policies.push(format!("cnp/{ns}/{name}"));
                    if denying_guess.is_none() && port > 0 && it.to_string().contains(&port.to_string()) {
                        denying_guess = Some(name);
                    }
                }
            }
        }
        let ccnp = state.k8s.kubectl_json(&["get", "ccnp", "-o", "json"]).await;
        if let Some(items) = ccnp.get("items").and_then(|i| i.as_array()) {
            for it in items.iter().take(8) {
                if let Some(name) = it.pointer("/metadata/name").and_then(|n| n.as_str()) {
                    matching_policies.push(format!("ccnp/{name}"));
                }
            }
        }
        matching_policies.truncate(12);
    }
    steps.push(InvestigateStep {
        id: "policy".into(),
        title: "CNP / CCNP candidates".into(),
        detail: if matching_policies.is_empty() {
            if k8s_ok { "No CNP/CCNP listed — default-deny or remote identity may apply (inferred).".into() }
            else { "Cannot list policies without Kubernetes.".into() }
        } else {
            format!("Candidates: {}", matching_policies.join(", "))
        },
        confidence: if k8s_ok { Confidence::Inferred } else { Confidence::Unavailable },
        evidence: matching_policies.iter().take(5).map(|p| EvidenceRef { kind: "policy".into(), id: p.clone() }).collect(),
    });

    let is_drop = verdict.eq_ignore_ascii_case("DROPPED") || drop_reason.to_uppercase().contains("POLICY");
    steps.push(InvestigateStep {
        id: "drop".into(),
        title: "Drop reason".into(),
        detail: if drop_reason.is_empty() {
            if is_drop { "Verdict is DROPPED but Hubble did not include drop_reason_desc.".into() }
            else { format!("Verdict is {verdict} — not a deny.") }
        } else {
            format!("Hubble drop_reason_desc: {drop_reason}")
        },
        confidence: if drop_reason.is_empty() { Confidence::Inferred } else { Confidence::Observed },
        evidence: vec![],
    });

    let mut next_actions = Vec::new();
    let likely_owner = if port == 53 && is_drop { "dns" } else if is_drop { "policy" } else { "unknown" };

    if is_drop && !src_ns.is_empty() && !dst_ns.is_empty() && port > 0 {
        let fix = format!(
            "apiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy\nmetadata:\n  name: allow-{}-to-{}-{}\n  namespace: {}\nspec:\n  endpointSelector:\n    matchLabels: {{}}\n  egress:\n  - toEndpoints:\n    - matchLabels:\n        k8s:io.kubernetes.pod.namespace: {}\n    toPorts:\n    - ports:\n      - port: \"{}\"\n        protocol: {}\n",
            src_pod.replace('.', "-"), dst_pod.replace('.', "-"), port, src_ns, dst_ns, port, protocol.to_uppercase()
        );
        steps.push(InvestigateStep {
            id: "fix".into(),
            title: "Proposed minimal allow (review only)".into(),
            detail: format!("Draft CNP — simulate before apply. Confidence: inferred.\n\n{fix}"),
            confidence: Confidence::Inferred,
            evidence: vec![],
        });
        next_actions.push("Simulate the draft CNP (POST /api/v1/policies/simulate), then apply via GitOps/CRD.".into());
    }
    if let Some(n) = denying_guess {
        next_actions.push(format!("Review policy '{n}' selectors against this flow's identities."));
    }
    if stats.gap_count > 0 {
        next_actions.push(format!("Ingest reported {} gap(s) — evidence may be incomplete.", stats.gap_count));
    }
    next_actions.push("Enforcement stays in Cilium CRDs — Paqtra will not attach or rewrite BPF programs.".into());

    let result = InvestigateResult {
        id: id.clone(),
        request: json!({
            "flow_id": req.flow_id,
            "flow_ts": flow_ts,
            "path": {
                "source": { "namespace": src_ns, "name": src_pod },
                "destination": { "namespace": dst_ns, "name": dst_pod },
                "port": port,
                "protocol": protocol,
                "verdict": verdict,
                "drop_reason": drop_reason,
            }
        }),
        steps: steps.clone(),
        likely_owner: likely_owner.into(),
        next_actions: next_actions.clone(),
        flow_ingest: json!({
            "total": stats.total,
            "last_ok": stats.last_ingest_ok,
            "last_at": stats.last_ingest_at,
            "source": stats.ingest_source,
            "gaps": stats.gap_count,
            "connected": stats.stream_connected,
            "confidence": if stats.last_ingest_ok { "observed" } else { "unavailable" },
        }),
        created_at: Utc::now().to_rfc3339(),
    };

    let _ = state.cache.set_persistent(
        &format!("cv:investigate_bundle:{}", id),
        &json!({ "id": id, "kind": "flow_deny", "result": serde_json::to_value(&result).unwrap_or(json!({})) }),
    ).await;

    result
}


fn count_name_matches(list: &Value, hint: &str) -> usize {
    let hint = hint.to_lowercase();
    list.get("items")
        .and_then(|i| i.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|it| {
                    let name = it
                        .pointer("/metadata/name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_lowercase();
                    !hint.is_empty() && (name.contains(&hint) || hint.contains(&name))
                })
                .count()
        })
        .unwrap_or(0)
}

fn endpoint_address_count(eps: &Value, hint: &str) -> usize {
    let hint = hint.to_lowercase();
    eps.get("items")
        .and_then(|i| i.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|it| {
                    let name = it
                        .pointer("/metadata/name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_lowercase();
                    hint.is_empty() || name.contains(&hint) || hint.contains(&name)
                })
                .map(|it| {
                    it.pointer("/subsets")
                        .and_then(|s| s.as_array())
                        .map(|subs| {
                            subs.iter()
                                .map(|s| {
                                    s.get("addresses")
                                        .and_then(|a| a.as_array())
                                        .map(|a| a.len())
                                        .unwrap_or(0)
                                })
                                .sum::<usize>()
                        })
                        .unwrap_or(0)
                })
                .sum()
        })
        .unwrap_or(0)
}

fn build_bundle(
    result: &InvestigateResult,
    path_flows: &[StoredFlow],
    dropped: &[StoredFlow],
    policies: &[&crate::models::policy::Policy],
    changes: &[Value],
    stats: &crate::services::flow_store::FlowStoreStats,
    time_window_minutes: i64,
) -> Value {
    let cited_flow_ids: Vec<String> = path_flows
        .iter()
        .chain(dropped.iter())
        .map(|f| f.id.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .take(100)
        .collect();
    let change_ids: Vec<String> = changes
        .iter()
        .filter_map(|c| c.get("id").and_then(|v| v.as_str()).map(str::to_string))
        .take(20)
        .collect();

    json!({
        "id": result.id,
        "created_at": result.created_at,
        "likely_owner": result.likely_owner,
        "request": result.request,
        "time_range": {
            "window_minutes": time_window_minutes,
            "captured_at": result.created_at,
        },
        "steps": result.steps,
        "next_actions": result.next_actions,
        "source_health": {
            "ingest_source": stats.ingest_source,
            "stream_connected": stats.stream_connected,
            "last_ingest_ok": stats.last_ingest_ok,
            "last_ingest_at": stats.last_ingest_at,
            "gap_count": stats.gap_count,
            "last_gap_at": stats.last_gap_at,
            "disconnect_count": stats.disconnect_count,
            "lag_secs": stats.lag_secs,
        },
        "ingest_gaps": {
            "count": stats.gap_count,
            "last_at": stats.last_gap_at,
        },
        "cited_flow_ids": cited_flow_ids,
        "related_change_ids": change_ids,
        "kubernetes_resources": {
            "policies": policies.iter().take(20).map(|p| json!({
                "name": p.name,
                "namespace": p.namespace,
                "kind": "CiliumNetworkPolicy",
            })).collect::<Vec<_>>(),
        },
        "flows": path_flows.iter().take(50).map(|f| json!({
            "id": f.id,
            "ts": f.ts,
            "verdict": f.verdict,
            "port": f.port,
            "src": format!("{}/{}", f.src_namespace, f.src_pod),
            "dst": format!("{}/{}", f.dst_namespace, f.dst_pod),
            "source": f.source.as_str(),
        })).collect::<Vec<_>>(),
        "dropped_sample": dropped.iter().take(20).map(|f| f.id.clone()).collect::<Vec<_>>(),
        "policies": policies.iter().take(20).map(|p| json!({
            "name": p.name,
            "namespace": p.namespace,
        })).collect::<Vec<_>>(),
        "redaction": "No payloads, argv, or Secret contents included.",
        "confidence_rule": "Each step carries observed|inferred|unavailable; inaccessible evidence is omitted.",
    })
}

/// Render a concise Markdown incident summary from a stored (redacted) bundle.
pub fn bundle_to_markdown(bundle: &Value) -> String {
    let id = bundle.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
    let owner = bundle
        .get("likely_owner")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let created = bundle
        .get("created_at")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mut md = format!("# Investigation {id}\n\n");
    md.push_str(&format!("- Created: {created}\n"));
    md.push_str(&format!("- Likely owner: **{owner}**\n"));
    if let Some(req) = bundle.get("request") {
        md.push_str(&format!(
            "- Request: `{}`\n",
            serde_json::to_string(req).unwrap_or_default()
        ));
    }
    md.push_str("\n## Source health\n\n");
    if let Some(h) = bundle.get("source_health") {
        md.push_str(&format!(
            "- Ingest: {} (connected={}, gaps={})\n",
            h.get("ingest_source")
                .and_then(|v| v.as_str())
                .unwrap_or("?"),
            h.get("stream_connected")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            h.get("gap_count").and_then(|v| v.as_u64()).unwrap_or(0)
        ));
    }
    md.push_str("\n## Steps\n\n");
    if let Some(steps) = bundle.get("steps").and_then(|v| v.as_array()) {
        for s in steps {
            let title = s.get("title").and_then(|v| v.as_str()).unwrap_or("step");
            let conf = s
                .get("confidence")
                .and_then(|v| v.as_str())
                .unwrap_or("unavailable");
            let detail = s.get("detail").and_then(|v| v.as_str()).unwrap_or("");
            md.push_str(&format!("### {title} ({conf})\n\n{detail}\n\n"));
        }
    }
    md.push_str("## Evidence\n\n");
    if let Some(ids) = bundle.get("cited_flow_ids").and_then(|v| v.as_array()) {
        md.push_str(&format!(
            "- Flow IDs: {}\n",
            ids.iter()
                .filter_map(|v| v.as_str())
                .take(30)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(ids) = bundle.get("related_change_ids").and_then(|v| v.as_array()) {
        md.push_str(&format!(
            "- Change IDs: {}\n",
            ids.iter()
                .filter_map(|v| v.as_str())
                .take(20)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    md.push_str("\n---\n*Redacted: no payloads, argv, or Secret contents.*\n");
    md
}

/// Namespace labels referenced by a bundle (for RBAC on retrieval).
pub fn bundle_namespaces(bundle: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(req) = bundle.get("request") {
        for path in ["/source/namespace", "/destination/namespace", "/path/source/namespace", "/path/destination/namespace"] {
            if let Some(ns) = req.pointer(path).and_then(|v| v.as_str()) {
                if !ns.is_empty() {
                    out.push(ns.to_string());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}


// ---------------------------------------------------------------------------
// Policy preview (replaces heuristic simulate)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct PolicyPreview {
    pub policy: String,
    pub namespace: String,
    pub analysis: Value,
    pub impact: Value,
    pub pairs: Vec<Value>,
    pub uncertainty: Vec<String>,
    pub rollback: Value,
    pub confidence: Confidence,
}

pub async fn preview_policy(
    state: &AppState,
    name: &str,
    namespace: &str,
    spec: &Value,
) -> PolicyPreview {
    let mut uncertainty = Vec::new();
    let unsupported = detect_unsupported(spec);
    uncertainty.extend(unsupported.clone());

    let ingress_rules = spec
        .get("ingress")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let egress_rules = spec
        .get("egress")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    let selector_labels = endpoint_selector_labels(spec);
    let ports = collect_ports(spec);

    let k8s_ok = state.k8s.is_healthy().await;
    let mut matched_endpoints = 0usize;
    if k8s_ok && !selector_labels.is_empty() {
        let pods = state
            .k8s
            .kubectl_json(&["get", "pods", "-n", namespace, "-o", "json"])
            .await;
        matched_endpoints = pods
            .get("items")
            .and_then(|i| i.as_array())
            .map(|arr| {
                arr.iter()
                    .filter(|p| {
                        let labels = p.pointer("/metadata/labels").cloned().unwrap_or(json!({}));
                        selector_labels.iter().all(|(k, v)| {
                            labels.get(k).and_then(|x| x.as_str()) == Some(v.as_str())
                        })
                    })
                    .count()
            })
            .unwrap_or(0);
    } else if selector_labels.is_empty() {
        uncertainty.push(
            "Empty or missing endpointSelector — namespace-wide scope; endpoint match unknown."
                .into(),
        );
    } else if !k8s_ok {
        uncertainty.push("Kubernetes unavailable — cannot resolve selectors to endpoints.".into());
    }

    let since = (Utc::now() - ChronoDuration::hours(24)).to_rfc3339();
    let recent = state
        .flow_store
        .query(&FlowQuery {
            src_namespace: Some(namespace.to_string()),
            since_rfc3339: Some(since),
            limit: 500,
            ..Default::default()
        })
        .unwrap_or_default();

    let mut pairs = Vec::new();
    let mut would_allow = 0u64;
    let mut would_deny = 0u64;
    let mut unknown_n = 0u64;

    if !unsupported.is_empty() {
        unknown_n = recent.len() as u64;
        uncertainty.push(
            "Unsupported constructs present — matching flows marked unknown, not a confident pass."
                .into(),
        );
        for f in recent.iter().take(30) {
            pairs.push(json!({
                "src": format!("{}/{}", f.src_namespace, f.src_pod),
                "dst": format!("{}/{}", f.dst_namespace, f.dst_pod),
                "port": f.port,
                "outcome": "unknown",
                "confidence": "unavailable",
                "flow_id": f.id,
            }));
        }
    } else {
        for f in &recent {
            let port_ok = ports.is_empty() || ports.contains(&f.port);
            let in_ns = f.src_namespace == namespace || f.dst_namespace == namespace;
            if !in_ns {
                continue;
            }
            // L3/L4 allow-list style inference only.
            let outcome = if port_ok && (ingress_rules + egress_rules) > 0 {
                would_allow += 1;
                "would_allow"
            } else if (ingress_rules + egress_rules) == 0 {
                would_deny += 1;
                "would_deny"
            } else {
                unknown_n += 1;
                "unknown"
            };
            if pairs.len() < 40 {
                pairs.push(json!({
                    "src": format!("{}/{}", f.src_namespace, f.src_pod),
                    "dst": format!("{}/{}", f.dst_namespace, f.dst_pod),
                    "port": f.port,
                    "prior_verdict": f.verdict,
                    "outcome": outcome,
                    "confidence": if outcome == "unknown" { "unavailable" } else { "inferred" },
                    "flow_id": f.id,
                }));
            }
        }
        if recent.is_empty() {
            uncertainty.push(
                "No indexed flows in namespace for the last 24h — impact pairs unavailable.".into(),
            );
        }
    }

    let confidence = if !unsupported.is_empty() {
        Confidence::Unavailable
    } else if k8s_ok {
        Confidence::Inferred
    } else {
        Confidence::Unavailable
    };

    PolicyPreview {
        policy: name.to_string(),
        namespace: namespace.to_string(),
        analysis: json!({
            "ingress_rules": ingress_rules,
            "egress_rules": egress_rules,
            "total_rules": ingress_rules + egress_rules,
            "endpoint_selector_labels": selector_labels,
            "ports": ports,
            "matched_endpoints": matched_endpoints,
            "unsupported_constructs": unsupported,
            "flows_considered": recent.len(),
        }),
        impact: json!({
            "would_allow": would_allow,
            "would_deny": would_deny,
            "unknown": unknown_n,
            "matched_endpoints": matched_endpoints,
            "risk_level": if !unsupported.is_empty() {
                "unknown"
            } else if selector_labels.is_empty() && ports.is_empty() {
                "high"
            } else if ports.is_empty() {
                "medium"
            } else {
                "low"
            },
            // Back-compat fields (explicitly inferred, not observed counts)
            "estimated_flows_affected": would_allow + would_deny + unknown_n,
            "services_impacted": matched_endpoints.max(1),
            "confidence": confidence,
        }),
        pairs,
        uncertainty,
        rollback: json!({
            "method": "cilium_crd",
            "steps": [
                format!("kubectl -n {namespace} delete ciliumnetworkpolicy {name}"),
                "Re-apply previous CNP revision from GitOps or backup if required",
                "Re-run POST /api/v1/investigate/path to verify connectivity contracts",
            ],
            "note": "Paqtra never writes Cilium BPF maps or attaches programs.",
        }),
        confidence,
    }
}

fn detect_unsupported(spec: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let blob = spec.to_string();
    if blob.contains("toFQDNs") || blob.contains("toFQDN") {
        out.push("toFQDNs not evaluated in v1 preview".into());
    }
    if blob.contains("\"http\"") || blob.contains("l7") || blob.contains("L7") {
        out.push("L7 rules not evaluated in v1 preview".into());
    }
    if blob.contains("toEntities") {
        out.push("toEntities matching is partial — results may be unknown".into());
    }
    if blob.contains("deny") && (blob.contains("ingress") || blob.contains("egress")) {
        // Cilium deny rules coexist with allows — we don't model precedence fully.
        if blob.contains("ingressDeny") || blob.contains("egressDeny") {
            out.push("ingressDeny/egressDeny precedence not fully modeled".into());
        }
    }
    out
}

fn endpoint_selector_labels(spec: &Value) -> Vec<(String, String)> {
    spec.pointer("/endpointSelector/matchLabels")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn collect_ports(spec: &Value) -> Vec<u16> {
    let mut ports = Vec::new();
    for key in ["ingress", "egress"] {
        if let Some(rules) = spec.get(key).and_then(|v| v.as_array()) {
            for rule in rules {
                if let Some(to_ports) = rule.get("toPorts").and_then(|v| v.as_array()) {
                    for tp in to_ports {
                        if let Some(plist) = tp.get("ports").and_then(|v| v.as_array()) {
                            for p in plist {
                                if let Some(n) = p
                                    .get("port")
                                    .and_then(|x| x.as_str())
                                    .and_then(|s| s.parse().ok())
                                    .or_else(|| {
                                        p.get("port").and_then(|x| x.as_u64()).map(|n| n as u16)
                                    })
                                {
                                    ports.push(n);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    ports.sort_unstable();
    ports.dedup();
    ports
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_fqdn() {
        let spec = json!({"egress": [{"toFQDNs": [{"matchName": "example.com"}]}]});
        let u = detect_unsupported(&spec);
        assert!(u.iter().any(|s| s.contains("toFQDNs")));
    }

    #[test]
    fn selector_and_ports() {
        let spec = json!({
            "endpointSelector": {"matchLabels": {"app": "web"}},
            "ingress": [{"toPorts": [{"ports": [{"port": "443", "protocol": "TCP"}]}]}]
        });
        assert_eq!(
            endpoint_selector_labels(&spec),
            vec![("app".into(), "web".into())]
        );
        assert_eq!(collect_ports(&spec), vec![443]);
    }
}
