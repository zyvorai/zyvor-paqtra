// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Stored flows as OpenTelemetry log records (OTLP/JSON).
//!
//! The mapping uses OpenTelemetry's stable network attributes where a flow has
//! the data (`source.address`, `destination.address`, `destination.port`,
//! `network.transport`, `network.type`) and Kubernetes endpoint names in the
//! `k8s.src.*` / `k8s.dst.*` form Grafana Beyla uses for its network flow
//! metric. Cilium's verdict and drop reason have no OpenTelemetry convention,
//! so they sit under `cilium.*`. The output is a document a collector can
//! ingest with the `otlpjson` receiver or that a script can POST to an OTLP/HTTP
//! logs endpoint; Paqtra itself sends nothing.
//!
//! Flow records are untrusted: addresses that do not parse are dropped rather
//! than copied, strings are cut to a fixed length, and a record with no usable
//! time is skipped and counted.

use super::flow_agg::{truncate, workload_of};
use super::flow_store::StoredFlow;
use chrono::DateTime;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::net::IpAddr;

/// Most records one document carries.
pub const MAX_RECORDS: usize = 1000;
const MAX_ATTR_CHARS: usize = 256;

const SEVERITY_INFO: u32 = 9;
const SEVERITY_WARN: u32 = 13;

fn s(key: &str, v: &str) -> Value {
    let v: String = truncate(v).chars().take(MAX_ATTR_CHARS).collect();
    json!({ "key": key, "value": { "stringValue": v } })
}

fn i(key: &str, v: i64) -> Value {
    // OTLP/JSON carries 64-bit integers as decimal strings.
    json!({ "key": key, "value": { "intValue": v.to_string() } })
}

/// Parse an address; only well-formed IPs are exported.
fn ip(v: &str) -> Option<IpAddr> {
    v.trim().parse().ok()
}

/// `network.transport` values OpenTelemetry defines that a flow can be.
fn transport(protocol: &str) -> Option<&'static str> {
    match protocol.to_ascii_lowercase().as_str() {
        "tcp" => Some("tcp"),
        "udp" => Some("udp"),
        _ => None,
    }
}

/// One log record, or None when the flow has no usable timestamp.
fn record(f: &StoredFlow) -> Option<Value> {
    let t = DateTime::parse_from_rfc3339(&f.ts).ok()?;
    let nanos = t.timestamp_nanos_opt()?;
    if nanos < 0 {
        return None;
    }
    let dropped = f.verdict == "DROPPED";
    let mut attrs: Vec<Value> = Vec::new();

    if let Some(a) = ip(&f.src_ip) {
        attrs.push(s("source.address", &a.to_string()));
        attrs.push(s("network.type", if a.is_ipv4() { "ipv4" } else { "ipv6" }));
    }
    if let Some(a) = ip(&f.dst_ip) {
        attrs.push(s("destination.address", &a.to_string()));
    }
    if f.port != 0 {
        attrs.push(i("destination.port", f.port as i64));
    }
    if let Some(t) = transport(&f.protocol) {
        attrs.push(s("network.transport", t));
    }
    for (side, ns, pod) in [
        ("src", &f.src_namespace, &f.src_pod),
        ("dst", &f.dst_namespace, &f.dst_pod),
    ] {
        if !ns.is_empty() {
            attrs.push(s(&format!("k8s.{side}.namespace"), ns));
        }
        if !pod.is_empty() {
            attrs.push(s(&format!("k8s.{side}.name"), pod));
            attrs.push(s(&format!("k8s.{side}.owner.name"), &workload_of(pod)));
        }
    }
    attrs.push(s("cilium.verdict", &f.verdict));
    if dropped && !f.drop_reason.is_empty() {
        attrs.push(s("cilium.drop_reason", &f.drop_reason));
    }
    attrs.push(s("paqtra.protocol", &f.protocol));
    attrs.push(s("paqtra.flow.id", &f.id));

    let body = format!(
        "{} {} {}/{} -> {}/{}:{}",
        truncate(&f.verdict),
        truncate(&f.protocol),
        truncate(&f.src_namespace),
        truncate(&f.src_pod),
        truncate(&f.dst_namespace),
        truncate(&f.dst_pod),
        f.port
    );
    Some(json!({
        "timeUnixNano": nanos.to_string(),
        "observedTimeUnixNano": nanos.to_string(),
        "severityNumber": if dropped { SEVERITY_WARN } else { SEVERITY_INFO },
        "severityText": if dropped { "WARN" } else { "INFO" },
        "body": { "stringValue": body },
        "attributes": attrs,
    }))
}

/// An OTLP/JSON logs document for `flows`, one resource per cluster, plus the
/// number of flows skipped for want of a usable timestamp. At most
/// [`MAX_RECORDS`] flows are read; the caller says whether it truncated.
pub fn to_otlp_logs(flows: &[StoredFlow], version: &str) -> (Value, usize) {
    let mut by_cluster: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut skipped = 0usize;
    for f in flows.iter().take(MAX_RECORDS) {
        match record(f) {
            Some(r) => by_cluster
                .entry(truncate(&f.cluster).chars().take(MAX_ATTR_CHARS).collect())
                .or_default()
                .push(r),
            None => skipped += 1,
        }
    }
    let resources: Vec<Value> = by_cluster
        .into_iter()
        .map(|(cluster, records)| {
            let mut res = Map::new();
            res.insert(
                "attributes".into(),
                json!([s("service.name", "paqtra"), s("k8s.cluster.name", &cluster)]),
            );
            json!({
                "resource": Value::Object(res),
                "scopeLogs": [{
                    "scope": { "name": "paqtra.flows", "version": version },
                    "logRecords": records,
                }],
            })
        })
        .collect();
    (json!({ "resourceLogs": resources }), skipped)
}

