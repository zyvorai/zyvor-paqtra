// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Canary Deployment View - Sidecarless Progressive Traffic Shifting
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{Gauge, List, ListItem, Paragraph},
    Frame,
};

use super::theme::*;
use crate::modules::canary::{CanaryEngine, CanaryState};

pub struct CanaryView {
    pub selected_canary_index: usize,
    pub show_details: bool,
    pub confirmation_mode: ConfirmationType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmationType {
    None,
    Promote,
    Rollback,
    #[allow(dead_code)]
    Pause,
}

impl CanaryView {
    pub fn new() -> Self {
        Self {
            selected_canary_index: 0,
            show_details: false,
            confirmation_mode: ConfirmationType::None,
        }
    }

    pub fn move_selection_up(&mut self) {
        move_selection_up(&mut self.selected_canary_index);
    }

    pub fn move_selection_down(&mut self, max: usize) {
        move_selection_down(&mut self.selected_canary_index, max);
    }

    pub fn toggle_details(&mut self) {
        self.show_details = !self.show_details;
    }

    pub fn trigger_confirmation(&mut self, confirmation_type: ConfirmationType) {
        self.confirmation_mode = confirmation_type;
    }

    pub fn cancel_confirmation(&mut self) {
        self.confirmation_mode = ConfirmationType::None;
    }

