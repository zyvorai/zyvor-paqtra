// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Chaos Engineering View - eBPF-based Fault Injection
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::theme::*;
use crate::modules::chaos::{ChaosEngine, ChaosExperiment, ChaosStatus};

/// Preset chaos experiments that can be launched from the UI.
pub const CHAOS_PRESETS: &[(
    &str,                  // name
    &str,                  // description
    &str,                  // severity label
    ratatui::style::Color, // color
)] = &[
    (
        "Network Partition",
        "Drop 20% of packets",
        "Medium",
        WARNING_COLOR,
    ),
    ("Latency Spike", "Add 500ms delay", "Medium", WARNING_COLOR),
    ("DNS Outage", "Fail 30% DNS lookups", "High", ERROR_COLOR),
    (
        "Connection Reset",
        "Kill 15% connections",
        "Medium",
        WARNING_COLOR,
    ),
    (
        "Bandwidth Limit",
        "Throttle to 10 Mbps",
        "Low",
        SUCCESS_COLOR,
    ),
    (
        "Packet Corruption",
        "Corrupt 5% packets",
        "High",
        ERROR_COLOR,
    ),
    (
        "Total Partition",
        "Drop 100% packets (DANGER)",
        "Critical",
        ERROR_COLOR,
    ),
];

/// Convert a preset index into a concrete ChaosExperiment value.
/// Returns `None` if the index is out of range.
pub fn preset_to_experiment(index: usize) -> Option<ChaosExperiment> {
    match index {
        0 => Some(ChaosExperiment::PacketDrop { drop_rate: 0.20 }),
        1 => Some(ChaosExperiment::Latency {
            delay_ms: 500,
            jitter_ms: 50,
        }),
        2 => Some(ChaosExperiment::DNSFailure { failure_rate: 0.30 }),
        3 => Some(ChaosExperiment::ConnectionKill { kill_rate: 0.15 }),
        4 => Some(ChaosExperiment::Bandwidth { limit_mbps: 10 }),
        5 => Some(ChaosExperiment::PacketCorruption {
            corruption_rate: 0.05,
        }),
        6 => Some(ChaosExperiment::PacketDrop { drop_rate: 1.0 }),
        _ => None,
    }
}

pub struct ChaosView {
    pub selected_experiment_index: usize,
    pub selected_preset_index: usize,
    pub show_presets: bool,
    pub confirmation_mode: bool,
    pub confirmed: bool,
    pub circuit_breaker_confirm: bool,
}

impl ChaosView {
    pub fn new() -> Self {
        Self {
            selected_experiment_index: 0,
            selected_preset_index: 0,
            show_presets: true,
            confirmation_mode: false,
            confirmed: false,
            circuit_breaker_confirm: false,
        }
    }

    pub fn move_selection_up(&mut self) {
        if self.show_presets {
            move_selection_up(&mut self.selected_preset_index);
        } else {
            move_selection_up(&mut self.selected_experiment_index);
        }
    }

    pub fn move_selection_down(&mut self, max: usize) {
        if self.show_presets {
            move_selection_down(&mut self.selected_preset_index, max);
        } else {
            move_selection_down(&mut self.selected_experiment_index, max);
        }
    }

    pub fn toggle_view(&mut self) {
        self.show_presets = !self.show_presets;
    }

    pub fn trigger_confirmation(&mut self) {
        self.confirmation_mode = true;
    }

    pub fn cancel_confirmation(&mut self) {
        self.confirmation_mode = false;
        self.circuit_breaker_confirm = false;
    }

