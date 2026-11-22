// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Cilium and Hubble metrics read from Prometheus.
//
// Cilium exports its own metrics (agent, operator) and Hubble exports flow
// metrics when `hubble.metrics.enabled` lists them. Paqtra does not scrape
// anything itself: it asks the Prometheus that already does, with a fixed
// catalog of queries, so there is no user-supplied PromQL to validate.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{json, Value};

use super::prometheus::PrometheusService;

/// One named query in a catalog.
pub struct MetricDef {
    pub key: &'static str,
    pub title: &'static str,
    /// What one sample means, e.g. `flows/s`.
    pub unit: &'static str,
    /// Which Cilium setting exports it, shown when the series is empty.
    pub needs: &'static str,
    pub promql: &'static str,
}

/// Hubble flow metrics. Each needs its name in `hubble.metrics.enabled`.
pub const HUBBLE_METRICS: &[MetricDef] = &[
    MetricDef {
        key: "flows_by_verdict",
        title: "Flows by verdict",
        unit: "flows/s",
        needs: "hubble.metrics.enabled=flow",
        promql: "sum by (verdict, type) (rate(hubble_flows_processed_total[5m]))",
    },
    MetricDef {
        key: "drops_by_reason",
        title: "Drops by reason",
        unit: "drops/s",
        needs: "hubble.metrics.enabled=drop",
        promql: "sum by (reason, protocol) (rate(hubble_drop_total[5m]))",
    },
    MetricDef {
        key: "policy_verdicts",
        title: "Policy verdicts",
        unit: "verdicts/s",
        needs: "hubble.metrics.enabled=policy",
        promql: "sum by (verdict, match, direction) (rate(hubble_policy_verdicts_total[5m]))",
    },
    MetricDef {
        key: "dns_responses",
        title: "DNS responses by rcode",
        unit: "responses/s",
        needs: "hubble.metrics.enabled=dns",
        promql: "sum by (rcode) (rate(hubble_dns_responses_total[5m]))",
    },
    MetricDef {
        key: "http_requests",
        title: "HTTP requests by status",
        unit: "requests/s",
        needs: "hubble.metrics.enabled=http (L7 visibility)",
        promql: "sum by (method, status) (rate(hubble_http_requests_total[5m]))",
    },
    MetricDef {
        key: "tcp_flags",
        title: "TCP flags",
        unit: "packets/s",
        needs: "hubble.metrics.enabled=tcp",
        promql: "sum by (flag, family) (rate(hubble_tcp_flags_total[5m]))",
    },
    MetricDef {
        key: "icmp",
        title: "ICMP by type",
        unit: "packets/s",
        needs: "hubble.metrics.enabled=icmp",
        promql: "sum by (family, type) (rate(hubble_icmp_total[5m]))",
    },
    MetricDef {
        key: "lost_events",
        title: "Lost events",
        unit: "events/s",
        needs: "Hubble ring buffer overflow (raise hubble.eventBufferCapacity)",
        promql: "sum by (source) (rate(hubble_lost_events_total[5m]))",
    },
];

/// Cilium agent and operator metrics.
pub const CILIUM_METRICS: &[MetricDef] = &[
    MetricDef {
        key: "drops",
        title: "Datapath drops by reason",
        unit: "packets/s",
        needs: "cilium-agent Prometheus metrics (prometheus.enabled=true)",
        promql: "sum by (reason, direction) (rate(cilium_drop_count_total[5m]))",
    },
    MetricDef {
        key: "forwards",
        title: "Forwarded packets",
        unit: "packets/s",
        needs: "cilium-agent Prometheus metrics",
        promql: "sum by (direction) (rate(cilium_forward_count_total[5m]))",
    },
    MetricDef {
        key: "endpoints_by_state",
        title: "Endpoints by state",
        unit: "endpoints",
        needs: "cilium-agent Prometheus metrics",
        promql: "sum by (endpoint_state) (cilium_endpoint_state)",
    },
    MetricDef {
        key: "policy_enforcement",
        title: "Endpoints by policy enforcement",
        unit: "endpoints",
        needs: "cilium-agent Prometheus metrics",
        promql: "sum by (enforcement) (cilium_policy_endpoint_enforcement_status)",
    },
    MetricDef {
        key: "policy_import_errors",
        title: "Policy import errors",
        unit: "errors/s",
        needs: "cilium-agent Prometheus metrics",
        promql: "sum(rate(cilium_policy_import_errors_total[5m]))",
    },
    MetricDef {
        key: "unreachable_nodes",
        title: "Unreachable nodes",
        unit: "nodes",
        needs: "cilium-agent Prometheus metrics",
        promql: "sum(cilium_unreachable_nodes)",
    },
    MetricDef {
        key: "errors_warnings",
        title: "Agent errors and warnings",
        unit: "messages/s",
        needs: "cilium-agent Prometheus metrics",
        promql: "sum by (level) (rate(cilium_errors_warnings_total[5m]))",
    },
    MetricDef {
        key: "bpf_map_pressure",
        title: "BPF map pressure",
        unit: "ratio (0-1)",
        needs: "cilium-agent Prometheus metrics (Cilium 1.15+)",
        promql: "max by (map_name) (cilium_bpf_map_pressure)",
    },
];

/// One sample of a query result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Series {
    pub labels: BTreeMap<String, String>,
    pub value: f64,
}

