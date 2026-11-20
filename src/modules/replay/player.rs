// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// Replay Player
///
/// Replays recorded flows in a target environment
use super::*;
use anyhow::Result;
use std::time::Duration;
use tokio::time::sleep;

pub struct ReplayPlayer<'a, M: MapReader> {
    replay_rate: f32,
    ebpf_reader: &'a M,
    k8s_client: &'a K8sClient,
}

/// Outcome of replaying a single flow
#[derive(Debug, Clone)]
pub struct ReplayOutcome {
    pub flow: RecordedFlow,
    pub success: bool,
    pub verdict: PolicyVerdict,
    pub latency_ms: Option<f64>,
    pub error: Option<String>,
}

impl<'a, M: MapReader> ReplayPlayer<'a, M> {
    pub fn new(replay_rate: f32, ebpf_reader: &'a M, k8s_client: &'a K8sClient) -> Self {
        Self {
            replay_rate,
            ebpf_reader,
            k8s_client,
        }
    }

    /// Replay a list of flows
    pub async fn replay(&self, flows: &[RecordedFlow]) -> Result<Vec<ReplayOutcome>> {
        let mut outcomes = Vec::new();

        println!(
            "▶️  Replaying {} flows at {}x speed",
            flows.len(),
            self.replay_rate
        );

        for (idx, flow) in flows.iter().enumerate() {
            if idx > 0 && idx % 100 == 0 {
                println!("   Progress: {}/{}", idx, flows.len());
            }

            let outcome = self.replay_flow(flow).await;
            outcomes.push(outcome);

            // Respect timing if replay_rate is not 0
            if self.replay_rate > 0.0 && idx < flows.len() - 1 {
                let next_flow = &flows[idx + 1];
                let delay_ms = next_flow.offset_ms.saturating_sub(flow.offset_ms);

                if delay_ms > 0 {
                    let adjusted_delay = (delay_ms as f32 / self.replay_rate) as u64;
                    sleep(Duration::from_millis(adjusted_delay)).await;
                }
            }
        }

        println!(
            "✅ Replay complete: {}/{} successful",
            outcomes.iter().filter(|o| o.success).count(),
            outcomes.len()
        );

        Ok(outcomes)
    }

    /// Replay a single flow
    async fn replay_flow(&self, flow: &RecordedFlow) -> ReplayOutcome {
        let start = std::time::Instant::now();

        // Simulate the flow by checking current policy
        let verdict = self.check_policy(flow).await;

        let latency_ms = start.elapsed().as_micros() as f64 / 1000.0;

        // Determine success
        let success = verdict == flow.verdict;

        let error = if !success {
            Some(format!(
                "Verdict mismatch: expected {:?}, got {:?}",
                flow.verdict, verdict
            ))
        } else {
            None
        };

        ReplayOutcome {
            flow: flow.clone(),
            success,
            verdict,
            latency_ms: Some(latency_ms),
            error,
        }
    }

    /// Check current policy for this flow
    async fn check_policy(&self, flow: &RecordedFlow) -> PolicyVerdict {
        // Read current policy decisions
        let policies = match self.ebpf_reader.read_policy_map() {
            Ok(p) => p,
            Err(_) => return PolicyVerdict::Deny,
        };

        // Find matching policy
        for policy in &policies {
            if policy.src_identity == flow.src_identity
                && policy.dst_identity == flow.dst_identity
                && policy.port == flow.dst_port
                && policy.protocol == flow.protocol
            {
                return policy.verdict;
            }
        }

        // Default deny
        PolicyVerdict::Deny
    }

