// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Flow data model - canonical definition used across handlers and services
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Flow {
    pub id: String,
    pub timestamp: String,
    pub source: FlowEndpoint,
    pub destination: FlowEndpoint,
    pub verdict: String,
    pub protocol: String,
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster: Option<String>,
    /// L7 DNS query name when Hubble DNS visibility is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_qtypes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_rcode: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_rcode_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_ips: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_latency_ns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop_reason: Option<String>,
    /// Hubble context beyond the 5-tuple: identities, labels, direction, policy match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hubble: Option<FlowMeta>,
}

/// What Hubble reports about a flow besides who talked to whom. Every field is
/// optional: the CLI fallback and older Cilium versions omit some of them.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct FlowMeta {
    /// `INGRESS` or `EGRESS`, from the observing endpoint's point of view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traffic_direction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_reply: Option<bool>,
    /// Which policy layers matched a policy verdict, see [`policy_match_name`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_match_type: Option<String>,
    /// Where in the datapath the flow was observed (`TO_ENDPOINT`, `FROM_NETWORK`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_observation_point: Option<String>,
    /// The Cilium node that observed the flow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_name: Option<String>,
    /// Cilium security identity numbers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_identity: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_identity: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destination_labels: Vec<String>,
}

impl FlowMeta {
    /// `None` when nothing is set, so flows without context serialize unchanged.
    pub fn or_none(self) -> Option<Self> {
        (self != Self::default()).then_some(self)
    }
}

/// Endpoint labels kept per flow. Hubble sends every label of the identity,
/// which can be dozens; flows are held in memory by the thousand.
pub const MAX_FLOW_LABELS: usize = 16;

/// Name of Cilium's `PolicyMatchType` (datapath policy verdict events).
/// 0 means "not applicable / none" and is reported as absent.
pub fn policy_match_name(code: u32) -> Option<String> {
    match code {
        0 => None,
        1 => Some("l3-only".into()),
        2 => Some("l3-l4".into()),
        3 => Some("l4-only".into()),
        4 => Some("all".into()),
        n => Some(format!("type-{n}")),
    }
}

/// IANA DNS RCODE short name.
pub fn dns_rcode_name(code: u32) -> &'static str {
    match code {
        0 => "NOERROR",
        1 => "FORMERR",
        2 => "SERVFAIL",
        3 => "NXDOMAIN",
        4 => "NOTIMP",
        5 => "REFUSED",
        6 => "YXDOMAIN",
        7 => "YXRRSET",
        8 => "NXRRSET",
        9 => "NOTAUTH",
        10 => "NOTZONE",
        _ => "UNKNOWN",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlowEndpoint {
    pub namespace: String,
    pub pod: String,
    pub ip: String,
}

#[derive(Debug, Deserialize)]
pub struct FlowQueryParams {
    pub namespace: Option<String>,
    pub verdict: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowStats {
    pub total_flows: u64,
    pub forwarded: u64,
    pub dropped: u64,
    pub requests_per_second: f64,
    pub avg_latency_ms: f64,
}

impl Default for FlowStats {
    fn default() -> Self {
        Self {
            total_flows: 0,
            forwarded: 0,
            dropped: 0,
            requests_per_second: 0.0,
            avg_latency_ms: 0.0,
        }
    }
}
