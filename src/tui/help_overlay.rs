// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use super::theme::*;

pub fn render_help_overlay(f: &mut Frame) {
    // Create a centered popup
    let area = centered_rect(85, 90, f.area());

    // Clear the background
    f.render_widget(Clear, area);

    let help_content = vec![
        Line::from(vec![Span::styled(
            "Paqtra - Keyboard Shortcuts",
            Style::default()
                .fg(TITLE_COLOR)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "═══ GLOBAL ═══",
            Style::default()
                .fg(HEADER_COLOR)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled(
                "  ?",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Toggle this help screen"),
        ]),
        Line::from(vec![
            Span::styled(
                "  q",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Quit application"),
        ]),
        Line::from(vec![
            Span::styled(
                "  Tab",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("        Next tab"),
        ]),
        Line::from(vec![
            Span::styled(
                "  Shift+Tab",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("   Previous tab"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "═══ FLOWS TAB ═══",
            Style::default()
                .fg(HEADER_COLOR)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled(
                "  ↑/↓",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("        Navigate flows"),
        ]),
        Line::from(vec![
            Span::styled(
                "  e",
                Style::default().fg(INFO_COLOR).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Explain selected packet (what/why/how)"),
        ]),
        Line::from(vec![
            Span::styled(
                "  Esc",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("        Exit packet explanation"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "═══ HEALER TAB ═══",
            Style::default()
                .fg(HEADER_COLOR)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled(
                "  d",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Detect problems manually (trigger scan)"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "═══ AUTOPOLICY TAB ═══",
            Style::default()
                .fg(HEADER_COLOR)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled(
                "  u",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Update learning (manual observation)"),
        ]),
        Line::from(vec![
            Span::styled(
                "  g",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Generate policies from patterns"),
        ]),
        Line::from(vec![
            Span::styled(
                "  v",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Toggle policy detail view"),
        ]),
        Line::from(vec![
            Span::styled(
                "  A",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Batch apply all unapplied policies"),
        ]),
        Line::from(vec![
            Span::styled(
                "  R",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Batch rollback all applied policies"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  ══ Policy Detail Mode ══",
            Style::default().fg(INFO_COLOR).add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled(
                "  ↑/↓",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("        Navigate between policies"),
        ]),
        Line::from(vec![
            Span::styled(
                "  a",
                Style::default()
                    .fg(SUCCESS_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Apply selected policy (requires confirmation)"),
        ]),
        Line::from(vec![
            Span::styled(
                "  r",
                Style::default()
                    .fg(ERROR_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Rollback selected policy (requires confirmation)"),
        ]),
        Line::from(vec![
            Span::styled(
                "  y",
                Style::default()
                    .fg(SUCCESS_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Confirm action"),
        ]),
        Line::from(vec![
            Span::styled(
                "  n",
                Style::default()
                    .fg(ERROR_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Cancel action"),
        ]),
        Line::from(vec![
            Span::styled(
                "  Esc",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("        Exit detail view or cancel confirmation"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "═══ ROOTCAUSE TAB ═══",
            Style::default()
                .fg(HEADER_COLOR)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled(
                "  ↑/↓",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("        Navigate recommended fixes"),
        ]),
        Line::from(vec![
            Span::styled(
                "  a",
                Style::default()
                    .fg(SUCCESS_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Apply selected fix (requires confirmation)"),
        ]),
        Line::from(vec![
            Span::styled(
                "  y",
                Style::default()
                    .fg(SUCCESS_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Confirm fix application"),
        ]),
        Line::from(vec![
            Span::styled(
                "  n",
                Style::default()
                    .fg(ERROR_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Cancel fix application"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "═══ SIMULATOR TAB ═══",
            Style::default()
                .fg(HEADER_COLOR)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled(
                "  ↑/↓",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("        Navigate scenarios"),
        ]),
        Line::from(vec![
            Span::styled(
                "  s",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Run simulation (dry-run what-if analysis)"),
        ]),
        Line::from(vec![
            Span::styled(
                "  c",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Clear simulation results"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "═══ REPLAY TAB ═══",
            Style::default()
                .fg(HEADER_COLOR)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled(
                "  ↑/↓",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("        Navigate recordings"),
        ]),
        Line::from(vec![
            Span::styled(
                "  t",
                Style::default().fg(INFO_COLOR).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Enter time-travel debugging mode"),
        ]),
        Line::from(vec![
            Span::styled(
                "  r",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Refresh recordings list"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  ══ Time-Travel Mode ══",
            Style::default().fg(INFO_COLOR).add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled(
                "  ←/→",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("        Step backward/forward in timeline"),
        ]),
        Line::from(vec![
            Span::styled(
                "  Space",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("      Play/Pause timeline playback"),
        ]),
        Line::from(vec![
            Span::styled(
                "  [/]",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("        Jump to previous/next event marker"),
        ]),
        Line::from(vec![
            Span::styled(
                "  +/-",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("        Increase/decrease playback speed"),
        ]),
        Line::from(vec![
            Span::styled(
                "  Esc",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("        Exit time-travel mode"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "═══ CHAOS TAB ═══",
            Style::default()
                .fg(HEADER_COLOR)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled(
                "  ↑/↓",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("        Navigate experiments/presets"),
        ]),
        Line::from(vec![
            Span::styled(
                "  Enter",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("      Run selected chaos experiment"),
        ]),
        Line::from(vec![
            Span::styled(
                "  v",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Toggle between presets and active experiments"),
        ]),
        Line::from(vec![
            Span::styled(
                "  s",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Stop selected experiment"),
        ]),
        Line::from(vec![
            Span::styled(
                "  S",
                Style::default()
                    .fg(ERROR_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Stop all experiments (emergency)"),
        ]),
        Line::from(vec![
            Span::styled(
                "  b",
                Style::default()
                    .fg(ERROR_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Trigger circuit breaker (disable all chaos)"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "═══ CANARY TAB ═══",
            Style::default()
                .fg(HEADER_COLOR)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled(
                "  ↑/↓",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("        Navigate canary deployments"),
        ]),
        Line::from(vec![
            Span::styled(
                "  p",
                Style::default()
                    .fg(SUCCESS_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Promote canary (confirm with y/n)"),
        ]),
        Line::from(vec![
            Span::styled(
                "  r",
                Style::default()
                    .fg(ERROR_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Rollback canary (confirm with y/n)"),
        ]),
        Line::from(vec![
            Span::styled(
                "  +",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Progress traffic (+10%)"),
        ]),
        Line::from(vec![
            Span::styled(
                "  d",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Toggle detailed metrics view"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "═══ MULTICLUSTER TAB ═══",
            Style::default()
                .fg(HEADER_COLOR)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled(
                "  ↑/↓",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("        Navigate clusters/items"),
        ]),
        Line::from(vec![
            Span::styled(
                "  v",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Cycle views: Clusters → Topology → Syncs → Placements"),
        ]),
        Line::from(vec![
            Span::styled(
                "  h",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("          Health check all clusters"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "═══════════════════════════════════════════════",
            Style::default().fg(BORDER_COLOR),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Press ", Style::default().fg(TEXT_COLOR)),
            Span::styled(
                "?",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" or ", Style::default().fg(TEXT_COLOR)),
            Span::styled(
                "Esc",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to close this help", Style::default().fg(TEXT_COLOR)),
        ]),
    ];

    let help_paragraph = Paragraph::new(help_content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ORANGE).add_modifier(Modifier::BOLD))
                .title(vec![
                    Span::raw(" "),
                    Span::styled(
                        "❓ HELP",
                        Style::default()
                            .fg(TITLE_COLOR)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                ])
                .style(Style::default().bg(ratatui::style::Color::Black)),
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false });

    f.render_widget(help_paragraph, area);
}

/// Helper function to create a centered rectangle
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
