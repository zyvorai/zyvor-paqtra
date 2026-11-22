// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Replay View - Traffic Recording & Playback with Time-Travel Debugging
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::theme::*;
use crate::ebpf::{MapReader, PolicyVerdict};
use crate::modules::replay::{RecordedFlow, ReplayEngine};

pub struct ReplayView {
    // Time-travel state
    pub time_travel_mode: bool,
    pub selected_recording_index: usize,
    pub timeline_position: usize, // Current position in timeline (0-100)
    pub is_playing: bool,
    pub playback_speed: f32,
    // Loaded flow data for time-travel mode
    pub loaded_flows: Vec<RecordedFlow>,
    pub loaded_recording_name: String,
}

impl ReplayView {
    pub fn new() -> Self {
        Self {
            time_travel_mode: false,
            selected_recording_index: 0,
            timeline_position: 0,
            is_playing: false,
            playback_speed: 1.0,
            loaded_flows: Vec::new(),
            loaded_recording_name: String::new(),
        }
    }

    /// Load flows from the selected recording into local state.
    pub fn load_flows<M: MapReader>(&mut self, replay: &ReplayEngine<M>) {
        if let Some(rec) = replay.recordings().get(self.selected_recording_index) {
            match replay.load_recording_flows(&rec.id) {
                Ok(flows) => {
                    self.loaded_recording_name = rec.name.clone();
                    self.loaded_flows = flows;
                }
                Err(e) => {
                    tracing::warn!("Failed to load recording flows: {}", e);
                    self.loaded_flows.clear();
                }
            }
        }
    }

    pub fn enter_time_travel(&mut self) {
        self.time_travel_mode = true;
        self.timeline_position = 0;
        self.is_playing = false;
        // Flows will be loaded from events.rs when entering time-travel
    }

    pub fn exit_time_travel(&mut self) {
        self.time_travel_mode = false;
        self.is_playing = false;
        self.loaded_flows.clear();
        self.loaded_recording_name.clear();
    }

    pub fn toggle_playback(&mut self) {
        self.is_playing = !self.is_playing;
    }

    pub fn step_forward(&mut self) {
        if self.timeline_position < 100 {
            self.timeline_position += 1;
        }
    }

    pub fn step_backward(&mut self) {
        if self.timeline_position > 0 {
            self.timeline_position -= 1;
        }
    }

    pub fn jump_to_next_event(&mut self) {
        // Jump to next significant event (drop, error, etc.)
        self.timeline_position = (self.timeline_position + 10).min(100);
    }

    pub fn jump_to_prev_event(&mut self) {
        // Jump to previous significant event
        self.timeline_position = self.timeline_position.saturating_sub(10);
    }

    pub fn move_selection_up(&mut self) {
        move_selection_up(&mut self.selected_recording_index);
    }

    pub fn move_selection_down(&mut self, max: usize) {
        move_selection_down(&mut self.selected_recording_index, max);
    }

    pub fn adjust_speed(&mut self, faster: bool) {
        if faster {
            self.playback_speed = (self.playback_speed * 2.0).min(8.0);
        } else {
            self.playback_speed = (self.playback_speed / 2.0).max(0.25);
        }
    }

    pub fn render<M: MapReader>(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        replay: Option<&ReplayEngine<M>>,
    ) {
        if self.time_travel_mode {
            // Time-travel debugging mode
            self.render_time_travel(f, area, replay);
        } else {
            // Normal recording list mode
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(8), // Header
                    Constraint::Min(10),   // Recordings List
                    Constraint::Length(8), // Comparison Results
                ])
                .split(area);

            // Header
            self.render_header(f, chunks[0], replay);

            // Recordings
            self.render_recordings(f, chunks[1], replay);