/// The samples of an instant-vector response, largest first. Non-finite values
/// (`NaN` from a 0/0 rate) are dropped: JSON cannot carry them and they mean
/// "no data".
pub fn parse_vector(response: &Value) -> Result<Vec<Series>, String> {
    if response.get("status").and_then(Value::as_str) == Some("error") {
        return Err(response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Prometheus reported an error")
            .to_string());
    }
    let result = response
        .pointer("/data/result")
        .and_then(Value::as_array)
        .ok_or("unexpected Prometheus response shape")?;
    let mut out: Vec<Series> = result
        .iter()
        .filter_map(|r| {
            let value = r
                .pointer("/value/1")
                .and_then(Value::as_str)?
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())?;
            let labels = r
                .get("metric")
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            Some(Series { labels, value })
        })
        .collect();
    out.sort_by(|a, b| b.value.total_cmp(&a.value));
    Ok(out)
}

/// Run a catalog against Prometheus. A query that fails or returns nothing
/// does not fail the response: the entry says so, since an unexported metric
/// is the normal case on a cluster with a narrow `hubble.metrics.enabled`.
pub async fn collect(prom: &PrometheusService, defs: &'static [MetricDef]) -> Value {
    if !prom.is_configured() {
        return json!({
            "available": false,
            "reason": "PROMETHEUS_URL is not set",
            "metrics": [],
        });
    }
    let queries = defs.iter().map(|d| async move {
        let result = match prom.query(d.promql).await {
            Ok(resp) => parse_vector(&resp),
            Err(e) => Err(e.to_string()),
        };
        (d, result)
    });
    let results = join_all(queries).await;

    let mut any_data = false;
    let metrics: Vec<Value> = results
        .into_iter()
        .map(|(d, result)| {
            let (series, error) = match result {
                Ok(s) => (s, None),
                Err(e) => (Vec::new(), Some(e)),
            };
            any_data |= !series.is_empty();
            json!({
                "key": d.key,
                "title": d.title,
                "unit": d.unit,
                "series": series,
                // Only explain an empty result: it is usually a setting, not a fault.
                "hint": series.is_empty().then_some(d.needs),
                "error": error,
            })
        })
        .collect();
    json!({ "available": any_data, "source": "prometheus", "window": "5m", "metrics": metrics })
}

/// Await every future concurrently, keeping order. Polled from one task, so the
/// futures may borrow (a `JoinSet` would need `'static`).
async fn join_all<F: std::future::Future>(futs: impl IntoIterator<Item = F>) -> Vec<F::Output> {
    let mut pinned: Vec<std::pin::Pin<Box<F>>> = futs.into_iter().map(Box::pin).collect();
    let mut results: Vec<Option<F::Output>> = pinned.iter().map(|_| None).collect();
    std::future::poll_fn(|cx| {
        let mut pending = false;
        for (slot, fut) in results.iter_mut().zip(pinned.iter_mut()) {
            if slot.is_none() {
                match fut.as_mut().poll(cx) {
                    std::task::Poll::Ready(v) => *slot = Some(v),
                    std::task::Poll::Pending => pending = true,
                }
            }
        }
        if pending {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(())
        }
    })
    .await;
    results.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_instant_vector_largest_first() {
        let resp = json!({
            "status": "success",
            "data": { "resultType": "vector", "result": [
                { "metric": { "reason": "Policy denied", "direction": "INGRESS" }, "value": [1.0, "0.5"] },
                { "metric": { "reason": "Invalid source mac" }, "value": [1.0, "3.25"] },
            ]}
        });
        let s = parse_vector(&resp).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].value, 3.25);
        assert_eq!(s[0].labels["reason"], "Invalid source mac");
        assert_eq!(s[1].labels["direction"], "INGRESS");
    }

    #[test]
    fn drops_nan_and_unparseable_samples() {
        let resp = json!({ "status": "success", "data": { "result": [
            { "metric": { "a": "1" }, "value": [1.0, "NaN"] },
            { "metric": { "a": "2" }, "value": [1.0, "+Inf"] },
            { "metric": { "a": "3" }, "value": [1.0, "oops"] },
            { "metric": { "a": "4" }, "value": [1.0, "7"] },
        ]}});
        let s = parse_vector(&resp).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].labels["a"], "4");
    }

    #[test]
    fn an_empty_result_is_not_an_error() {
        let resp = json!({ "status": "success", "data": { "result": [] } });
        assert!(parse_vector(&resp).unwrap().is_empty());
    }

    #[test]
    fn prometheus_errors_and_odd_shapes_are_reported() {
        let err = json!({ "status": "error", "error": "parse error at char 3" });
        assert_eq!(parse_vector(&err).unwrap_err(), "parse error at char 3");
        assert!(parse_vector(&json!({ "hello": 1 })).is_err());
    }

    #[test]
    fn catalog_keys_are_unique_and_queries_nonempty() {
        for defs in [HUBBLE_METRICS, CILIUM_METRICS] {
            let mut keys: Vec<_> = defs.iter().map(|d| d.key).collect();
            keys.sort_unstable();
            keys.dedup();
            assert_eq!(keys.len(), defs.len(), "duplicate metric key");
            assert!(defs
                .iter()
                .all(|d| !d.promql.is_empty() && !d.needs.is_empty()));
        }
    }

    #[tokio::test]
    async fn unconfigured_prometheus_says_so_instead_of_failing() {
        let prom = PrometheusService::new(None);
        let out = collect(&prom, HUBBLE_METRICS).await;
        assert_eq!(out["available"], false);
        assert!(out["reason"].as_str().unwrap().contains("PROMETHEUS_URL"));
    }

    #[tokio::test]
    async fn join_all_keeps_order() {
        let futs = (0..5u64).map(|i| async move {
            tokio::time::sleep(std::time::Duration::from_millis(10 * (5 - i))).await;
            i
        });
        assert_eq!(join_all(futs).await, vec![0, 1, 2, 3, 4]);
    }
}
