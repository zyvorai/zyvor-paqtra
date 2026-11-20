// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph, Tabs},
    Frame,
};

use super::app::{ModuleContainer, TuiApp};
use super::canary_view;
use super::help_overlay;
use super::tabs::TabIndex;
use super::theme::*;

impl TuiApp {
    pub(crate) fn ui(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(f.area());

        // Title
        let title = Paragraph::new(format!("🚀 Paqtra — trace every flow - {}", self.context))
            .style(
                Style::default()
                    .fg(TITLE_COLOR)
                    .add_modifier(Modifier::BOLD),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BORDER_COLOR)),
            );
        f.render_widget(title, chunks[0]);

        // Tabs
        let tabs = Tabs::new(TabIndex::tab_names())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Intelligence Modules")
                    .border_style(Style::default().fg(BORDER_COLOR)),
            )
            .select(self.selected_tab)
            .style(Style::default().fg(TAB_NORMAL_COLOR))
            .highlight_style(
                Style::default()
                    .fg(TAB_SELECTED_COLOR)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_widget(tabs, chunks[1]);

        // Content based on selected tab
        match self.selected_tab {
            0 => self.render_flows(f, chunks[2]),
            1 => self.render_connections(f, chunks[2]),
            2 => self.render_endpoints(f, chunks[2]),
            3 => self.render_policies(f, chunks[2]),
            4 => self.render_metrics(f, chunks[2]),
            5 => self.render_healer(f, chunks[2]),
            6 => {
                // AutoPolicy view
                if self.policy_detail_mode {
                    // Show policy detail view
                    self.render_policy_detail(f, chunks[2]);
                } else {
                    // Show normal AutoPolicy view
                    match &self.modules {
                        ModuleContainer::Enriched { autopolicy, .. } => {
                            self.autopolicy_view.render(f, chunks[2], Some(autopolicy))
                        }
                        ModuleContainer::Mock { autopolicy, .. } => {
                            self.autopolicy_view.render(f, chunks[2], Some(autopolicy))
                        }
                    }
                }
            }
            7 => {
                // RootCause view
                match &self.modules {
                    ModuleContainer::Enriched { rootcause, .. } => self.rootcause_view.render(
                        f,
                        chunks[2],
                        Some(rootcause),
                        self.selected_fix_index,
                        self.fix_apply_confirmation,
                    ),
                    ModuleContainer::Mock { rootcause, .. } => self.rootcause_view.render(
                        f,
                        chunks[2],
                        Some(rootcause),
                        self.selected_fix_index,
                        self.fix_apply_confirmation,
                    ),
                }
            }
            8 => {
                // Simulator view
                match &self.modules {
                    ModuleContainer::Enriched { simulator, .. } => {
                        self.simulator_view.render(f, chunks[2], Some(simulator))
                    }
                    ModuleContainer::Mock { simulator, .. } => {
                        self.simulator_view.render(f, chunks[2], Some(simulator))
                    }
                }
            }
            9 => {
                // Replay view
                match &self.modules {
                    ModuleContainer::Enriched { replay, .. } => {
                        self.replay_view.render(f, chunks[2], Some(replay))
                    }
                    ModuleContainer::Mock { replay, .. } => {
                        self.replay_view.render(f, chunks[2], Some(replay))
                    }
                }
            }
            10 => {
                // Chaos view
                self.chaos_view
                    .render(f, chunks[2], Some(&self.chaos_engine))
            }
            11 => {
                // Canary view
                self.canary_view
                    .render(f, chunks[2], Some(&self.canary_engine))
            }
            12 => {
                // MultiCluster view
                self.multicluster_view
                    .render(f, chunks[2], Some(&self.multicluster_engine))
            }
            _ => {}
        }

        // Footer with context-specific help and status messages
        let footer_text = if let Some(ref msg) = self.status_message {
            // Show status message for 5 seconds
            if self.status_message_time.elapsed().as_secs() < 5 {
                msg.clone()
            } else {
                // Clear expired message
                self.get_footer_text_for_tab()
            }
        } else {
            self.get_footer_text_for_tab()
        };

        let footer_style = if self.policy_apply_confirmation
            || self.policy_rollback_confirmation
            || self.policy_batch_apply_confirmation
            || self.policy_batch_rollback_confirmation
            || self.fix_apply_confirmation
        {
            // Confirmation prompt - use red for warning
            Style::default()
                .fg(ERROR_COLOR)
                .add_modifier(Modifier::BOLD)
        } else if self.status_message.is_some() && self.status_message_time.elapsed().as_secs() < 5
        {
            Style::default().fg(WARNING_COLOR)
        } else {
            Style::default().fg(TEXT_COLOR)
        };

        let footer = Paragraph::new(footer_text).style(footer_style).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
        f.render_widget(footer, chunks[3]);

        // Render help overlay on top if active
        if self.show_help {
            self.render_help_overlay(f);
        }
    }

    fn get_footer_text_for_tab(&self) -> String {
        match self.selected_tab {
            0 => {
                if self.show_packet_explanation {
                    "?: Help | q: Quit | Esc: Exit Explanation".to_string()
                } else {
                    "?: Help | q: Quit | ↑/↓: Select Flow | e: Explain Packet".to_string()
                }
            }
            5 => "?: Help | q: Quit | Tab: Next | d: Detect Problems".to_string(),
            6 => {
                if self.policy_apply_confirmation {
                    "⚠️ CONFIRM: y: Apply Policy | n: Cancel | Esc: Cancel".to_string()
                } else if self.policy_rollback_confirmation {
                    "⚠️ CONFIRM: y: Rollback Policy | n: Cancel | Esc: Cancel".to_string()
                } else if self.policy_batch_apply_confirmation {
                    "⚠️ CONFIRM: y: Apply All | n: Cancel | Esc: Cancel".to_string()
                } else if self.policy_batch_rollback_confirmation {
                    "⚠️ CONFIRM: y: Rollback All | n: Cancel | Esc: Cancel".to_string()
                } else if self.policy_detail_mode {
                    // Show different options based on policy applied status
                    let policies = match &self.modules {
                        ModuleContainer::Enriched { autopolicy, .. } => autopolicy.policies(),
                        ModuleContainer::Mock { autopolicy, .. } => autopolicy.policies(),
                    };
                    if !policies.is_empty()
                        && self.selected_policy_index < policies.len()
                        && self
                            .applied_policies
                            .contains(&policies[self.selected_policy_index].name)
                    {
                        "?: Help | q: Quit | Esc: Exit | ↑/↓: Navigate | r: Rollback".to_string()
                    } else {
                        "?: Help | q: Quit | Esc: Exit | ↑/↓: Navigate | a: Apply".to_string()
                    }
                } else {
                    "?: Help | q: Quit | u: Update | g: Generate | v: View | A: Apply All | R: Rollback All".to_string()
                }
            }
            7 => {
                if self.fix_apply_confirmation {
                    "⚠️ CONFIRM: y: Apply Fix | n: Cancel | Esc: Cancel".to_string()
                } else {
                    "?: Help | q: Quit | ↑/↓: Select Fix | a: Apply Fix".to_string()
                }
            }
            8 => {
                if self.simulator_view.last_simulation.is_some() {
                    "?: Help | q: Quit | c: Clear Results | Esc: Back".to_string()
                } else {
                    "?: Help | q: Quit | ↑/↓: Select | s: Simulate | c: Clear".to_string()
                }
            }
            9 => {
                if self.replay_view.time_travel_mode {
                    if self.replay_view.is_playing {
                        "?: Help | Space: Pause | ←/→: Step | [/]: Jump Events | +/-: Speed | Esc: Exit".to_string()
                    } else {
                        "?: Help | Space: Play | ←/→: Step | [/]: Jump Events | +/-: Speed | Esc: Exit".to_string()
                    }
                } else {
                    "?: Help | q: Quit | ↑/↓: Select | t: Time-Travel | r: Refresh".to_string()
                }
            }
            10 => {
                if self.chaos_view.confirmation_mode {
                    "⚠️ CONFIRM: y: Run Experiment | n: Cancel".to_string()
                } else if self.chaos_view.show_presets {
                    "?: Help | ↑/↓: Select | Enter: Run | v: View Active | b: Circuit Breaker"
                        .to_string()
                } else {
                    "?: Help | ↑/↓: Select | s: Stop | S: Stop All | v: View Presets".to_string()
                }
            }
            11 => {
                if self.canary_view.confirmation_mode != canary_view::ConfirmationType::None {
                    "⚠️ CONFIRM: y: Execute | n: Cancel".to_string()
                } else {
                    "?: Help | ↑/↓: Select | p: Promote | r: Rollback | +: Progress | d: Details"
                        .to_string()
                }
            }
            12 => "?: Help | ↑/↓: Select | v: Cycle View (Clusters/Topology/Syncs/Placements)"
                .to_string(),
            _ => "?: Help | q: Quit | Tab: Next | Shift+Tab: Previous".to_string(),
        }
    }

    fn render_help_overlay(&self, f: &mut Frame) {
        help_overlay::render_help_overlay(f);
    }
}
