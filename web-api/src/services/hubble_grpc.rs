// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Native Hubble client: talks to the Observer gRPC API of Hubble Relay (or an
// agent) directly, so the `hubble` binary is not needed.
//
// The protos are vendored in `proto/cilium` and compiled by `build.rs`.

use crate::models::flow::{Flow, FlowEndpoint, FlowMeta};
use anyhow::{Context, Result};
use std::time::Duration;
use tonic::transport::{Channel, Endpoint};

/// Generated protobuf types. The generated files refer to each other as
/// `super::flow`, `super::relay`, so they must be sibling modules.
#[allow(clippy::all, dead_code, unused_qualifications)]
pub mod pb {
    pub mod flow {
        include!(concat!(env!("OUT_DIR"), "/flow.rs"));
    }
    pub mod relay {
        include!(concat!(env!("OUT_DIR"), "/relay.rs"));
    }
    pub mod observer {
        include!(concat!(env!("OUT_DIR"), "/observer.rs"));
    }
}

use pb::flow::{layer4, layer7, Flow as PbFlow, FlowFilter, Verdict};
use pb::observer::{
    get_flows_response::ResponseTypes, observer_client::ObserverClient, GetFlowsRequest,
    GetFlowsResponse, GetNodesRequest, ServerStatusRequest,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Bound for one-shot calls. A snapshot of 10,000 flows from a large cluster
/// can take a while, but not unboundedly.
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(20);
const STATUS_TIMEOUT: Duration = Duration::from_secs(3);
/// `GetNodes` through Relay fans out to every node and takes seconds on a busy
/// cluster (2-8s measured against a 15M-flow node), unlike a `ServerStatus` ping.
const NODES_TIMEOUT: Duration = Duration::from_secs(20);

/// Connect to an Observer at `host:port` (plaintext gRPC, which is how Hubble
/// Relay serves inside a cluster).
async fn connect(address: &str) -> Result<ObserverClient<Channel>> {
    let channel = Endpoint::from_shared(format!("http://{address}"))
        .with_context(|| format!("invalid Hubble address '{address}'"))?
        .connect_timeout(CONNECT_TIMEOUT)
        .tcp_nodelay(true)
        .connect()
        .await
        .with_context(|| format!("cannot connect to Hubble at {address}"))?;
    Ok(ObserverClient::new(channel))
}

/// Whether the Observer at `address` answers `ServerStatus`.
pub async fn is_healthy(address: &str) -> bool {
    let call = async {
        let mut client = connect(address).await?;
        client
            .server_status(ServerStatusRequest {})
            .await
            .map(|_| ())
            .map_err(anyhow::Error::from)
    };
    matches!(tokio::time::timeout(STATUS_TIMEOUT, call).await, Ok(Ok(())))
}

/// One node behind Hubble Relay, from `GetNodes`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct HubbleNode {
    pub name: String,
    pub version: String,
    pub address: String,
    /// `NODE_CONNECTED`, `NODE_UNAVAILABLE`, ... (Relay); empty against a single agent.
    pub state: String,
    pub tls_enabled: bool,
    pub uptime_seconds: u64,
    /// Flows currently held in the node's ring buffer, its capacity, and the
    /// total seen since start. `num_flows == max_flows` means the buffer wraps.
    pub num_flows: u64,
    pub max_flows: u64,
    pub seen_flows: u64,
}

/// The nodes Hubble knows about, with per-node flow-buffer figures.
pub async fn nodes(address: &str) -> Result<Vec<HubbleNode>> {
    let call = async {
        let mut client = connect(address).await?;
        let resp = client
            .get_nodes(GetNodesRequest {})
            .await
            .context("GetNodes failed")?
            .into_inner();
        Ok::<_, anyhow::Error>(
            resp.nodes
                .into_iter()
                .map(|n| HubbleNode {
                    state: pb::relay::NodeState::try_from(n.state)
                        .ok()
                        .filter(|s| *s != pb::relay::NodeState::UnknownNodeState)
                        .map(|s| s.as_str_name().to_string())
                        .unwrap_or_default(),
                    tls_enabled: n.tls.map(|t| t.enabled).unwrap_or(false),
                    uptime_seconds: n.uptime_ns / 1_000_000_000,
                    name: n.name,
                    version: n.version,
                    address: n.address,
                    num_flows: n.num_flows,
                    max_flows: n.max_flows,
                    seen_flows: n.seen_flows,
                })
                .collect(),
        )
    };
    tokio::time::timeout(NODES_TIMEOUT, call)
        .await
        .context("timed out asking Hubble for its nodes")?
}