    pub fn render(&self, f: &mut Frame, area: ratatui::layout::Rect, chaos: Option<&ChaosEngine>) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(9),  // Header & Status
                Constraint::Min(10),    // Experiments or Presets
                Constraint::Length(10), // Details
            ])
            .split(area);

        // Header
        self.render_header(f, chunks[0], chaos);

        if self.show_presets {
            // Preset experiments
            self.render_presets(f, chunks[1]);
            self.render_preset_details(f, chunks[2]);
        } else {
            // Active experiments
            self.render_active_experiments(f, chunks[1], chaos);
            self.render_experiment_details(f, chunks[2], chaos);
        }
    }

    fn render_header(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        chaos: Option<&ChaosEngine>,
    ) {
        let (active, total, cb_active) = if let Some(engine) = chaos {
            let stats = engine.stats();
            (
                stats.active_experiments,
                stats.total_experiments,
                stats.circuit_breaker_active,
            )
        } else {
            (0, 0, false)
        };

        let header_text = format!(
            "  eBPF Chaos Engineering\n\n\
            Status:            {}\n\
            Circuit Breaker:   {}\n\
            Active Tests:      {}\n\
            Total Runs:        {}\n\
            Safety Limits:     50% drop max, 5000ms latency max\n\n\
            View: {} | Press 'v' to toggle",
            if active > 0 { "Active" } else { "Idle" },
            if cb_active { "TRIGGERED" } else { "Normal" },
            active,
            total,
            if self.show_presets {
                "Experiment Presets"
            } else {
                "Active Experiments"
            }
        );

        let header = Paragraph::new(header_text)
            .style(Style::default().fg(WARNING_COLOR))
            .block(bordered_block("Chaos Status"));

        f.render_widget(header, area);
    }

    fn render_presets(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let items: Vec<ListItem> = CHAOS_PRESETS
            .iter()
            .enumerate()
            .map(|(idx, (name, desc, severity, color))| {
                let is_selected = idx == self.selected_preset_index;
                let style = if is_selected {
                    selected_style()
                } else {
                    Style::default().fg(*color)
                };

                let prefix = if is_selected { "> " } else { "  " };
                let content = format!(
                    "{}{}. {:<25} - {:<30} [{}]",
                    prefix,
                    idx + 1,
                    name,
                    desc,
                    severity
                );
                ListItem::new(Line::from(Span::styled(content, style)))
            })
            .collect();

        let title = if self.confirmation_mode {
            "Chaos Presets [CONFIRM: y/n]"
        } else {
            "Chaos Presets [Up/Down: Select | Enter: Run | v: View Active | b: Circuit Breaker]"
        };

        let list = List::new(items).block(bordered_block(title));

        f.render_widget(list, area);
    }

    fn render_preset_details(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let details = match self.selected_preset_index {
            0 => {
                "  Network Partition\n\n\
                Type:        Packet Drop\n\
                Rate:        20%\n\
                Target:      All namespaces, egress traffic\n\
                Duration:    5 minutes (auto-cleanup)\n\
                Severity:    Medium\n\n\
                What It Tests:\n\
                Simulates network instability and packet loss.\n\
                Tests application retry logic and resilience."
            }
            1 => {
                "  Latency Spike\n\n\
                Type:        Latency Injection\n\
                Delay:       500ms (+/-50ms jitter)\n\
                Target:      Default namespace, both directions\n\
                Duration:    5 minutes (auto-cleanup)\n\
                Severity:    Medium\n\n\
                What It Tests:\n\
                Simulates network congestion and slow responses.\n\
                Tests timeout handling and async patterns."
            }
            2 => {
                "  DNS Outage\n\n\
                Type:        DNS Failure\n\
                Rate:        30%\n\
                Target:      Port 53 (DNS)\n\
                Duration:    3 minutes (auto-cleanup)\n\
                Severity:    High\n\n\
                What It Tests:\n\
                Simulates DNS resolver issues.\n\
                Tests DNS caching and service discovery."
            }
            3 => {
                "  Connection Reset\n\n\
                Type:        Connection Termination\n\
                Rate:        15%\n\
                Target:      TCP connections\n\
                Duration:    5 minutes (auto-cleanup)\n\
                Severity:    Medium\n\n\
                What It Tests:\n\
                Simulates connection failures.\n\
                Tests reconnection logic and circuit breakers."
            }
            4 => {
                "  Bandwidth Limit\n\n\
                Type:        Bandwidth Throttling\n\
                Limit:       10 Mbps\n\
                Target:      All traffic\n\
                Duration:    5 minutes (auto-cleanup)\n\
                Severity:    Low\n\n\
                What It Tests:\n\
                Simulates bandwidth constraints.\n\
                Tests performance under limited bandwidth."
            }
            5 => {
                "  Packet Corruption\n\n\
                Type:        Packet Corruption\n\
                Rate:        5%\n\
                Target:      All traffic\n\
                Duration:    3 minutes (auto-cleanup)\n\
                Severity:    High\n\n\
                What It Tests:\n\
                Simulates data corruption in transit.\n\
                Tests checksum validation and error handling."
            }
            6 => {
                "  Total Partition (DANGER)\n\n\
                Type:        Packet Drop\n\
                Rate:        100%\n\
                Target:      Selected namespace only\n\
                Duration:    1 minute (auto-cleanup)\n\
                Severity:    Critical\n\n\
                What It Tests:\n\
                Complete network isolation.\n\
                Tests disaster recovery and failover.\n\n\
                WARNING: This will completely isolate the target!"
            }
            _ => "Select a chaos preset to see details",
        };

        let paragraph = Paragraph::new(details)
            .style(Style::default().fg(TEXT_COLOR))
            .block(bordered_block("Experiment Details"))
            .wrap(Wrap { trim: false });

        f.render_widget(paragraph, area);
    }

    fn render_active_experiments(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        chaos: Option<&ChaosEngine>,
    ) {
        let items: Vec<ListItem> = if let Some(engine) = chaos {
            engine
                .active_experiments()
                .iter()
                .enumerate()
                .map(|(idx, exp)| {
                    let is_selected = idx == self.selected_experiment_index;
                    let status_str = match &exp.status {
                        ChaosStatus::Active => "Active",
                        ChaosStatus::Pending => "Pending",
                        ChaosStatus::Paused => "Paused",
                        ChaosStatus::Stopped => "Stopped",
                        ChaosStatus::Failed { .. } => "Failed",
                    };
                    let color = match &exp.status {
                        ChaosStatus::Active => SUCCESS_COLOR,
                        ChaosStatus::Paused => WARNING_COLOR,
                        ChaosStatus::Failed { .. } => ERROR_COLOR,
                        _ => INFO_COLOR,
                    };
                    let style = if is_selected {
                        selected_style()
                    } else {
                        Style::default().fg(color)
                    };

                    let prefix = if is_selected { "> " } else { "  " };
                    let elapsed = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs().saturating_sub(exp.started_at))
                        .unwrap_or(0);
                    let truncated_id: String = exp.id.chars().take(14).collect();
                    let content = format!(
                        "{}{:<15} {:<25} [{:<8}] {}s ago",
                        prefix, truncated_id, exp.name, status_str, elapsed
                    );
                    ListItem::new(Line::from(Span::styled(content, style)))
                })
                .collect()
        } else {
            vec![ListItem::new(Line::from(Span::styled(
                "  No chaos engine available",
                Style::default().fg(UNKNOWN_STATUS_COLOR),
            )))]
        };

        if items.is_empty() {
            let empty = vec![ListItem::new(Line::from(Span::styled(
                "  No active experiments",
                Style::default().fg(UNKNOWN_STATUS_COLOR),
            )))];
            let list = List::new(empty).block(bordered_block(
                "Active Experiments [Up/Down: Select | s: Stop | S: Stop All | v: View Presets]",
            ));
            f.render_widget(list, area);
        } else {
            let list = List::new(items).block(bordered_block(
                "Active Experiments [Up/Down: Select | s: Stop | S: Stop All | v: View Presets]",
            ));
            f.render_widget(list, area);
        }
    }

    fn render_experiment_details(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        chaos: Option<&ChaosEngine>,
    ) {
        let details = if let Some(engine) = chaos {
            let experiments = engine.active_experiments();
            if let Some(exp) = experiments.get(self.selected_experiment_index) {
                let elapsed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs().saturating_sub(exp.started_at))
                    .unwrap_or(0);
                let remaining = exp
                    .expires_at
                    .map(|e| {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        e.saturating_sub(now)
                    })
                    .map(|r| format!("{}s remaining", r))
                    .unwrap_or_else(|| "No auto-cleanup".to_string());

                format!(
                    "Active Experiment: {}\n\n\
                    ID:              {}\n\
                    Type:            {}\n\
                    Target:          {} namespace(s)\n\
                    Running:         {}s\n\
                    Auto-cleanup:    {}\n\n\
                    Metrics:\n\
                    Packets Affected:  {}\n\
                    Connections:       {}\n\
                    Error Rate:        {:.1}%",
                    exp.name,
                    exp.id,
                    exp.experiment.description(),
                    if exp.target.namespaces.is_empty() {
                        "all".to_string()
                    } else {
                        exp.target.namespaces.join(", ")
                    },
                    elapsed,
                    remaining,
                    exp.metrics.packets_affected,
                    exp.metrics.connections_affected,
                    exp.metrics.error_rate * 100.0,
                )
            } else {
                "No experiment selected".to_string()
            }
        } else {
            "Chaos engine not available".to_string()
        };

        let paragraph = Paragraph::new(details)
            .style(Style::default().fg(INFO_COLOR))
            .block(bordered_block("Experiment Metrics"));

        f.render_widget(paragraph, area);
    }
}