    pub fn render(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        canary: Option<&CanaryEngine>,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8),  // Header
                Constraint::Min(10),    // Canary List
                Constraint::Length(10), // Traffic Split Visualization
            ])
            .split(area);

        // Header
        self.render_header(f, chunks[0], canary);

        // Canary List
        self.render_canary_list(f, chunks[1], canary);

        // Traffic Split
        self.render_traffic_split(f, chunks[2], canary);
    }

    fn render_header(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        canary: Option<&CanaryEngine>,
    ) {
        let (active, total, promotions, rollbacks) = if let Some(engine) = canary {
            let stats = engine.stats();
            (
                stats.active_canaries,
                stats.total_deployments,
                stats.successful_promotions,
                stats.rollbacks,
            )
        } else {
            (0, 0, 0, 0)
        };

        let success_rate = if total > 0 {
            (promotions as f64 / total as f64 * 100.0) as u32
        } else {
            0
        };

        let header_text = format!(
            "  Sidecarless Canary Deployments\n\n\
            Status:            {}\n\
            Active Canaries:   {}\n\
            Total Deployed:    {}\n\
            Success Rate:      {}%\n\
            Rollbacks:         {}\n\n\
            Progressive traffic shifting without sidecar overhead",
            if active > 0 { "Active" } else { "Idle" },
            active,
            total,
            success_rate,
            rollbacks,
        );

        let header = Paragraph::new(header_text)
            .style(Style::default().fg(INFO_COLOR))
            .block(bordered_block("Canary Status"));

        f.render_widget(header, area);
    }

    fn render_canary_list(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        canary: Option<&CanaryEngine>,
    ) {
        let items: Vec<ListItem> = if let Some(engine) = canary {
            engine
                .active_canaries()
                .iter()
                .enumerate()
                .map(|(idx, c)| {
                    let is_selected = idx == self.selected_canary_index;
                    let state_str = match &c.state {
                        CanaryState::Created => "Created",
                        CanaryState::Running => "Running",
                        CanaryState::Paused => "Paused",
                        CanaryState::Promoting => "Promoting",
                        CanaryState::Promoted => "Promoted",
                        CanaryState::RollingBack => "Rolling Back",
                        CanaryState::RolledBack => "Rolled Back",
                        CanaryState::Failed { .. } => "Failed",
                    };
                    let color = match &c.state {
                        CanaryState::Running => SUCCESS_COLOR,
                        CanaryState::Paused => WARNING_COLOR,
                        CanaryState::Failed { .. } | CanaryState::RolledBack => ERROR_COLOR,
                        CanaryState::Promoted => SUCCESS_COLOR,
                        _ => INFO_COLOR,
                    };
                    let style = if is_selected {
                        selected_style()
                    } else {
                        Style::default().fg(color)
                    };

                    let prefix = if is_selected { "> " } else { "  " };
                    let content = format!(
                        "{}{:<20} ns:{:<12} [{:<8}] Traffic: {}%",
                        prefix, c.name, c.namespace, state_str, c.current_split.canary_pct
                    );
                    ListItem::new(Line::from(Span::styled(content, style)))
                })
                .collect()
        } else {
            vec![]
        };

        let title = if self.confirmation_mode != ConfirmationType::None {
            match self.confirmation_mode {
                ConfirmationType::Promote => "Canaries [CONFIRM PROMOTE: y/n]",
                ConfirmationType::Rollback => "Canaries [CONFIRM ROLLBACK: y/n]",
                ConfirmationType::Pause => "Canaries [CONFIRM PAUSE: y/n]",
                _ => "Canaries",
            }
        } else {
            "Active Canaries [Up/Down: Select | p: Promote | r: Rollback | +: Progress | d: Details]"
        };

        if items.is_empty() {
            let empty = vec![ListItem::new(Line::from(Span::styled(
                "  No active canary deployments",
                Style::default().fg(UNKNOWN_STATUS_COLOR),
            )))];
            let list = List::new(empty).block(bordered_block(title));
            f.render_widget(list, area);
        } else {
            let list = List::new(items).block(bordered_block(title));
            f.render_widget(list, area);
        }
    }

    fn render_traffic_split(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        canary: Option<&CanaryEngine>,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);

        let (split_text, canary_health_pct, canary_traffic_pct) = if let Some(engine) = canary {
            if let Some(c) = engine.active_canaries().get(self.selected_canary_index) {
                let stable_bar_len = (c.current_split.stable_pct as usize * 20) / 100;
                let canary_bar_len = (c.current_split.canary_pct as usize * 20) / 100;
                let stable_bar: String =
                    "|".repeat(stable_bar_len) + &" ".repeat(20 - stable_bar_len);
                let canary_bar: String =
                    "|".repeat(canary_bar_len) + &" ".repeat(20 - canary_bar_len);

                let sr = c.metrics.canary_success_rate();
                let text = format!(
                    "Traffic Distribution ({})\n\n\
                        Stable ({}):[{}] {}%\n\
                        Canary ({}):[{}] {}%\n\n\
                        Metrics:\n\
                        Canary Success:  {:.1}%\n\
                        Canary Latency:  {:.0}ms (vs {:.0}ms stable)\n\
                        Error Rate:      {:.1}%",
                    c.name,
                    c.stable_version,
                    stable_bar,
                    c.current_split.stable_pct,
                    c.canary_version,
                    canary_bar,
                    c.current_split.canary_pct,
                    sr * 100.0,
                    c.metrics.canary_avg_latency_ms,
                    c.metrics.stable_avg_latency_ms,
                    c.metrics.canary_error_rate() * 100.0,
                );
                (text, sr, c.current_split.canary_pct as f64 / 100.0)
            } else {
                ("No canary selected".to_string(), 0.0_f32, 0.0)
            }
        } else {
            ("Canary engine not available".to_string(), 0.0_f32, 0.0)
        };

        let split = Paragraph::new(split_text)
            .style(Style::default().fg(TEXT_COLOR))
            .block(bordered_block("Traffic Split"));

        f.render_widget(split, chunks[0]);

        // Health gauge
        let health_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(4), Constraint::Length(4)])
            .split(chunks[1]);

        let health_u16 = ((canary_health_pct * 100.0) as u16).min(100);
        let gauge1 = Gauge::default()
            .block(bordered_block("Canary Health"))
            .gauge_style(Style::default().fg(SUCCESS_COLOR))
            .percent(health_u16)
            .label(format!("{:.1}%", canary_health_pct * 100.0));

        f.render_widget(gauge1, health_chunks[0]);

        let progress_u16 = ((canary_traffic_pct * 100.0) as u16).min(100);
        let gauge2 = Gauge::default()
            .block(bordered_block("Progress"))
            .gauge_style(Style::default().fg(INFO_COLOR))
            .percent(progress_u16)
            .label(format!("{}% -> 100%", progress_u16));

        f.render_widget(gauge2, health_chunks[1]);
    }
}