#[cfg(test)]
mod tests {
    use super::super::flow_agg::fixtures::flow;
    use super::*;

    fn attr<'a>(rec: &'a Value, key: &str) -> Option<&'a Value> {
        rec["attributes"]
            .as_array()?
            .iter()
            .find(|a| a["key"] == key)
            .map(|a| &a["value"])
    }

    fn first_record(doc: &Value) -> &Value {
        &doc["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]
    }

    #[test]
    fn maps_a_forwarded_flow_to_network_attributes() {
        let f = flow(
            "FORWARDED",
            ("prod", "web-7c9d8f6b5-x2k4p"),
            ("data", "db-0"),
            5432,
            "",
        );
        let (doc, skipped) = to_otlp_logs(&[f], "2.2.6");
        assert_eq!(skipped, 0);
        let r = first_record(&doc);
        assert_eq!(
            attr(r, "source.address").unwrap()["stringValue"],
            "10.0.0.1"
        );
        assert_eq!(
            attr(r, "destination.address").unwrap()["stringValue"],
            "10.0.0.2"
        );
        assert_eq!(attr(r, "destination.port").unwrap()["intValue"], "5432");
        assert_eq!(attr(r, "network.transport").unwrap()["stringValue"], "tcp");
        assert_eq!(attr(r, "network.type").unwrap()["stringValue"], "ipv4");
        assert_eq!(attr(r, "k8s.src.owner.name").unwrap()["stringValue"], "web");
        assert_eq!(attr(r, "k8s.dst.owner.name").unwrap()["stringValue"], "db");
        assert_eq!(
            attr(r, "cilium.verdict").unwrap()["stringValue"],
            "FORWARDED"
        );
        assert!(attr(r, "cilium.drop_reason").is_none());
        assert_eq!(r["severityNumber"], 9);
        assert_eq!(r["timeUnixNano"], "1793491200000000000");
        let res = &doc["resourceLogs"][0]["resource"]["attributes"];
        assert_eq!(res[0]["value"]["stringValue"], "paqtra");
        assert_eq!(res[1]["value"]["stringValue"], "local");
    }

    #[test]
    fn drops_carry_reason_and_warn_severity() {
        let f = flow("DROPPED", ("a", "x"), ("b", "y"), 80, "Policy denied");
        let (doc, _) = to_otlp_logs(&[f], "v");
        let r = first_record(&doc);
        assert_eq!(r["severityText"], "WARN");
        assert_eq!(
            attr(r, "cilium.drop_reason").unwrap()["stringValue"],
            "Policy denied"
        );
    }

    #[test]
    fn malformed_records_are_skipped_or_sanitised_never_copied() {
        let mut bad_time = flow("FORWARDED", ("a", "x"), ("b", "y"), 1, "");
        bad_time.ts = "yesterday".into();
        let mut pre_epoch = flow("FORWARDED", ("a", "x"), ("b", "y"), 1, "");
        pre_epoch.ts = "1960-01-01T00:00:00Z".into();
        let mut hostile = flow("FORWARDED", ("a\u{0}b", "p\nq"), ("b", "y"), 0, "");
        hostile.src_ip = "10.0.0.1; DROP TABLE flows".into();
        hostile.dst_ip = "::1".into();
        hostile.protocol = "SCTP".into();
        hostile.id = "i".repeat(10_000);
        let (doc, skipped) = to_otlp_logs(&[bad_time, pre_epoch, hostile], "v");
        assert_eq!(skipped, 2);
        let r = first_record(&doc);
        assert!(attr(r, "source.address").is_none());
        assert!(attr(r, "network.type").is_none());
        assert_eq!(
            attr(r, "destination.address").unwrap()["stringValue"],
            "::1"
        );
        assert!(attr(r, "destination.port").is_none());
        assert!(attr(r, "network.transport").is_none());
        assert_eq!(attr(r, "paqtra.protocol").unwrap()["stringValue"], "SCTP");
        let id = attr(r, "paqtra.flow.id").unwrap()["stringValue"]
            .as_str()
            .unwrap();
        assert_eq!(id.chars().count(), super::super::flow_agg::MAX_LABEL_CHARS);
        let text = doc.to_string();
        assert!(!text.contains("\\u0000") && !text.contains("DROP TABLE"));
    }

    #[test]
    fn output_is_bounded_and_grouped_by_cluster() {
        let mut flows: Vec<StoredFlow> = (0..MAX_RECORDS + 500)
            .map(|n| {
                let mut f = flow("FORWARDED", ("a", "x"), ("b", "y"), 80, "");
                f.cluster = format!("c{}", n % 2);
                f
            })
            .collect();
        let (doc, _) = to_otlp_logs(&flows, "v");
        let total: usize = doc["resourceLogs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["scopeLogs"][0]["logRecords"].as_array().unwrap().len())
            .sum();
        assert_eq!(total, MAX_RECORDS);
        assert_eq!(doc["resourceLogs"].as_array().unwrap().len(), 2);
        flows.clear();
        let (empty, skipped) = to_otlp_logs(&flows, "v");
        assert_eq!(empty["resourceLogs"].as_array().unwrap().len(), 0);
        assert_eq!(skipped, 0);
    }
}
