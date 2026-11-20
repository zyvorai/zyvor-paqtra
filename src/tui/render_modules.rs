// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Intelligence module rendering (healer, policy detail)

use ratatui::{
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::app::{ModuleContainer, TuiApp};
use super::theme::*;

impl TuiApp {
    pub(crate) fn render_healer(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let stats = self.modules.healer_stats();
        let data_mode = if self.modules.is_enriched() {
            "Enriched (Pod-Aware)"
        } else {
            "Mock Data"
        };

        // Get actual problems and fixes
        let (problems_list, fixes_list) = match &self.modules {
            ModuleContainer::Enriched { healer, .. } => (healer.problems(), healer.fixes()),
            ModuleContainer::Mock { healer, .. } => (healer.problems(), healer.fixes()),
        };

        // Last run info
        let elapsed = self.last_healer_run.elapsed().as_secs();
        let next_run = 30_u64.saturating_sub(elapsed);

        // Build problems display
        let mut problem_text = String::new();
        for (idx, problem) in problems_list.iter().take(3).enumerate() {
            problem_text.push_str(&format!("\n{}. {}", idx + 1, Self::format_problem(problem)));
        }
        if problem_text.is_empty() {
            problem_text = "\n  No problems detected ✓".to_string();
        }

        // Build fixes display
        let mut fixes_text = String::new();
        for (idx, fix) in fixes_list.iter().take(3).enumerate() {
            let status = if fix.applied { "✅" } else { "📝" };
            fixes_text.push_str(&format!(
                "\n{}. {} {}",
                idx + 1,
                status,
                Self::format_fix_action(&fix.action)
            ));
        }
        if fixes_text.is_empty() {
            fixes_text = "\n  No fixes proposed".to_string();
        }

        let healer_status = format!(
            "🏥 Self-Healer Status\n\n\
            Mode:               Active ({})\n\
            Problems Detected:  {}\n\
            Fixes Proposed:     {}\n\
            Fixes Applied:      {}\n\
            Last Check:         {}s ago (next in {}s)\n\
            Recent Problems:{}\n\
            Proposed Fixes:{}",
            data_mode,
            stats.problems_detected,
            stats.fixes_proposed,
            stats.fixes_applied,
            elapsed,
            next_run,
            problem_text,
            fixes_text,
        );

        let content = Paragraph::new(healer_status)
            .style(Style::default().fg(SUCCESS_COLOR))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("🏥 Self-Healer (Auto-Scan: 30s | Press 'd' for manual)")
                    .border_style(Style::default().fg(BORDER_COLOR)),
            );
        f.render_widget(content, area);
    }

    pub(crate) fn format_problem(problem: &crate::modules::healer::Problem) -> String {
        use crate::modules::healer::Problem;
        match problem {
            Problem::DNSDrops {
                namespace,
                pod,
                count,
            } => {
                format!("DNS drops: {}/{} ({} drops)", namespace, pod, count)
            }
            Problem::MTUMismatch {
                namespace,
                pod,
                expected,
                actual,
            } => {
                format!(
                    "MTU mismatch: {}/{} (expected {}, got {})",
                    namespace, pod, expected, actual
                )
            }
            Problem::PolicyGap {
                src_namespace,
                src_pod,
                dst_namespace,
                dst_pod,
                port,
                protocol,
            } => {
                format!(
                    "Policy gap: {}/{} → {}/{}:{} {}",
                    src_namespace, src_pod, dst_namespace, dst_pod, port, protocol
                )
            }
            Problem::LoadBalancerTimeout {
                service,
                backend,
                timeout_count,
            } => {
                format!(
                    "LB timeout: {} → {} ({} timeouts)",
                    service, backend, timeout_count
                )
            }
            Problem::ConntrackFull { node, utilization } => {
                format!(
                    "Conntrack full: {} ({:.1}% util)",
                    node,
                    utilization * 100.0
                )
            }
        }
    }

    pub(crate) fn format_fix_action(action: &crate::modules::healer::FixAction) -> String {
        use crate::modules::healer::FixAction;
        match action {
            FixAction::CreateDNSPolicy { namespace } => {
                format!("Create DNS policy for '{}'", namespace)
            }
            FixAction::AdjustMTU {
                namespace,
                pod,
                new_mtu,
            } => {
                format!("Adjust MTU for {}/{} to {}", namespace, pod, new_mtu)
            }
            FixAction::CreateAllowPolicy { src, dst, port } => {
                format!("Allow {} → {}:{}", src, dst, port)
            }
            FixAction::RebalanceBackend { service, backend } => {
                format!("Rebalance {} → {}", service, backend)
            }
            FixAction::TuneConntrack { node, new_timeout } => {
                format!("Tune conntrack on {} (timeout: {})", node, new_timeout)
            }
        }
    }

    pub(crate) fn render_policy_detail(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let policies = match &self.modules {
            ModuleContainer::Enriched { autopolicy, .. } => autopolicy.policies(),
            ModuleContainer::Mock { autopolicy, .. } => autopolicy.policies(),
        };

        if policies.is_empty() {
            let no_policies = Paragraph::new(
                "No policies generated yet.\n\n\
                Press 'g' to generate policies from learned patterns.\n\
                Press 'Esc' to return to main view.",
            )
            .style(Style::default().fg(Color::Yellow))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Policy Detail"),
            );
            f.render_widget(no_policies, area);
            return;
        }

        let clamped_index = if self.selected_policy_index >= policies.len() {
            policies.len() - 1
        } else {
            self.selected_policy_index
        };
        let selected_policy = &policies[clamped_index];
        let is_applied = self.applied_policies.contains(&selected_policy.name);

        // Create policy detail content
        let status_indicator = if is_applied {
            "Status:     ✅ APPLIED"
        } else {
            "Status:     📋 Not Applied"
        };

        // Get ML-based confidence information
        use crate::modules::autopolicy::confidence::ConfidenceScorer;
        let confidence_level = ConfidenceScorer::confidence_level(selected_policy.confidence);
        let recommendation = ConfidenceScorer::get_recommendation(selected_policy.confidence);

        let detail_text = format!(
            "📄 Policy {} of {} (ML-Enhanced Zero-Trust)\n\n\
            Name:          {}\n\
            Namespace:     {}\n\
            \n\
            🤖 ML Confidence Analysis:\n\
            Score:         {:.1}% ({})\n\
            Assessment:    {}\n\
            Patterns:      {} observed traffic patterns\n\
            \n\
            {}\n\
            File:          ./policies/{}.yaml\n\
            \n\
            ─────────────────────────────────────────────────────────\n\
            YAML Content:\n\
            ─────────────────────────────────────────────────────────\n\
            {}\n\
            ─────────────────────────────────────────────────────────\n\n\
            {}",
            clamped_index + 1,
            policies.len(),
            selected_policy.name,
            selected_policy.namespace,
            selected_policy.confidence * 100.0,
            confidence_level,
            recommendation,
            selected_policy.patterns.len(),
            status_indicator,
            selected_policy.name,
            selected_policy.yaml,
            if is_applied {
                "Press ↑/↓ to navigate | r: Rollback Policy | Esc to exit"
            } else {
                "Press ↑/↓ to navigate | a: Apply Policy | Esc to exit"
            }
        );

        let policy_detail = Paragraph::new(detail_text)
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("Policy Detail: {}", selected_policy.name))
                    .style(Style::default().fg(Color::Cyan)),
            );

        f.render_widget(policy_detail, area);
    }
}
