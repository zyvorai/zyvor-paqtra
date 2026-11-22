// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Theme colors for Paqtra TUI
// Inspired by GuestKit's Coral-Terracotta Orange Theme (Pantone 7416 C)

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders};

// Primary Theme Colors - Coral-Terracotta Orange Palette
pub const ORANGE: Color = Color::Rgb(222, 115, 86); // Primary coral orange
pub const DARK_ORANGE: Color = Color::Rgb(180, 85, 60); // Darker terracotta
pub const LIGHT_ORANGE: Color = Color::Rgb(255, 145, 115); // Lighter coral

// Background and Text
pub const BG_COLOR: Color = Color::Reset;
pub const TEXT_COLOR: Color = Color::Rgb(220, 220, 220); // Softer white
pub const BORDER_COLOR: Color = DARK_ORANGE; // Uses Dark Orange

// Status Colors
pub const SUCCESS_COLOR: Color = Color::Rgb(50, 205, 50); // Brighter green
pub const WARNING_COLOR: Color = Color::Rgb(255, 200, 0); // Deeper yellow
pub const ERROR_COLOR: Color = Color::Rgb(220, 50, 47); // Deep red
pub const INFO_COLOR: Color = Color::Rgb(100, 150, 255); // Soft blue

// State-specific Colors
pub const ESTABLISHED_COLOR: Color = SUCCESS_COLOR; // Green for established connections
pub const NEW_CONNECTION_COLOR: Color = INFO_COLOR; // Blue for new connections
pub const RELATED_COLOR: Color = Color::Rgb(138, 180, 248); // Lighter blue for related
pub const INVALID_COLOR: Color = ERROR_COLOR; // Red for invalid states

// Traffic Colors
pub const FORWARDED_COLOR: Color = SUCCESS_COLOR; // Green for forwarded traffic
pub const DROPPED_COLOR: Color = ERROR_COLOR; // Red for dropped traffic
pub const UNKNOWN_TRAFFIC_COLOR: Color = WARNING_COLOR; // Yellow for unknown

// Endpoint Status Colors
pub const RUNNING_COLOR: Color = SUCCESS_COLOR; // Green for running
pub const PENDING_COLOR: Color = WARNING_COLOR; // Yellow for pending
pub const FAILED_COLOR: Color = ERROR_COLOR; // Red for failed
pub const UNKNOWN_STATUS_COLOR: Color = Color::Rgb(160, 160, 160); // Gray for unknown

// Confidence Level Colors
pub const HIGH_CONFIDENCE_COLOR: Color = SUCCESS_COLOR; // >80% confidence
pub const MEDIUM_CONFIDENCE_COLOR: Color = WARNING_COLOR; // 50-80% confidence
pub const LOW_CONFIDENCE_COLOR: Color = ERROR_COLOR; // <50% confidence

// Severity Colors (for incidents/alerts)
pub const CRITICAL_SEVERITY_COLOR: Color = ERROR_COLOR; // Critical issues
pub const MEDIUM_SEVERITY_COLOR: Color = WARNING_COLOR; // Medium severity
pub const LOW_SEVERITY_COLOR: Color = INFO_COLOR; // Low severity

// UI Element Colors
pub const TAB_SELECTED_COLOR: Color = ORANGE; // Selected tab
pub const TAB_NORMAL_COLOR: Color = TEXT_COLOR; // Normal tab
pub const HEADER_COLOR: Color = LIGHT_ORANGE; // Headers
pub const TITLE_COLOR: Color = ORANGE; // Titles
pub const HIGHLIGHT_COLOR: Color = LIGHT_ORANGE; // Highlighted items

// Recording/Replay Colors
pub const RECORDING_ACTIVE_COLOR: Color = SUCCESS_COLOR; // Active recording
pub const RECORDING_RECENT_COLOR: Color = INFO_COLOR; // Recent recording
pub const RECORDING_OLD_COLOR: Color = TEXT_COLOR; // Old recording
pub const RECORDING_WARNING_COLOR: Color = WARNING_COLOR; // Warning state

// Gauge/Progress Colors
pub const PROGRESS_NORMAL_COLOR: Color = SUCCESS_COLOR; // Normal progress
pub const PROGRESS_WARNING_COLOR: Color = WARNING_COLOR; // Warning progress
pub const PROGRESS_ERROR_COLOR: Color = ERROR_COLOR; // Error progress

// Widget helpers — shared by all TUI views

pub fn bordered_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(BORDER_COLOR))
}

pub fn selected_style() -> Style {
    Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)
}

pub fn move_selection_up(index: &mut usize) {
    if *index > 0 {
        *index -= 1;
    }
}

pub fn move_selection_down(index: &mut usize, max: usize) {
    if *index < max.saturating_sub(1) {
        *index += 1;
    }
}

// Helper function to get confidence color
pub fn confidence_color(confidence: f64) -> Color {
    if confidence >= 0.8 {
        HIGH_CONFIDENCE_COLOR
    } else if confidence >= 0.5 {
        MEDIUM_CONFIDENCE_COLOR
    } else {
        LOW_CONFIDENCE_COLOR
    }
}

// Helper function to get severity color
pub fn severity_color(severity: &str) -> Color {
    match severity.to_lowercase().as_str() {
        "critical" => CRITICAL_SEVERITY_COLOR,
        "high" => ERROR_COLOR,
        "medium" => MEDIUM_SEVERITY_COLOR,
        "low" => LOW_SEVERITY_COLOR,
        _ => TEXT_COLOR,
    }
}