/// A verdict name as users write it (`dropped`, `DROPPED`), or `None` if it is
/// not one Hubble knows. Callers then skip the server-side filter and rely on
/// their own post-filter, so a typo returns nothing instead of everything.
pub fn parse_verdict(name: &str) -> Option<Verdict> {
    Verdict::from_str_name(&name.trim().to_ascii_uppercase())
}

/// The filter the `hubble observe --namespace` flag builds: a flow matches when
/// either end is in the namespace. `whitelist` entries are OR-ed; the fields of
/// one entry are AND-ed, so a verdict is repeated in every entry.
fn flow_filters(namespace: Option<&str>, verdict: Option<Verdict>) -> Vec<FlowFilter> {
    let verdicts: Vec<i32> = verdict.into_iter().map(|v| v as i32).collect();
    match namespace {
        None if verdicts.is_empty() => Vec::new(),
        None => vec![FlowFilter {
            verdict: verdicts,
            ..Default::default()
        }],
        Some(ns) => {
            let prefix = format!("{ns}/");
            vec![
                FlowFilter {
                    source_pod: vec![prefix.clone()],
                    verdict: verdicts.clone(),
                    ..Default::default()
                },
                FlowFilter {
                    destination_pod: vec![prefix],
                    verdict: verdicts,
                    ..Default::default()
                },
            ]
        }
    }
}

fn namespace_filters(namespace: Option<&str>) -> Vec<FlowFilter> {
    flow_filters(namespace, None)
}

/// The most recent `limit` flows, oldest first.
pub async fn last_flows(
    address: &str,
    limit: usize,
    namespace: Option<&str>,
    verdict: Option<&str>,
    cluster: Option<&str>,
) -> Result<Vec<Flow>> {
    let call = async {
        let mut client = connect(address).await?;
        let mut stream = client
            .get_flows(GetFlowsRequest {
                number: limit as u64,
                follow: false,
                whitelist: flow_filters(namespace, verdict.and_then(parse_verdict)),
                ..Default::default()
            })
            .await
            .context("GetFlows failed")?
            .into_inner();
        let mut flows = Vec::new();
        while let Some(msg) = stream.message().await.context("GetFlows stream failed")? {
            if let Some(mut flow) = flow_from_response(&msg) {
                if let Some(c) = cluster {
                    flow.cluster = Some(c.to_string());
                }
                flows.push(flow);
            }
        }
        Ok::<_, anyhow::Error>(flows)
    };
    tokio::time::timeout(SNAPSHOT_TIMEOUT, call)
        .await
        .context("timed out reading flows from Hubble")?
}

/// Follow new flows. Connection and request errors surface here; errors after
/// the stream has started arrive as `Err` items.
pub async fn follow_flows(
    address: &str,
    namespace: Option<&str>,
) -> Result<tonic::Streaming<GetFlowsResponse>> {
    let mut client = connect(address).await?;
    let stream = client
        .get_flows(GetFlowsRequest {
            follow: true,
            whitelist: namespace_filters(namespace),
            ..Default::default()
        })
        .await
        .context("GetFlows failed")?
        .into_inner();
    Ok(stream)
}

/// A flow in a stream message, or None for lost-event, node-status and agent
/// messages (which carry no flow).
pub fn flow_from_response(msg: &GetFlowsResponse) -> Option<Flow> {
    let ResponseTypes::Flow(flow) = msg.response_types.as_ref()? else {
        return None;
    };
    let mut out = flow_to_model(flow);
    if out.timestamp.is_empty() {
        // The response envelope carries a time as well.
        if let Some(t) = &msg.time {
            out.timestamp = format_timestamp(t);
        }
    }
    Some(out)
}

fn format_timestamp(t: &prost_types::Timestamp) -> String {
    use chrono::{DateTime, SecondsFormat};
    // A zero Timestamp is how protobuf spells "unset"; treat it as absent.
    if t.seconds == 0 && t.nanos == 0 {
        return String::new();
    }
    DateTime::from_timestamp(t.seconds, t.nanos.max(0) as u32)
        .map(|d| d.to_rfc3339_opts(SecondsFormat::AutoSi, true))
        .unwrap_or_default()
}

