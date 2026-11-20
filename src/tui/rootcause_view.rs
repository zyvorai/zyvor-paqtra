// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// RootCause View - Drop Analysis & Troubleshooting
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
    Frame,
};

use super::theme::*;
use crate::ebpf::MapReader;
use crate::modules::rootcause::{DropReason, RootCauseEngine};

pub struct RootCauseView;

impl RootCauseView {
    pub fn new() -> Self {
        Self
    }

    pub fn render<M: MapReader>(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        rootcause: Option<&RootCauseEngine<M>>,
        selected_fix_index: usize,
        fix_apply_confirmation: bool,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8),  // Header
                Constraint::Min(10),    // Drop Analysis
                Constraint::Length(12), // Fixes
            ])
            .split(area);

        // Header
        self.render_header(f, chunks[0], rootcause);

        // Drop Analysis
        self.render_drops(f, chunks[1], rootcause);

        // Recommended Fixes
        self.render_fixes(
            f,
            chunks[2],
            rootcause,
            selected_fix_index,
            fix_apply_confirmation,
        );
    }

    fn render_header<M: MapReader>(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        rootcause: Option<&RootCauseEngine<M>>,
    ) {
        let stats = if let Some(engine) = rootcause {
            let s = engine.get_stats();
            format!(
                "🔍 Root-Cause Analysis Engine\n\n\
                Status:            Active\n\
                Drops Analyzed:    {}\n\
                Patterns:          {}\n\
                Top Reason:        {}",
                s.total_drops,
                s.top_patterns.len(),
                s.top_patterns
                    .first()
                    .map(|(reason, count)| format!("{:?} ({})", reason, count))
                    .unwrap_or_else(|| "None".to_string()),
            )
        } else {
            "🔍 Root-Cause Analysis Engine\n\n\
            Status:            Not initialized"
                .to_string()
        };

        let content = Paragraph::new(stats)
            .style(Style::default().fg(ERROR_COLOR))
            .block(bordered_block("Analysis Status"));

        f.render_widget(content, area);
    }

    fn render_drops<M: MapReader>(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        rootcause: Option<&RootCauseEngine<M>>,
    ) {
        let items: Vec<ListItem> = if let Some(engine) = rootcause {
            let stats = engine.get_stats();
            if stats.top_patterns.is_empty() {
                vec![ListItem::new(Line::from(Span::styled(
                    "No drops detected",
                    Style::default().fg(SUCCESS_COLOR),
                )))]
            } else {
                stats
                    .top_patterns
                    .iter()
                    .take(5)
                    .map(|(pattern, count)| {
                        let severity = Self::severity_for_reason(&pattern.reason);
                        let color = severity_color(severity);
                        let flow = format!(
                            "identity:{}→identity:{}:{}",
                            pattern.src_identity, pattern.dst_identity, pattern.dst_port
                        );
                        let content =
                            format!("{} {} [{severity}] {count} drops", pattern.reason, flow,);
                        ListItem::new(Line::from(Span::styled(
                            content,
                            Style::default().fg(color).add_modifier(Modifier::BOLD),
                        )))
                    })
                    .collect()
            }
        } else {
            vec![ListItem::new(Line::from(Span::styled(
                "No drops detected",
                Style::default().fg(SUCCESS_COLOR),
            )))]
        };

        let list = List::new(items).block(bordered_block("Recent Packet Drops (Top 5)"));

        f.render_widget(list, area);
    }

    /// Map a drop reason to a severity label for display.
    fn severity_for_reason(reason: &DropReason) -> &'static str {
        match reason {
            DropReason::PolicyDenied | DropReason::PortNotAllowed => "Critical",
            DropReason::NoBackend
            | DropReason::ServiceNotFound
            | DropReason::CTStateMismatch
            | DropReason::FragNeeded
            | DropReason::LBError => "Medium",
            _ => "Low",
        }
    }

    fn render_fixes<M: MapReader>(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        rootcause: Option<&RootCauseEngine<M>>,
        selected_fix_index: usize,
        fix_apply_confirmation: bool,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area);

        // Build fix suggestions from real data
        let fix_entries: Vec<(String, String)> = if let Some(engine) = rootcause {
            let stats = engine.get_stats();
            if stats.top_patterns.is_empty() {
                Vec::new()
            } else {
                stats
                    .top_patterns
                    .iter()
                    .take(5)
                    .map(|(pattern, count)| {
                        let (name, detail) = Self::fix_for_pattern(pattern, *count);
                        (name, detail)
                    })
                    .collect()
            }
        } else {
            Vec::new()
        };

        // Render the list panel
        let items: Vec<ListItem> = if fix_entries.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "  No fixes needed",
                Style::default().fg(SUCCESS_COLOR),
            )))]
        } else {
            fix_entries
                .iter()
                .enumerate()
                .map(|(idx, (name, _))| {
                    let style = if idx == selected_fix_index {
                        selected_style()
                    } else {
                        Style::default().fg(SUCCESS_COLOR)
                    };

                    let prefix = if idx == selected_fix_index {
                        "▶ "
                    } else {
                        "  "
                    };
                    let content = format!("{}{}. {}", prefix, idx + 1, name);

                    ListItem::new(Line::from(Span::styled(content, style)))
                })
                .collect()
        };

        let fixes_title = if fix_apply_confirmation {
            "Fixes [CONFIRM: y/n]"
        } else {
            "Fixes [↑/↓: Select | a: Apply]"
        };

        let fixes = List::new(items).block(bordered_block(fixes_title));
        f.render_widget(fixes, chunks[0]);

        // Fix Details panel
        let details_text = if fix_entries.is_empty() {
            "No fixes needed — no drop patterns detected.".to_string()
        } else if let Some((_, detail)) = fix_entries.get(selected_fix_index) {
            detail.clone()
        } else {
            "Select a fix to see details".to_string()
        };

        let details = Paragraph::new(details_text)
            .style(Style::default().fg(INFO_COLOR))
            .block(bordered_block("Policy Details"));

        f.render_widget(details, chunks[1]);
    }

    /// Generate a fix name and detail description for a given drop pattern.
    fn fix_for_pattern(
        pattern: &crate::modules::rootcause::DropPattern,
        count: u64,
    ) -> (String, String) {
        let proto = match pattern.protocol {
            6 => "TCP",
            17 => "UDP",
            _ => "IP",
        };

        match &pattern.reason {
            DropReason::PolicyDenied | DropReason::PortNotAllowed => {
                // DNS-specific suggestion for port 53
                if pattern.dst_port == 53 {
                    let name = "Enable DNS egress".to_string();
                    let detail = format!(
                        "Policy: rootcause-fix-dns-egress\n\n\
                        Enables DNS resolution (port 53 {proto})\n\
                        from identity:{src} to identity:{dst}\n\n\
                        Allow egress to kube-dns on port 53 {proto}\n\
                        to resolve DNS-related drops.\n\n\
                        Affected drops: {count}",
                        src = pattern.src_identity,
                        dst = pattern.dst_identity,
                    );
                    (name, detail)
                } else {
                    let name = format!("Allow port {} {proto}", pattern.dst_port,);
                    let detail = format!(
                        "Policy: rootcause-fix-allow-{port}\n\n\
                        Allows identity:{src} -> identity:{dst}:{port} {proto}\n\n\
                        This policy permits traffic on port {port}\n\
                        resolving the detected policy deny.\n\n\
                        Affected drops: {count}",
                        port = pattern.dst_port,
                        src = pattern.src_identity,
                        dst = pattern.dst_identity,
                    );
                    (name, detail)
                }
            }
            DropReason::FragNeeded => {
                let name = "Adjust MTU settings".to_string();
                let detail = format!(
                    "Note: MTU Adjustment\n\n\
                    Fragment-needed drops detected on\n\
                    identity:{src} -> identity:{dst}:{port}\n\n\
                    Consider adjusting CNI MTU settings\n\
                    in cilium-config ConfigMap or\n\
                    disabling tunneling protocol.\n\n\
                    Affected drops: {count}",
                    src = pattern.src_identity,
                    dst = pattern.dst_identity,
                    port = pattern.dst_port,
                );
                (name, detail)
            }
            DropReason::CTStateMismatch => {
                let name = "Check conntrack state".to_string();
                let detail = format!(
                    "Note: Connection Tracking\n\n\
                    CT state mismatch on\n\
                    identity:{src} -> identity:{dst}:{port}\n\n\
                    Run: cilium bpf ct list global\n\
                    Run: cilium bpf ct flush\n\n\
                    Affected drops: {count}",
                    src = pattern.src_identity,
                    dst = pattern.dst_identity,
                    port = pattern.dst_port,
                );
                (name, detail)
            }
            DropReason::NoBackend | DropReason::ServiceNotFound | DropReason::LBError => {
                let name = format!("Fix service on port {}", pattern.dst_port);
                let detail = format!(
                    "Note: Service / LB Issue\n\n\
                    {reason} on port {port}\n\
                    identity:{src} -> identity:{dst}\n\n\
                    Verify service endpoints are healthy\n\
                    and the service is registered.\n\n\
                    Affected drops: {count}",
                    reason = pattern.reason,
                    port = pattern.dst_port,
                    src = pattern.src_identity,
                    dst = pattern.dst_identity,
                );
                (name, detail)
            }
            other => {
                let name = format!("Investigate: {other}");
                let detail = format!(
                    "Note: Manual Investigation\n\n\
                    {other} drops on\n\
                    identity:{src} -> identity:{dst}:{port} {proto}\n\n\
                    Check cilium monitor output\n\
                    Review Hubble flows\n\
                    Inspect eBPF maps directly\n\n\
                    Affected drops: {count}",
                    src = pattern.src_identity,
                    dst = pattern.dst_identity,
                    port = pattern.dst_port,
                );
                (name, detail)
            }
        }
    }
}