            // Latest Comparison
            self.render_comparison(f, chunks[2]);
        }
    }

    fn render_header<M: MapReader>(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        replay: Option<&ReplayEngine<M>>,
    ) {
        let stats = if let Some(engine) = replay {
            let s = engine.stats();
            format!(
                "📹 Traffic Replay System\n\n\
                Status:            {}\n\
                Total Recordings:  {}\n\
                Total Flows:       {}\n\n\
                Press 'r' to refresh recordings list",
                if s.recording_in_progress {
                    "🔴 Recording"
                } else {
                    "⏹️  Idle"
                },
                s.total_recordings,
                s.total_flows_recorded,
            )
        } else {
            "Traffic Replay not initialized".to_string()
        };

        let content = Paragraph::new(stats)
            .style(Style::default().fg(ORANGE))
            .block(bordered_block("Replay Status"));

        f.render_widget(content, area);
    }

    fn render_recordings<M: MapReader>(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        replay: Option<&ReplayEngine<M>>,
    ) {
        let title = "Recordings [↑/↓: Select | t: Time-Travel | r: Refresh]";

        let recordings = replay.map(|engine| engine.recordings());

        if recordings.is_none() || recordings.is_some_and(|r| r.is_empty()) {
            let empty_msg = Paragraph::new("No recordings. Press 'r' to start a new recording.")
                .style(Style::default().fg(TEXT_COLOR))
                .block(bordered_block(title));
            f.render_widget(empty_msg, area);
            return;
        }

        let recordings = recordings.unwrap();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let items: Vec<ListItem> = recordings
            .iter()
            .enumerate()
            .map(|(idx, rec)| {
                let is_selected = idx == self.selected_recording_index;
                let style = if is_selected {
                    selected_style()
                } else {
                    Style::default().fg(TEXT_COLOR)
                };

                let prefix = if is_selected { "▶ " } else { "  " };

                let duration = if rec.end_time > rec.start_time {
                    let secs = rec.end_time - rec.start_time;
                    if secs < 60 {
                        format!("{}s", secs)
                    } else if secs < 3600 {
                        format!("{}m {}s", secs / 60, secs % 60)
                    } else {
                        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
                    }
                } else {
                    "recording...".to_string()
                };

                let age = if rec.end_time > 0 && now > rec.end_time {
                    let elapsed = now - rec.end_time;
                    if elapsed < 60 {
                        format!("{}s ago", elapsed)
                    } else if elapsed < 3600 {
                        format!("{}m ago", elapsed / 60)
                    } else if elapsed < 86400 {
                        format!("{}h ago", elapsed / 3600)
                    } else {
                        format!("{}d ago", elapsed / 86400)
                    }
                } else {
                    "now".to_string()
                };

                let content = format!(
                    "{}{:<23} Flows: {:>5}  Duration: {:>10}  Age: {}",
                    prefix, rec.name, rec.flow_count, duration, age
                );
                ListItem::new(Line::from(Span::styled(content, style)))
            })
            .collect();

        let list = List::new(items).block(bordered_block(title));

        f.render_widget(list, area);
    }

    fn render_comparison(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let stats_text = "Run a replay comparison to see results.\n\n\
            Select a recording and replay it against current policies\n\
            to compare flow verdicts.";

        let stats = Paragraph::new(stats_text)
            .style(Style::default().fg(TEXT_COLOR))
            .block(bordered_block("Comparison"));

        f.render_widget(stats, area);
    }

    fn render_time_travel<M: MapReader>(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        replay: Option<&ReplayEngine<M>>,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(10), // Timeline & Controls
                Constraint::Length(12), // Network State at Current Time
                Constraint::Min(8),     // Flow Events
            ])
            .split(area);

        // Timeline & Controls
        self.render_timeline(f, chunks[0], replay);

        // Network State
        self.render_network_state(f, chunks[1]);

        // Flow Events
        self.render_flow_events(f, chunks[2]);
    }

    fn render_timeline<M: MapReader>(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        replay: Option<&ReplayEngine<M>>,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6), // Header & Info
                Constraint::Length(4), // Timeline bar
            ])
            .split(area);

        // Use loaded flows when available, otherwise fall back to recording metadata
        let (rec_name, flow_count) = if !self.loaded_flows.is_empty() {
            (self.loaded_recording_name.as_str(), self.loaded_flows.len())
        } else {
            let selected =
                replay.and_then(|engine| engine.recordings().get(self.selected_recording_index));
            match selected {
                Some(rec) => (rec.name.as_str(), rec.flow_count),
                None => ("(none)", 0),
            }
        };

        let current_flow = if flow_count > 0 {
            (self.timeline_position * flow_count) / 100
        } else {
            0
        };

        // Header
        let playback_icon = if self.is_playing { "▶️" } else { "⏸️" };
        let header_text = format!(
            "⏱️  Time-Travel Debugging\n\n\
            Recording:     {} ({} flows)\n\
            Time:          {}% ({}/{})\n\
            Status:        {}  Speed: {}x",
            rec_name,
            flow_count,
            self.timeline_position,
            current_flow,
            flow_count,
            playback_icon,
            self.playback_speed
        );

        let header = Paragraph::new(header_text)
            .style(Style::default().fg(INFO_COLOR))
            .block(bordered_block("Time-Travel Controls"));

        f.render_widget(header, chunks[0]);

        // Timeline Bar with Event Markers
        let timeline_text = self.create_timeline_bar();

        let timeline = Paragraph::new(timeline_text)
            .style(Style::default().fg(TEXT_COLOR))
            .block(bordered_block("Timeline [←/→: Step | [/]: Jump Events | Space: Play/Pause | +/-: Speed | Esc: Exit]"));

        f.render_widget(timeline, chunks[1]);
    }

    fn create_timeline_bar(&self) -> String {
        let bar_width = 70;
        let position = (self.timeline_position as f32 / 100.0 * bar_width as f32) as usize;

        // Event markers (drops at specific positions)
        let events = [10, 25, 45, 60, 80]; // Example event positions

        let mut bar = String::new();
        bar.push_str("Time:  ");

        for i in 0..bar_width {
            if i == position {
                bar.push('█'); // Current position
            } else if events.contains(&(i * 100 / bar_width)) {
                bar.push('!'); // Event marker
            } else if i < position {
                bar.push('─'); // Completed
            } else {
                bar.push('·'); // Future
            }
        }

        bar.push_str("\n       ");
        bar.push_str("0%");
        bar.push_str(&" ".repeat(bar_width - 8));
        bar.push_str("100%");

        bar
    }

    fn render_network_state(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let state_text = if !self.loaded_flows.is_empty() {
            // Compute real stats from loaded flows up to the current timeline position
            let total = self.loaded_flows.len();
            let flows_up_to = if total > 0 {
                ((self.timeline_position as f64 / 100.0) * total as f64).ceil() as usize
            } else {
                0
            }
            .min(total);

            let visible = &self.loaded_flows[..flows_up_to];

            let allow_count = visible
                .iter()
                .filter(|f| f.verdict == PolicyVerdict::Allow)
                .count();
            let deny_count = visible
                .iter()
                .filter(|f| f.verdict == PolicyVerdict::Deny)
                .count();

            let mut src_endpoints: Vec<&str> =
                visible.iter().map(|f| f.src_namespace.as_str()).collect();
            let mut dst_endpoints: Vec<&str> =
                visible.iter().map(|f| f.dst_namespace.as_str()).collect();
            src_endpoints.sort_unstable();
            src_endpoints.dedup();
            dst_endpoints.sort_unstable();
            dst_endpoints.dedup();

            let mut all_ns: Vec<&str> = src_endpoints
                .iter()
                .chain(dst_endpoints.iter())
                .copied()
                .collect();
            all_ns.sort_unstable();
            all_ns.dedup();

            format!(
                "Network State Snapshot\n\n\
                Flows (up to {}%):     {}/{}\n\
                Verdicts:              Allow: {}  Deny: {}\n\
                Source endpoints:      {}\n\
                Destination endpoints: {}\n\
                Namespaces:            {}",
                self.timeline_position,
                flows_up_to,
                total,
                allow_count,
                deny_count,
                src_endpoints.len(),
                dst_endpoints.len(),
                all_ns.join(", "),
            )
        } else {
            format!(
                "Network State Snapshot\n\n\
                Timeline Position:     {}%\n\
                Playback Speed:        {}x\n\
                Playing:               {}\n\n\
                Load a recording and enter time-travel mode to view\n\
                state snapshots at each point in time.",
                self.timeline_position,
                self.playback_speed,
                if self.is_playing { "Yes" } else { "No" },
            )
        };

        let state = Paragraph::new(state_text)
            .style(Style::default().fg(SUCCESS_COLOR))
            .block(bordered_block("Network State Snapshot"))
            .wrap(Wrap { trim: false });

        f.render_widget(state, area);
    }

    fn render_flow_events(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        if self.loaded_flows.is_empty() {
            let msg = Paragraph::new(
                "Load a recording to view flow events.\n\n\
                Select a recording from the list and enter time-travel mode\n\
                to step through individual flow events.",
            )
            .style(Style::default().fg(TEXT_COLOR))
            .block(bordered_block("Flow Events (Around Current Time)"));

            f.render_widget(msg, area);
            return;
        }

        let total = self.loaded_flows.len();
        let current_idx = if total > 0 {
            ((self.timeline_position as f64 / 100.0) * (total.saturating_sub(1)) as f64) as usize
        } else {
            0
        }
        .min(total.saturating_sub(1));

        // Show 5 flows centered around the current index
        let window_half = 2usize;
        let start = current_idx.saturating_sub(window_half);
        let end = (current_idx + window_half + 1).min(total);

        let items: Vec<ListItem> = self.loaded_flows[start..end]
            .iter()
            .enumerate()
            .map(|(i, flow)| {
                let flow_num = start + i;
                let is_current = flow_num == current_idx;

                let proto = match flow.protocol {
                    1 => "ICMP",
                    6 => "TCP",
                    17 => "UDP",
                    58 => "ICMPv6",
                    _ => "OTHER",
                };

                let verdict_str = match flow.verdict {
                    PolicyVerdict::Allow => "Allow",
                    PolicyVerdict::Deny => "Deny",
                    PolicyVerdict::Redirect => "Redirect",
                    PolicyVerdict::Audit => "Audit",
                };

                let src_pod = flow
                    .src_labels
                    .get("app")
                    .or_else(|| flow.src_labels.get("k8s:app"))
                    .cloned()
                    .unwrap_or_else(|| flow.src_ip.to_string());

                let dst_pod = flow
                    .dst_labels
                    .get("app")
                    .or_else(|| flow.dst_labels.get("k8s:app"))
                    .cloned()
                    .unwrap_or_else(|| flow.dst_ip.to_string());

                let prefix = if is_current { "\u{25b6} " } else { "  " };

                let content = format!(
                    "{}#{} {}/{} -> {}/{}:{} [{}] {}",
                    prefix,
                    flow_num,
                    flow.src_namespace,
                    src_pod,
                    flow.dst_namespace,
                    dst_pod,
                    flow.dst_port,
                    verdict_str,
                    proto,
                );

                let verdict_color = match flow.verdict {
                    PolicyVerdict::Allow => SUCCESS_COLOR,
                    PolicyVerdict::Deny => ERROR_COLOR,
                    PolicyVerdict::Redirect => INFO_COLOR,
                    PolicyVerdict::Audit => WARNING_COLOR,
                };

                let style = if is_current {
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
            "Flow Events (Around Current Time) [{}/{}]",
            current_idx + 1,
            total
        );
        let list = List::new(items).block(bordered_block(&title));

        f.render_widget(list, area);
    }
}