/// Convert a Hubble flow into our canonical `Flow`.
pub fn flow_to_model(f: &PbFlow) -> Flow {
    let endpoint = |ep: &Option<pb::flow::Endpoint>, ip: &str| FlowEndpoint {
        namespace: ep.as_ref().map(|e| e.namespace.clone()).unwrap_or_default(),
        pod: ep.as_ref().map(|e| e.pod_name.clone()).unwrap_or_default(),
        ip: ip.to_string(),
    };
    let (src_ip, dst_ip) =
        f.ip.as_ref()
            .map(|ip| (ip.source.as_str(), ip.destination.as_str()))
            .unwrap_or(("", ""));
    let source = endpoint(&f.source, src_ip);
    let destination = endpoint(&f.destination, dst_ip);

    let verdict = Verdict::try_from(f.verdict)
        .map(|v| v.as_str_name())
        .unwrap_or("UNKNOWN")
        .to_string();

    let (protocol, port) = match f.l4.as_ref().and_then(|l| l.protocol.as_ref()) {
        Some(layer4::Protocol::Tcp(t)) => ("TCP", t.destination_port),
        Some(layer4::Protocol::Udp(u)) => ("UDP", u.destination_port),
        Some(layer4::Protocol::Sctp(s)) => ("SCTP", s.destination_port),
        Some(layer4::Protocol::IcmPv4(_)) => ("ICMPv4", 0),
        Some(layer4::Protocol::IcmPv6(_)) => ("ICMPv6", 0),
        Some(layer4::Protocol::Vrrp(_)) => ("VRRP", 0),
        Some(layer4::Protocol::Igmp(_)) => ("IGMP", 0),
        None => ("UNKNOWN", 0),
    };
    let port = u16::try_from(port).unwrap_or(0);
    let protocol = protocol.to_string();

    let http = match f.l7.as_ref().and_then(|l| l.record.as_ref()) {
        Some(layer7::Record::Http(h)) => Some(h),
        _ => None,
    };
    let (dns, latency_ns) = match f.l7.as_ref() {
        Some(l7) => {
            let dns = match l7.record.as_ref() {
                Some(layer7::Record::Dns(d)) => Some(d),
                _ => None,
            };
            (dns, Some(l7.latency_ns).filter(|&n| n > 0))
        }
        None => (None, None),
    };

    let timestamp = f.time.as_ref().map(format_timestamp).unwrap_or_default();

    // Hubble's own id when it sends one; otherwise one derived from the flow's
    // content, so the same flow has the same id on every poll.
    let id = if f.uuid.is_empty() {
        super::hubble::content_id(&timestamp, &source, &destination, &verdict, &protocol, port)
    } else {
        f.uuid.clone()
    };

    let drop_reason = {
        use pb::flow::DropReason;
        DropReason::try_from(f.drop_reason_desc)
            .ok()
            .filter(|d| *d != DropReason::Unknown)
            .map(|d| d.as_str_name().to_string())
    };

    let (dns_query, dns_qtypes, dns_rcode, dns_rcode_name, dns_ips) = if let Some(d) = dns {
        (
            Some(d.query.clone()).filter(|q| !q.is_empty()),
            Some(d.qtypes.clone()).filter(|q| !q.is_empty()),
            Some(d.rcode),
            Some(crate::models::flow::dns_rcode_name(d.rcode).to_string()),
            Some(d.ips.clone()).filter(|i| !i.is_empty()),
        )
    } else {
        (None, None, None, None, None)
    };

    let labels = |ep: &Option<pb::flow::Endpoint>| -> Vec<String> {
        ep.as_ref()
            .map(|e| {
                e.labels
                    .iter()
                    .take(crate::models::flow::MAX_FLOW_LABELS)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    };
    let identity =
        |ep: &Option<pb::flow::Endpoint>| ep.as_ref().map(|e| e.identity).filter(|&i| i != 0);
    let hubble = FlowMeta {
        traffic_direction: pb::flow::TrafficDirection::try_from(f.traffic_direction)
            .ok()
            .filter(|d| *d != pb::flow::TrafficDirection::Unknown)
            .map(|d| d.as_str_name().to_string()),
        is_reply: f.is_reply,
        policy_match_type: crate::models::flow::policy_match_name(f.policy_match_type),
        trace_observation_point: pb::flow::TraceObservationPoint::try_from(
            f.trace_observation_point,
        )
        .ok()
        .filter(|p| *p != pb::flow::TraceObservationPoint::UnknownPoint)
        .map(|p| p.as_str_name().to_string()),
        node_name: Some(f.node_name.clone()).filter(|n| !n.is_empty()),
        source_identity: identity(&f.source),
        destination_identity: identity(&f.destination),
        source_labels: labels(&f.source),
        destination_labels: labels(&f.destination),
    }
    .or_none();

    Flow {
        id,
        timestamp,
        source,
        destination,
        verdict,
        protocol,
        port,
        hubble,
        http_method: http.map(|h| h.method.clone()),
        http_url: http.map(|h| h.url.clone()),
        http_code: http.map(|h| u16::try_from(h.code).unwrap_or(0)),
        cluster: None,
        dns_query,
        dns_qtypes,
        dns_rcode,
        dns_rcode_name,
        dns_ips,
        dns_latency_ns: latency_ns,
        drop_reason,
    }
}

/// An in-process fake Observer, shared with the service-level tests.
#[cfg(test)]
pub(crate) mod testing {
    use super::pb::flow::{Endpoint, Ip, Layer4, Tcp};
    use super::pb::observer::{
        observer_server::{Observer, ObserverServer},
        ServerStatusResponse,
    };
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio_stream::wrappers::TcpListenerStream;

    pub fn ts(seconds: i64, nanos: i32) -> Option<prost_types::Timestamp> {
        Some(prost_types::Timestamp { seconds, nanos })
    }

    pub fn ep(ns: &str, pod: &str) -> Option<Endpoint> {
        Some(Endpoint {
            namespace: ns.into(),
            pod_name: pod.into(),
            ..Default::default()
        })
    }

    pub fn tcp_flow(uuid: &str, verdict: Verdict, port: u32) -> PbFlow {
        PbFlow {
            time: ts(1_700_000_000, 123_456_000),
            uuid: uuid.into(),
            verdict: verdict as i32,
            ip: Some(Ip {
                source: "10.0.0.1".into(),
                destination: "10.0.0.2".into(),
                ..Default::default()
            }),
            l4: Some(Layer4 {
                protocol: Some(layer4::Protocol::Tcp(Tcp {
                    source_port: 40000,
                    destination_port: port,
                    flags: None,
                })),
            }),
            source: ep("shop", "web-1"),
            destination: ep("db", "pg-0"),
            ..Default::default()
        }
    }

    // ---- against an in-process fake Observer ----

    /// Serves a fixed set of flows and records the requests it received.
    pub struct Fake {
        pub flows: Vec<PbFlow>,
        pub seen: Arc<Mutex<Vec<GetFlowsRequest>>>,
    }

    type FlowStream = std::pin::Pin<
        Box<dyn tokio_stream::Stream<Item = Result<GetFlowsResponse, tonic::Status>> + Send>,
    >;

    #[tonic::async_trait]
    impl Observer for Fake {
        type GetFlowsStream = FlowStream;
        async fn get_flows(
            &self,
            request: tonic::Request<GetFlowsRequest>,
        ) -> Result<tonic::Response<Self::GetFlowsStream>, tonic::Status> {
            let req = request.into_inner();
            self.seen.lock().unwrap().push(req.clone());
            let mut items: Vec<Result<GetFlowsResponse, tonic::Status>> = Vec::new();
            // A node-status message first, as Relay sends: must not become a flow.
            items.push(Ok(GetFlowsResponse::default()));
            let take = if req.number == 0 {
                self.flows.len()
            } else {
                (req.number as usize).min(self.flows.len())
            };
            for f in &self.flows[self.flows.len() - take..] {
                items.push(Ok(GetFlowsResponse {
                    response_types: Some(ResponseTypes::Flow(f.clone())),
                    ..Default::default()
                }));
            }
            Ok(tonic::Response::new(Box::pin(tokio_stream::iter(items))))
        }
        async fn server_status(
            &self,
            _: tonic::Request<ServerStatusRequest>,
        ) -> Result<tonic::Response<ServerStatusResponse>, tonic::Status> {
            Ok(tonic::Response::new(ServerStatusResponse::default()))
        }

        type GetAgentEventsStream = std::pin::Pin<
            Box<
                dyn tokio_stream::Stream<
                        Item = Result<pb::observer::GetAgentEventsResponse, tonic::Status>,
                    > + Send,
            >,
        >;
        async fn get_agent_events(
            &self,
            _: tonic::Request<pb::observer::GetAgentEventsRequest>,
        ) -> Result<tonic::Response<Self::GetAgentEventsStream>, tonic::Status> {
            Err(tonic::Status::unimplemented(""))
        }
        type GetDebugEventsStream = std::pin::Pin<
            Box<
                dyn tokio_stream::Stream<
                        Item = Result<pb::observer::GetDebugEventsResponse, tonic::Status>,
                    > + Send,
            >,
        >;
        async fn get_debug_events(
            &self,
            _: tonic::Request<pb::observer::GetDebugEventsRequest>,
        ) -> Result<tonic::Response<Self::GetDebugEventsStream>, tonic::Status> {
            Err(tonic::Status::unimplemented(""))
        }
        async fn get_nodes(
            &self,
            _: tonic::Request<pb::observer::GetNodesRequest>,
        ) -> Result<tonic::Response<pb::observer::GetNodesResponse>, tonic::Status> {
            Ok(tonic::Response::new(pb::observer::GetNodesResponse {
                nodes: vec![pb::observer::Node {
                    name: "kind-worker".into(),
                    version: "cilium v1.19.0".into(),
                    address: "10.0.0.9:4244".into(),
                    state: pb::relay::NodeState::NodeConnected as i32,
                    uptime_ns: 90 * 1_000_000_000,
                    num_flows: 4095,
                    max_flows: 4095,
                    seen_flows: 123_456,
                    ..Default::default()
                }],
            }))
        }
        async fn get_namespaces(
            &self,
            _: tonic::Request<pb::observer::GetNamespacesRequest>,
        ) -> Result<tonic::Response<pb::observer::GetNamespacesResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented(""))
        }
    }

    pub async fn serve(flows: Vec<PbFlow>) -> (String, Arc<Mutex<Vec<GetFlowsRequest>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let svc = ObserverServer::new(Fake {
            flows,
            seen: seen.clone(),
        });
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(svc)
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        (addr.to_string(), seen)
    }
}

