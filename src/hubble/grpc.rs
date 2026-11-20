// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use anyhow::Result;
use tonic::transport::Channel;

// Include the generated protobuf code
pub mod flow {
    tonic::include_proto!("flow");
}

use flow::{observer_client::ObserverClient, GetFlowsRequest, Verdict};

pub struct GrpcHubbleClient {
    client: ObserverClient<Channel>,
}

impl GrpcHubbleClient {
    pub async fn connect(addr: String) -> Result<Self> {
        let client = ObserverClient::connect(addr).await?;
        Ok(Self { client })
    }

    pub async fn get_flows(&mut self, limit: u64) -> Result<Vec<super::Flow>> {
        let request = tonic::Request::new(GetFlowsRequest {
            number: limit,
            follow: false,
            blacklist: vec![],
            whitelist: vec![],
        });

        let mut stream = self.client.get_flows(request).await?.into_inner();
        let mut flows = Vec::new();

        while let Some(response) = stream.message().await? {
            if let Some(flow) = response.flow {
                flows.push(convert_flow(flow));
            }
        }

        Ok(flows)
    }

    pub async fn server_status(&mut self) -> Result<ServerStatus> {
        let request = tonic::Request::new(flow::ServerStatusRequest {});
        let response = self.client.server_status(request).await?.into_inner();

        Ok(ServerStatus {
            num_flows: response.num_flows,
            max_flows: response.max_flows,
            seen_flows: response.seen_flows,
            uptime_ns: response.uptime_ns,
        })
    }
}

pub struct ServerStatus {
    pub num_flows: u64,
    pub max_flows: u64,
    pub seen_flows: u64,
    pub uptime_ns: u64,
}

fn convert_flow(flow: flow::Flow) -> super::Flow {
    let time = if let Some(ts) = flow.time {
        format!("{}.{:09}", ts.seconds, ts.nanos)
    } else {
        String::new()
    };

    let verdict = match flow.verdict() {
        Verdict::Forwarded => "FORWARDED",
        Verdict::Dropped => "DROPPED",
        Verdict::Error => "ERROR",
        Verdict::Audit => "AUDIT",
        Verdict::Redirected => "REDIRECTED",
        Verdict::Traced => "TRACED",
        _ => "UNKNOWN",
    }
    .to_string();

    let source = if let Some(src) = flow.source {
        super::FlowEndpoint {
            namespace: src.namespace,
            pod_name: src.pod_name.first().cloned().unwrap_or_default(),
            ip: String::new(),
        }
    } else {
        super::FlowEndpoint::default()
    };

    let destination = if let Some(dst) = flow.destination {
        super::FlowEndpoint {
            namespace: dst.namespace,
            pod_name: dst.pod_name.first().cloned().unwrap_or_default(),
            ip: String::new(),
        }
    } else {
        super::FlowEndpoint::default()
    };

    super::Flow {
        time,
        verdict,
        source,
        destination,
        r#type: flow.summary,
    }
}
