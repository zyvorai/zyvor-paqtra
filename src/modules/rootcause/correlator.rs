// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// Policy Correlator
///
/// Correlates drop events with policies and other eBPF data
use super::*;
use crate::ebpf::{MapReader, PolicyVerdict};

pub struct PolicyCorrelator;

impl PolicyCorrelator {
    /// Correlate a drop event with policy decisions
    pub async fn correlate_with_policy<M: MapReader>(
        event: &DropEvent,
        ebpf_reader: &M,
    ) -> Result<(Option<String>, Vec<String>)> {
        let mut context = Vec::new();

        // Only correlate policy-related drops
        if !Self::is_policy_related(&event.reason) {
            return Ok((None, context));
        }

        // Read policy map
        let policy_decisions = ebpf_reader.read_policy_map()?;

        // Find related policy decisions
        let related = Self::find_related_decisions(event, &policy_decisions);

        if related.is_empty() {
            context.push("No policy decisions found for this traffic".to_string());
            return Ok((None, context));
        }

        // Analyze why policy denied
        let analysis = Self::analyze_policy_decisions(event, &related);
        context.extend(analysis.explanations);

        Ok((analysis.policy_name, context))
    }

    /// Check if drop reason is policy-related
    fn is_policy_related(reason: &DropReason) -> bool {
        matches!(
            reason,
            DropReason::PolicyDenied | DropReason::PortNotAllowed
        )
    }

    /// Find policy decisions related to this drop
    fn find_related_decisions(
        event: &DropEvent,
        decisions: &[PolicyDecision],
    ) -> Vec<PolicyDecision> {
        decisions
            .iter()
            .filter(|d| {
                d.src_identity == event.identity_src || d.dst_identity == event.identity_dst
            })
            .cloned()
            .collect()
    }

    /// Analyze policy decisions to understand the drop
    fn analyze_policy_decisions(event: &DropEvent, decisions: &[PolicyDecision]) -> PolicyAnalysis {
        let mut explanations = Vec::new();
        let mut policy_name = None;

        // Check for exact match (should be denied)
        let exact_match = decisions.iter().find(|d| {
            d.src_identity == event.identity_src
                && d.dst_identity == event.identity_dst
                && d.port == event.dst_port
                && d.protocol == event.protocol
        });

        if let Some(decision) = exact_match {
            match decision.verdict {
                PolicyVerdict::Deny => {
                    explanations.push(format!(
                        "Explicit DENY rule found: {} → {} port {}",
                        event.identity_src, event.identity_dst, event.dst_port
                    ));
                }
                PolicyVerdict::Allow => {
                    explanations.push(
                        "Policy shows ALLOW but packet was still dropped (possible conntrack issue)".to_string()
                    );
                }
                PolicyVerdict::Redirect => {
                    explanations
                        .push("Traffic should be redirected but was dropped instead".to_string());
                }
                PolicyVerdict::Audit => {
                    explanations.push(
                        "Traffic is in AUDIT mode - may be dropped due to other reasons"
                            .to_string(),
                    );
                }
            }
        } else {
            // No exact match - check for partial matches
            let src_matches: Vec<_> = decisions
                .iter()
                .filter(|d| d.src_identity == event.identity_src)
                .collect();

            let dst_matches: Vec<_> = decisions
                .iter()
                .filter(|d| d.dst_identity == event.identity_dst)
                .collect();

            if !src_matches.is_empty() {
                explanations.push(format!(
                    "Found {} policy rules for source identity {}, but none match destination {} port {}",
                    src_matches.len(),
                    event.identity_src,
                    event.identity_dst,
                    event.dst_port
                ));

                // Show what ports ARE allowed
                let allowed_ports: Vec<u16> = src_matches
                    .iter()
                    .filter(|d| d.verdict == PolicyVerdict::Allow)
                    .map(|d| d.port)
                    .collect();

                if !allowed_ports.is_empty() {
                    explanations.push(format!(
                        "Allowed ports for this source: {:?}",
                        allowed_ports
                    ));
                }
            }

            if !dst_matches.is_empty() {
                explanations.push(format!(
                    "Found {} policy rules for destination identity {}, but none match source {}",
                    dst_matches.len(),
                    event.identity_dst,
                    event.identity_src
                ));
            }

            if src_matches.is_empty() && dst_matches.is_empty() {
                explanations.push(format!(
                    "No policy rules found for identities {} or {}",
                    event.identity_src, event.identity_dst
                ));
            }
        }

        // Check for default-deny policy
        if Self::has_default_deny(decisions, event.identity_src) {
            explanations.push(
                "Default-deny policy is active (no egress allowed except explicitly permitted)"
                    .to_string(),
            );
            policy_name = Some("default-deny".to_string());
        }

        PolicyAnalysis {
            policy_name,
            explanations,
        }
    }