#[cfg(test)]
mod tests {
    use super::pb::flow::{Http, Layer4, Layer7, Udp};
    use super::testing::{serve, tcp_flow, ts};
    use super::*;

    #[test]
    fn converts_a_tcp_drop() {
        let f = flow_to_model(&tcp_flow("u-1", Verdict::Dropped, 5432));
        assert_eq!(f.id, "u-1");
        assert_eq!(f.verdict, "DROPPED");
        assert_eq!(f.protocol, "TCP");
        assert_eq!(f.port, 5432);
        assert_eq!(f.source.namespace, "shop");
        assert_eq!(f.source.pod, "web-1");
        assert_eq!(f.source.ip, "10.0.0.1");
        assert_eq!(f.destination.pod, "pg-0");
        assert_eq!(f.destination.ip, "10.0.0.2");
        assert_eq!(f.timestamp, "2023-11-14T22:13:20.123456Z");
    }

    #[test]
    fn verdict_names_parse_case_insensitively() {
        assert_eq!(parse_verdict("dropped"), Some(Verdict::Dropped));
        assert_eq!(parse_verdict(" FORWARDED "), Some(Verdict::Forwarded));
        assert_eq!(parse_verdict("audit"), Some(Verdict::Audit));
        assert_eq!(parse_verdict("drop"), None);
        assert_eq!(parse_verdict(""), None);
    }

