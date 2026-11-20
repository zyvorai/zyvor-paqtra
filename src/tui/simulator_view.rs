// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// Simulator View - What-If Scenario Testing
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{Gauge, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::theme::*;
use crate::ebpf::MapReader;
use crate::modules::simulator::{SimulationResult, Simulator};

pub struct SimulatorView {
    pub should_simulate: bool,
    pub selected_scenario_index: usize,
    pub last_simulation: Option<SimulationResult>,
    pub simulation_running: bool,
}

impl SimulatorView {
    pub fn new() -> Self {
        Self {
            should_simulate: false,
            selected_scenario_index: 0,
            last_simulation: None,
            simulation_running: false,
        }
    }

    pub fn trigger_simulation(&mut self) {
        self.should_simulate = true;
        self.simulation_running = true;
    }

    pub fn move_selection_up(&mut self) {
        move_selection_up(&mut self.selected_scenario_index);
    }

    pub fn move_selection_down(&mut self, max_scenarios: usize) {
        move_selection_down(&mut self.selected_scenario_index, max_scenarios);
    }

    pub fn set_simulation_result(&mut self, result: SimulationResult) {
        self.last_simulation = Some(result);
        self.simulation_running = false;
    }

    pub fn clear_simulation(&mut self) {
        self.last_simulation = None;
    }

    pub fn render<M: MapReader>(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        simulator: Option<&Simulator<M>>,
    ) {
        if self.last_simulation.is_some() {
            // Show detailed simulation results
            self.render_detailed_results(f, area);
        } else {
            // Show scenario selection
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(8),  // Header & Stats
                    Constraint::Min(10),    // Scenario Selection
                    Constraint::Length(10), // Scenario Details
                ])
                .split(area);

            // Header
            self.render_header(f, chunks[0], simulator);

            // Scenario Selection
            self.render_scenario_list(f, chunks[1]);

            // Selected Scenario Details
            self.render_scenario_details(f, chunks[2]);
        }
    }

    fn render_header<M: MapReader>(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        simulator: Option<&Simulator<M>>,
    ) {
        let stats = if let Some(sim) = simulator {
            let stats = sim.stats();
            format!(
                "🔮 Dry-Run Networking Simulator\n\n\
                Status:            {}\n\
                Flow History:      {}\n\
                Current Policies:  {}\n\
                History Window:    {}s",
                if self.simulation_running {
                    "Running..."
                } else {
                    "Ready"
                },
                stats.flow_history_size,
                stats.current_policies,
                stats.history_window_secs
            )
        } else {
            "🔮 Dry-Run Networking Simulator\n\n\
            Status:            Initializing...\n\
            Flow History:      0\n\
            Current Policies:  0\n\
            History Window:    3600s"
                .to_string()
        };

        let content = Paragraph::new(stats)
            .style(Style::default().fg(INFO_COLOR))
            .block(bordered_block("Simulator Status"));

        f.render_widget(content, area);
    }

    fn render_scenario_list(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let scenarios = [
            "Add DNS Egress Policy (allow DNS to 8.8.8.8)",
            "Block External IP 1.2.3.4",
            "Default Deny for 'production' namespace",
            "Allow frontend → backend:8080",
            "Modify ingress to allow port 443",
            "Block all traffic on port 22 (SSH)",
            "Allow DB access: api → postgres:5432",
        ];

        let items: Vec<ListItem> = scenarios
            .iter()
            .enumerate()
            .map(|(idx, name)| {
                let style = if idx == self.selected_scenario_index {
                    selected_style()
                } else {
                    Style::default().fg(TEXT_COLOR)
                };

                let prefix = if idx == self.selected_scenario_index {
                    "▶ "
                } else {
                    "  "
                };
                let content = format!("{}{}. {}", prefix, idx + 1, name);

                ListItem::new(Line::from(Span::styled(content, style)))
            })
            .collect();

        let title = if self.simulation_running {
            "Scenarios [Simulating...]"
        } else {
            "Scenarios [↑/↓: Select | s: Simulate | c: Clear]"
        };

        let list = List::new(items).block(bordered_block(title));

        f.render_widget(list, area);
    }

    fn render_scenario_details(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let details = match self.selected_scenario_index {
            0 => {
                "📝 Scenario: Add DNS Egress Policy\n\n\
                Type:        Add Policy\n\
                Target:      default namespace\n\
                Effect:      Allow egress to 8.8.8.8:53 (DNS)\n\n\
                What It Does:\n\
                Adds a CiliumNetworkPolicy allowing all pods in the\n\
                default namespace to make DNS queries to Google DNS.\n\n\
                Expected Impact: Low\n\
                Estimated Affected Flows: 50-100"
            }
            1 => {
                "📝 Scenario: Block External IP\n\n\
                Type:        Block Traffic\n\
                Target:      IP 1.2.3.4\n\
                Effect:      Block all egress to this IP\n\n\
                What It Does:\n\
                Prevents any pod from communicating with the\n\
                external IP address 1.2.3.4.\n\n\
                Expected Impact: Medium\n\
                Estimated Affected Flows: 10-20"
            }
            2 => {
                "📝 Scenario: Default Deny\n\n\
                Type:        Add Policy\n\
                Target:      production namespace\n\
                Effect:      Default deny all ingress and egress\n\n\
                What It Does:\n\
                Implements zero-trust by denying all traffic by\n\
                default. Requires explicit allow policies.\n\n\
                Expected Impact: CRITICAL ⚠️\n\
                Estimated Affected Flows: 500+"
            }
            3 => {
                "📝 Scenario: Allow Frontend → Backend\n\n\
                Type:        Add Policy\n\
                Target:      default namespace\n\
                Effect:      Allow frontend pods to backend:8080\n\n\
                What It Does:\n\
                Adds an egress policy for frontend pods and ingress\n\
                policy for backend pods on port 8080.\n\n\
                Expected Impact: Low\n\
                Estimated Affected Flows: 30-50"
            }
            4 => {
                "📝 Scenario: Modify Ingress Policy\n\n\
                Type:        Modify Policy\n\
                Target:      ingress-nginx namespace\n\
                Effect:      Add port 443 to allowed ports\n\n\
                What It Does:\n\
                Updates existing ingress policy to also allow\n\
                HTTPS traffic on port 443.\n\n\
                Expected Impact: Low\n\
                Estimated Affected Flows: 100-200"
            }
            5 => {
                "📝 Scenario: Block SSH\n\n\
                Type:        Block Traffic\n\
                Target:      All namespaces\n\
                Effect:      Block port 22 (SSH)\n\n\
                What It Does:\n\
                Adds network policy to deny all SSH traffic\n\
                cluster-wide for security hardening.\n\n\
                Expected Impact: Medium\n\
                Estimated Affected Flows: 5-10"
            }
            6 => {
                "📝 Scenario: Allow Database Access\n\n\
                Type:        Add Policy\n\
                Target:      default namespace\n\
                Effect:      Allow api → postgres:5432\n\n\
                What It Does:\n\
                Adds policy allowing API pods to connect to\n\
                PostgreSQL database on port 5432.\n\n\
                Expected Impact: Low\n\
                Estimated Affected Flows: 20-40"
            }
            _ => "Select a scenario to see details",
        };

        let paragraph = Paragraph::new(details)
            .style(Style::default().fg(INFO_COLOR))
            .block(bordered_block("Scenario Details"))
            .wrap(Wrap { trim: false });

        f.render_widget(paragraph, area);
    }

    fn render_detailed_results(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let result = match &self.last_simulation {
            Some(r) => r,
            None => return,
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8),  // Impact Summary
                Constraint::Length(10), // Risk Assessment
                Constraint::Min(8),     // Recommendations
            ])
            .split(area);

        // Impact Summary
        let impact_text = format!(
            "📊 Impact Analysis\n\n\
            Total Flows:       {}\n\
            Blocked Flows:     {} ({}%)\n\
            Allowed Flows:     {}\n\
            Changed Flows:     {}\n\
            Impacted Services: {}",
            result.impact.total_flows,
            result.impact.blocked_flows,
            (result.impact.blocked_flows * 100)
                .checked_div(result.impact.total_flows)
                .unwrap_or(0),
            result.impact.allowed_flows,
            result.impact.changed_flows,
            result.impact.impacted_services.len(),
        );

        let impact = Paragraph::new(impact_text)
            .style(Style::default().fg(INFO_COLOR))
            .block(bordered_block("Impact Summary"));

        f.render_widget(impact, chunks[0]);

        // Risk Assessment
        let risk_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[1]);

        let risk_color = match result.risk.level {
            crate::modules::simulator::RiskLevel::Low => SUCCESS_COLOR,
            crate::modules::simulator::RiskLevel::Medium => WARNING_COLOR,
            crate::modules::simulator::RiskLevel::High => ERROR_COLOR,
            crate::modules::simulator::RiskLevel::Critical => ERROR_COLOR,
        };

        let risk_text = format!(
            "🎯 Risk Assessment\n\n\
            Level:         {}\n\
            Score:         {}/10\n\
            Safe to Apply: {}\n\
            Confidence:    {:.0}%\n\
            Factors:       {}",
            result.risk.level,
            result.risk.score,
            if result.risk.safe_to_apply {
                "✅ YES"
            } else {
                "❌ NO"
            },
            result.confidence * 100.0,
            result.risk.factors.len(),
        );

        let risk_info = Paragraph::new(risk_text)
            .style(Style::default().fg(risk_color))
            .block(bordered_block("Risk Analysis"));

        f.render_widget(risk_info, risk_chunks[0]);

        // Risk Gauge
        let risk_percent = ((result.risk.score as f32 / 10.0) * 100.0) as u16;
        let gauge = Gauge::default()
            .block(bordered_block("Risk Meter"))
            .gauge_style(Style::default().fg(risk_color))
            .percent(risk_percent)
            .label(format!("{}%", risk_percent));

        f.render_widget(gauge, risk_chunks[1]);

        // Recommendations
        let rec_text = format!(
            "💡 Recommendations\n\n{}",
            result
                .recommendations
                .iter()
                .enumerate()
                .map(|(i, r)| format!("{}. {}", i + 1, r))
                .collect::<Vec<_>>()
                .join("\n\n")
        );

        let recommendations = Paragraph::new(rec_text)
            .style(Style::default().fg(TEXT_COLOR))
            .block(bordered_block(
                "Recommendations [c: Clear Results | Esc: Back]",
            ))
            .wrap(Wrap { trim: false });

        f.render_widget(recommendations, chunks[2]);
    }
}
