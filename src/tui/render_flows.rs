// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Flow and packet explanation rendering

use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use super::app::TuiApp;
use super::theme::*;

impl TuiApp {
    pub(crate) fn render_flows(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        if self.show_packet_explanation {
            self.render_packet_explanation(f, area);
            return;
        }

        // Show flows starting from the selection to keep it visible
        let visible_count = area.height.saturating_sub(2) as usize; // subtract borders
        let start = if self.selected_flow_index >= visible_count {
            self.selected_flow_index - visible_count + 1
        } else {
            0
        };
        let items: Vec<ListItem> = self
            .flows
            .iter()
            .enumerate()
            .skip(start)
            .take(visible_count.max(1))
            .map(|(idx, flow)| {
                let verdict_color = match flow.verdict.as_str() {
                    "FORWARDED" => FORWARDED_COLOR,
                    "DROPPED" => DROPPED_COLOR,
                    _ => UNKNOWN_TRAFFIC_COLOR,
                };

                let prefix = if idx == self.selected_flow_index {
                    "▶ "
                } else {
                    "  "
                };
                let content = format!(
                    "{}{} {} {}/{} -> {}/{} {}",
                    prefix,
                    flow.time,
                    flow.verdict,
                    flow.source.namespace,
                    flow.source.pod_name,
                    flow.destination.namespace,
                    flow.destination.pod_name,
                    flow.r#type,
                );

                let style = if idx == self.selected_flow_index {
                    Style::default()
                        .fg(verdict_color)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(verdict_color)
                };

                ListItem::new(Line::from(Span::styled(content, style)))
            })
            .collect();

        let title = format!(
            "Live Flows ({}) [↑/↓: Select | e: Explain]",
            self.flows.len()
        );
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
        f.render_widget(list, area);
    }

    pub(crate) fn render_packet_explanation(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        if self.flows.is_empty() || self.selected_flow_index >= self.flows.len() {
            let no_flows =
                Paragraph::new("No flow selected for explanation.\n\nPress Esc to return.")
                    .style(Style::default().fg(WARNING_COLOR))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Packet Explanation"),
                    );
            f.render_widget(no_flows, area);
            return;
        }

        let flow = &self.flows[self.selected_flow_index];

        // Parse protocol and port from the flow type field (e.g. "TCP:80", "UDP:53")
        let type_parts: Vec<&str> = flow.r#type.split(':').collect();
        let protocol = type_parts
            .first()
            .copied()
            .filter(|p| matches!(*p, "TCP" | "UDP" | "ICMP"))
            .unwrap_or("TCP");
        let port = type_parts
            .get(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);

        // Generate explanation
        let mut explainer = match self.packet_explainer.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                let busy =
                    Paragraph::new("Packet explainer is busy. Try again.\n\nPress Esc to return.")
                        .style(Style::default().fg(WARNING_COLOR))
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title("Packet Explanation"),
                        );
                f.render_widget(busy, area);
                return;
            }
        };
        let explanation = match explainer.explain_packet(
            &flow.source.namespace,
            &flow.source.pod_name,
            &flow.destination.namespace,
            &flow.destination.pod_name,
            port,
            protocol,
            &flow.verdict,
        ) {
            Ok(exp) => exp,
            Err(_) => {
                let error =
                    Paragraph::new("Failed to generate explanation.\n\nPress Esc to return.")
                        .style(Style::default().fg(ERROR_COLOR))
                        .block(Block::default().borders(Borders::ALL).title("Error"));
                f.render_widget(error, area);
                return;
            }
        };

        // Create layout
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8), // Header
                Constraint::Min(10),   // Analysis
                Constraint::Length(8), // Troubleshooting
            ])
            .split(area);

        // Header
        let header_text = format!(
            "📦 Packet Explanation\n\n\
            Source:      {}\n\
            Destination: {}\n\
            Protocol:    {}     Port: {}\n\
            Verdict:     {}     Time: {}",
            explanation.source,
            explanation.destination,
            explanation.protocol,
            port,
            explanation.verdict,
            explanation.timestamp,
        );

        let header = Paragraph::new(header_text)
            .style(Style::default().fg(INFO_COLOR))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Packet Info")
                    .border_style(Style::default().fg(BORDER_COLOR)),
            );
        f.render_widget(header, chunks[0]);

        // Analysis
        let analysis_text = format!(
            "🔍 What Happened:\n{}\n\n\
            💡 Why:\n{}\n\n\
            📋 Policy Context:\n{}\n\n\
            🔒 Security Analysis:\n{}",
            explanation.what_happened,
            explanation.why_happened,
            explanation.policy_context,
            explanation.security_analysis,
        );

        let analysis_color = if explanation.verdict == "DROPPED" {
            ERROR_COLOR
        } else {
            SUCCESS_COLOR
        };

        let analysis = Paragraph::new(analysis_text)
            .style(Style::default().fg(analysis_color))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Analysis")
                    .border_style(Style::default().fg(BORDER_COLOR)),
            )
            .wrap(ratatui::widgets::Wrap { trim: false });
        f.render_widget(analysis, chunks[1]);

        // Troubleshooting
        let tips_text = format!(
            "🔧 Troubleshooting:\n{}",
            explanation.troubleshooting_tips.join("\n")
        );

        let tips = Paragraph::new(tips_text)
            .style(Style::default().fg(TEXT_COLOR))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Tips [Esc: Exit]")
                    .border_style(Style::default().fg(BORDER_COLOR)),
            )
            .wrap(ratatui::widgets::Wrap { trim: false });
        f.render_widget(tips, chunks[2]);
    }
}