    #[test]
    fn a_verdict_is_repeated_in_every_namespace_filter() {
        assert!(flow_filters(None, None).is_empty());
        let only = flow_filters(None, Some(Verdict::Dropped));
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].verdict, vec![Verdict::Dropped as i32]);

        let both = flow_filters(Some("shop"), Some(Verdict::Dropped));
        assert_eq!(both.len(), 2);
        // AND within an entry, OR across entries: the verdict must be in each.
        assert!(both
            .iter()
            .all(|f| f.verdict == vec![Verdict::Dropped as i32]));
        assert_eq!(both[0].source_pod, vec!["shop/".to_string()]);
        assert_eq!(both[1].destination_pod, vec!["shop/".to_string()]);
    }

    #[tokio::test]
    async fn the_verdict_filter_is_sent_to_hubble() {
        let (addr, seen) = serve(vec![tcp_flow("u", Verdict::Dropped, 80)]).await;
        last_flows(&addr, 10, None, Some("dropped"), None)
            .await
            .unwrap();
        // An unknown verdict is not sent: no filter at all, the caller post-filters.
        last_flows(&addr, 10, None, Some("nonsense"), None)
            .await
            .unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].whitelist[0].verdict, vec![Verdict::Dropped as i32]);
        assert!(seen[1].whitelist.is_empty());
    }

    #[tokio::test]
    async fn lists_nodes_with_buffer_figures() {
        let (addr, _) = serve(vec![]).await;
        let nodes = nodes(&addr).await.unwrap();
        assert_eq!(
            nodes,
            vec![HubbleNode {
                name: "kind-worker".into(),
                version: "cilium v1.19.0".into(),
                address: "10.0.0.9:4244".into(),
                state: "NODE_CONNECTED".into(),
                tls_enabled: false,
                uptime_seconds: 90,
                num_flows: 4095,
                max_flows: 4095,
                seen_flows: 123_456,
            }]
        );
    }

    #[test]
    fn carries_hubble_context() {
        use super::pb::flow::{TraceObservationPoint, TrafficDirection};
        let mut f = tcp_flow("u", Verdict::Dropped, 443);
        f.traffic_direction = TrafficDirection::Ingress as i32;
        f.is_reply = Some(false);
        f.policy_match_type = 2;
        f.trace_observation_point = TraceObservationPoint::ToEndpoint as i32;
        f.node_name = "kind-worker".into();
        f.source.as_mut().unwrap().identity = 4242;
        f.source.as_mut().unwrap().labels = vec!["k8s:app=web".into()];
        f.destination.as_mut().unwrap().identity = 7;

        let meta = flow_to_model(&f).hubble.expect("context present");
        assert_eq!(meta.traffic_direction.as_deref(), Some("INGRESS"));
        assert_eq!(meta.is_reply, Some(false));
        assert_eq!(meta.policy_match_type.as_deref(), Some("l3-l4"));
        assert_eq!(meta.trace_observation_point.as_deref(), Some("TO_ENDPOINT"));
        assert_eq!(meta.node_name.as_deref(), Some("kind-worker"));
        assert_eq!(meta.source_identity, Some(4242));
        assert_eq!(meta.destination_identity, Some(7));
        assert_eq!(meta.source_labels, vec!["k8s:app=web".to_string()]);
        assert!(meta.destination_labels.is_empty());
    }

    #[test]
    fn a_flow_without_context_has_no_hubble_object() {
        let f = flow_to_model(&tcp_flow("u", Verdict::Forwarded, 80));
        assert!(f.hubble.is_none());
        let json = serde_json::to_value(&f).unwrap();
        assert!(
            json.get("hubble").is_none(),
            "serialized shape is unchanged"
        );
    }

    #[test]
    fn labels_are_capped() {
        let mut f = tcp_flow("u", Verdict::Forwarded, 80);
        f.source.as_mut().unwrap().labels = (0..100).map(|i| format!("k8s:l{i}=v")).collect();
        let meta = flow_to_model(&f).hubble.unwrap();
        assert_eq!(
            meta.source_labels.len(),
            crate::models::flow::MAX_FLOW_LABELS
        );
    }

    #[test]
    fn udp_and_icmp_protocols() {
        let mut udp = tcp_flow("", Verdict::Forwarded, 0);
        udp.l4 = Some(Layer4 {
            protocol: Some(layer4::Protocol::Udp(Udp {
                source_port: 1,
                destination_port: 53,
            })),
        });
        let f = flow_to_model(&udp);
        assert_eq!((f.protocol.as_str(), f.port), ("UDP", 53));

        let mut icmp = tcp_flow("", Verdict::Forwarded, 0);
        icmp.l4 = Some(Layer4 {
            protocol: Some(layer4::Protocol::IcmPv4(Default::default())),
        });
        let f = flow_to_model(&icmp);
        assert_eq!((f.protocol.as_str(), f.port), ("ICMPv4", 0));

        let mut none = tcp_flow("", Verdict::Forwarded, 0);
        none.l4 = None;
        assert_eq!(flow_to_model(&none).protocol, "UNKNOWN");
    }

    #[test]
    fn out_of_range_port_and_unknown_verdict_do_not_panic() {
        let mut f = tcp_flow("u", Verdict::Forwarded, 70_000);
        f.verdict = 999;
        let m = flow_to_model(&f);
        assert_eq!(m.port, 0);
        assert_eq!(m.verdict, "UNKNOWN");
    }

    #[test]
    fn http_fields_come_from_l7() {
        let mut f = tcp_flow("u", Verdict::Forwarded, 80);
        f.l7 = Some(Layer7 {
            record: Some(layer7::Record::Http(Http {
                code: 503,
                method: "GET".into(),
                url: "http://api/x".into(),
                ..Default::default()
            })),
            ..Default::default()
        });
        let m = flow_to_model(&f);
        assert_eq!(m.http_method.as_deref(), Some("GET"));
        assert_eq!(m.http_url.as_deref(), Some("http://api/x"));
        assert_eq!(m.http_code, Some(503));
        assert!(flow_to_model(&tcp_flow("u", Verdict::Forwarded, 80))
            .http_method
            .is_none());
    }

    #[test]
    fn dns_fields_come_from_l7_not_from_drop_verdict() {
        use super::pb::flow::Dns;
        let mut f = tcp_flow("u", Verdict::Dropped, 53);
        f.l7 = Some(Layer7 {
            latency_ns: 2_500_000,
            record: Some(layer7::Record::Dns(Dns {
                query: "payments.shop.svc.cluster.local.".into(),
                ips: vec!["10.0.0.9".into()],
                rcode: 3,
                qtypes: vec!["A".into()],
                ..Default::default()
            })),
            ..Default::default()
        });
        let m = flow_to_model(&f);
        assert_eq!(
            m.dns_query.as_deref(),
            Some("payments.shop.svc.cluster.local.")
        );
        assert_eq!(m.dns_rcode, Some(3));
        assert_eq!(m.dns_rcode_name.as_deref(), Some("NXDOMAIN"));
        assert_eq!(
            m.dns_ips.as_ref().map(|v| v.as_slice()),
            Some(&["10.0.0.9".to_string()][..])
        );
        assert_eq!(m.dns_latency_ns, Some(2_500_000));
        // A drop without L7 DNS must not invent an rcode.
        let bare = flow_to_model(&tcp_flow("d", Verdict::Dropped, 53));
        assert!(bare.dns_query.is_none());
        assert!(bare.dns_rcode.is_none());
    }

    #[test]
    fn id_is_stable_without_uuid_and_matches_the_json_path() {
        let a = flow_to_model(&tcp_flow("", Verdict::Dropped, 5432));
        let b = flow_to_model(&tcp_flow("", Verdict::Dropped, 5432));
        assert_eq!(a.id, b.id);
        assert!(a.id.starts_with("h-"));
        assert_ne!(
            a.id,
            flow_to_model(&tcp_flow("", Verdict::Dropped, 5433)).id
        );

        // The same flow read via the CLI's JSON must get the same id, so switching
        // backends does not duplicate rows in the history store.
        let json = serde_json::json!({"flow": {
            "time": "2023-11-14T22:13:20.123456Z",
            "verdict": "DROPPED",
            "IP": {"source": "10.0.0.1", "destination": "10.0.0.2"},
            "l4": {"TCP": {"source_port": 40000, "destination_port": 5432}},
            "source": {"namespace": "shop", "pod_name": "web-1"},
            "destination": {"namespace": "db", "pod_name": "pg-0"}
        }});
        let via_cli = crate::services::hubble::flow_from_hubble_line(0, &json).unwrap();
        assert_eq!(via_cli.id, a.id);
    }

    #[test]
    fn non_flow_messages_are_skipped_and_envelope_time_is_the_fallback() {
        let lost = GetFlowsResponse {
            response_types: Some(ResponseTypes::LostEvents(Default::default())),
            ..Default::default()
        };
        assert!(flow_from_response(&lost).is_none());
        assert!(flow_from_response(&GetFlowsResponse::default()).is_none());

        let mut inner = tcp_flow("u", Verdict::Forwarded, 80);
        inner.time = None;
        let msg = GetFlowsResponse {
            time: ts(1_700_000_000, 0),
            response_types: Some(ResponseTypes::Flow(inner)),
            ..Default::default()
        };
        assert_eq!(
            flow_from_response(&msg).unwrap().timestamp,
            "2023-11-14T22:13:20Z"
        );
    }

    #[test]
    fn namespace_filter_matches_either_end() {
        assert!(namespace_filters(None).is_empty());
        let f = namespace_filters(Some("shop"));
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].source_pod, vec!["shop/"]);
        assert!(f[0].destination_pod.is_empty());
        assert_eq!(f[1].destination_pod, vec!["shop/"]);
        assert!(f[1].source_pod.is_empty());
    }

    #[tokio::test]
    async fn last_flows_requests_n_without_follow_and_skips_non_flows() {
        let flows = vec![
            tcp_flow("a", Verdict::Forwarded, 80),
            tcp_flow("b", Verdict::Dropped, 81),
            tcp_flow("c", Verdict::Forwarded, 82),
        ];
        let (addr, seen) = serve(flows).await;
        let got = last_flows(&addr, 2, None, None, Some("east"))
            .await
            .unwrap();
        assert_eq!(
            got.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(),
            ["b", "c"]
        );
        assert!(got.iter().all(|f| f.cluster.as_deref() == Some("east")));
        let req = seen.lock().unwrap()[0].clone();
        assert_eq!(req.number, 2);
        assert!(!req.follow);
        assert!(req.whitelist.is_empty());
    }

    #[tokio::test]
    async fn last_flows_sends_the_namespace_filter() {
        let (addr, seen) = serve(vec![tcp_flow("a", Verdict::Forwarded, 80)]).await;
        last_flows(&addr, 10, Some("shop"), None, None)
            .await
            .unwrap();
        let req = seen.lock().unwrap()[0].clone();
        assert_eq!(req.whitelist.len(), 2);
        assert_eq!(req.whitelist[0].source_pod, vec!["shop/"]);
        assert_eq!(req.whitelist[1].destination_pod, vec!["shop/"]);
    }

    #[tokio::test]
    async fn follow_requests_a_live_stream() {
        let (addr, seen) = serve(vec![tcp_flow("a", Verdict::Forwarded, 80)]).await;
        let mut stream = follow_flows(&addr, Some("shop")).await.unwrap();
        let mut n = 0;
        while let Some(msg) = stream.message().await.unwrap() {
            if flow_from_response(&msg).is_some() {
                n += 1;
            }
        }
        assert_eq!(n, 1);
        let req = seen.lock().unwrap()[0].clone();
        assert!(req.follow);
        assert_eq!(req.number, 0);
    }

    #[tokio::test]
    async fn health_reflects_server_status() {
        let (addr, _) = serve(vec![]).await;
        assert!(is_healthy(&addr).await);

        // Nothing listening: bind then drop to get a free, closed port.
        let closed = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().to_string()
        };
        assert!(!is_healthy(&closed).await);
        assert!(last_flows(&closed, 5, None, None, None).await.is_err());
    }

    #[tokio::test]
    async fn a_plain_tcp_listener_is_not_a_healthy_hubble() {
        // The old TCP-connect check would call this healthy.
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            loop {
                if let Ok((mut s, _)) = l.accept().await {
                    use tokio::io::AsyncWriteExt;
                    let _ = s.write_all(b"not grpc").await;
                }
            }
        });
        assert!(!is_healthy(&addr).await);
    }

    /// Serves a fixed set of flows on `FAKE_HUBBLE_ADDR` until killed, for running the
    /// real API binary against a Hubble without a cluster:
    /// `FAKE_HUBBLE_ADDR=127.0.0.1:4245 cargo test fake_observer_server -- --ignored`
    #[tokio::test]
    #[ignore = "long-running helper, not a test"]
    async fn fake_observer_server() {
        let addr = std::env::var("FAKE_HUBBLE_ADDR").expect("set FAKE_HUBBLE_ADDR");
        let mut drop = tcp_flow("e2e-drop", Verdict::Dropped, 5432);
        drop.uuid = String::new(); // exercises the content-derived id
        let flows = vec![
            tcp_flow("e2e-1", Verdict::Forwarded, 80),
            drop,
            tcp_flow("e2e-3", Verdict::Forwarded, 443),
        ];
        let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
        let svc = pb::observer::observer_server::ObserverServer::new(testing::Fake {
            flows,
            seen: Default::default(),
        });
        tonic::transport::Server::builder()
            .add_service(svc)
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .unwrap();
    }
}