    /// Replay with validation
    pub async fn replay_with_validation(&self, flows: &[RecordedFlow]) -> Result<ReplayValidation> {
        let outcomes = self.replay(flows).await?;

        let total = outcomes.len();
        let successful = outcomes.iter().filter(|o| o.success).count();
        let failed = total - successful;

        let verdict_matches = outcomes
            .iter()
            .filter(|o| o.verdict == o.flow.verdict)
            .count();

        let new_drops = outcomes
            .iter()
            .filter(|o| o.flow.verdict == PolicyVerdict::Allow && o.verdict == PolicyVerdict::Deny)
            .count();

        let fixed_flows = outcomes
            .iter()
            .filter(|o| o.flow.verdict == PolicyVerdict::Deny && o.verdict == PolicyVerdict::Allow)
            .count();

        Ok(ReplayValidation {
            total,
            successful,
            failed,
            verdict_matches,
            new_drops,
            fixed_flows,
            success_rate: if total > 0 {
                successful as f32 / total as f32
            } else {
                1.0
            },
        })
    }

    /// Replay in batches
    pub async fn replay_batched(
        &self,
        flows: &[RecordedFlow],
        batch_size: usize,
    ) -> Result<Vec<ReplayOutcome>> {
        let mut all_outcomes = Vec::new();

        for (batch_idx, chunk) in flows.chunks(batch_size).enumerate() {
            println!(
                "📦 Batch {}/{}",
                batch_idx + 1,
                flows.len().div_ceil(batch_size)
            );

            let outcomes = self.replay(chunk).await?;
            all_outcomes.extend(outcomes);

            // Brief pause between batches
            sleep(Duration::from_millis(100)).await;
        }

        Ok(all_outcomes)
    }

    /// Replay specific flows by filter
    pub async fn replay_filtered(
        &self,
        flows: &[RecordedFlow],
        predicate: impl Fn(&RecordedFlow) -> bool,
    ) -> Result<Vec<ReplayOutcome>> {
        let filtered: Vec<_> = flows.iter().filter(|f| predicate(f)).cloned().collect();

        println!(
            "🔍 Replaying {} filtered flows (from {})",
            filtered.len(),
            flows.len()
        );

        self.replay(&filtered).await
    }
}

#[derive(Debug, Clone)]
pub struct ReplayValidation {
    pub total: usize,
    pub successful: usize,
    pub failed: usize,
    pub verdict_matches: usize,
    pub new_drops: usize,
    pub fixed_flows: usize,
    pub success_rate: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ebpf::MockMapReader;
    use std::net::Ipv4Addr;

    #[ignore = "needs a Kubernetes cluster"]
    #[tokio::test]
    async fn test_replay_player_creation() {
        let reader = MockMapReader;
        let k8s_client = K8sClient::new().await.unwrap();

        let player = ReplayPlayer::new(1.0, &reader, &k8s_client);
        assert_eq!(player.replay_rate, 1.0);
    }

    #[ignore = "needs a Kubernetes cluster"]
    #[tokio::test]
    async fn test_replay_empty() {
        let reader = MockMapReader;
        let k8s_client = K8sClient::new().await.unwrap();

        let player = ReplayPlayer::new(1.0, &reader, &k8s_client);
        let outcomes = player.replay(&[]).await.unwrap();

        assert_eq!(outcomes.len(), 0);
    }

    #[ignore = "needs a Kubernetes cluster"]
    #[tokio::test]
    async fn test_replay_single_flow() {
        let reader = MockMapReader;
        let k8s_client = K8sClient::new().await.unwrap();

        let player = ReplayPlayer::new(10.0, &reader, &k8s_client); // Fast replay

        let flow = RecordedFlow {
            timestamp: 1000,
            offset_ms: 0,
            src_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            src_port: 12345,
            dst_port: 80,
            protocol: 6,
            src_identity: 100,
            dst_identity: 200,
            src_namespace: "default".to_string(),
            dst_namespace: "default".to_string(),
            src_labels: HashMap::new(),
            dst_labels: HashMap::new(),
            verdict: PolicyVerdict::Allow,
            bytes: 1024,
            packets: 10,
            http_method: None,
            http_path: None,
            http_status: None,
        };

        let outcomes = player.replay(&[flow]).await.unwrap();
        assert_eq!(outcomes.len(), 1);
    }
}