    /// Check if default-deny is in effect
    fn has_default_deny(decisions: &[PolicyDecision], identity: u32) -> bool {
        // If we have decisions for this identity but no wildcard ALLOW, it's default-deny
        let has_rules = decisions.iter().any(|d| d.src_identity == identity);
        let has_allow_all = decisions.iter().any(|d| {
            d.src_identity == identity
                && d.dst_identity == 0 // wildcard
                && d.verdict == PolicyVerdict::Allow
        });

        has_rules && !has_allow_all
    }

    /// Correlate with connection tracking
    pub async fn correlate_with_conntrack<M: MapReader>(
        event: &DropEvent,
        ebpf_reader: &M,
    ) -> Result<Vec<String>> {
        let mut context = Vec::new();

        // Only for CT-related drops
        if event.reason != DropReason::CTStateMismatch {
            return Ok(context);
        }

        // Read conntrack map
        let conntrack_entries = ebpf_reader.read_conntrack_map()?;

        // Find related entries
        let src_ip_str = event.src_ip.to_string();
        let dst_ip_str = event.dst_ip.to_string();

        let related: Vec<_> = conntrack_entries
            .iter()
            .filter(|e| {
                (e.src_ip == src_ip_str && e.dst_ip == dst_ip_str)
                    || (e.src_ip == dst_ip_str && e.dst_ip == src_ip_str)
            })
            .collect();

        if related.is_empty() {
            context.push("No connection tracking entry found for this flow".to_string());
            context.push(
                "This suggests a new connection that doesn't match existing state".to_string(),
            );
        } else {
            context.push(format!("Found {} related conntrack entries", related.len()));

            for entry in related {
                context.push(format!(
                    "  Entry: {}:{} → {}:{} (state: established)",
                    entry.src_ip, entry.src_port, entry.dst_ip, entry.dst_port
                ));
            }
        }

        Ok(context)
    }

    /// Correlate with load balancer state
    pub async fn correlate_with_lb<M: MapReader>(
        event: &DropEvent,
        ebpf_reader: &M,
    ) -> Result<Vec<String>> {
        let mut context = Vec::new();

        // Only for LB-related drops
        if !matches!(
            event.reason,
            DropReason::LBError | DropReason::NoBackend | DropReason::ServiceNotFound
        ) {
            return Ok(context);
        }

        // Read LB map
        let lb_entries = ebpf_reader.read_lb_map()?;

        // Find service entry
        let dst_ip_str = event.dst_ip.to_string();
        let service = lb_entries.iter().find(|e| {
            e.service_port == event.dst_port
                && (e.service_ip == dst_ip_str || e.service_ip == "0.0.0.0")
        });

        if let Some(svc) = service {
            context.push(format!(
                "Load balancer entry found for {}:{}",
                svc.service_ip, svc.service_port
            ));

            context.push(format!(
                "Backend: {}:{} (weight: {}, active_conns: {})",
                svc.backend_ip, svc.backend_port, svc.weight, svc.active_conns
            ));

            // Count total backends for this service
            let backend_count = lb_entries
                .iter()
                .filter(|e| e.service_port == event.dst_port)
                .count();

            if backend_count == 0 {
                context.push("Service has zero backends configured".to_string());
            } else {
                context.push(format!("Total backends for service: {}", backend_count));
            }
        } else {
            context.push(format!(
                "No load balancer entry found for {}:{}",
                event.dst_ip, event.dst_port
            ));
        }

        Ok(context)
    }
}

struct PolicyAnalysis {
    policy_name: Option<String>,
    explanations: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ebpf::MockMapReader;
    use std::net::{IpAddr, Ipv4Addr};

    #[tokio::test]
    async fn test_correlate_with_policy() {
        let event = DropEvent {
            timestamp: 1234567890,
            src_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            src_port: 12345,
            dst_port: 80,
            protocol: 6,
            reason: DropReason::PolicyDenied,
            identity_src: 100,
            identity_dst: 200,
            namespace: None,
            pod_name: None,
        };

        let reader = MockMapReader;
        let (_policy, context) = PolicyCorrelator::correlate_with_policy(&event, &reader)
            .await
            .unwrap();

        // MockMapReader returns empty data
        assert!(!context.is_empty());
    }

    #[test]
    fn test_is_policy_related() {
        assert!(PolicyCorrelator::is_policy_related(
            &DropReason::PolicyDenied
        ));
        assert!(PolicyCorrelator::is_policy_related(
            &DropReason::PortNotAllowed
        ));
        assert!(!PolicyCorrelator::is_policy_related(
            &DropReason::FragNeeded
        ));
    }
}
