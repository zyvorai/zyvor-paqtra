// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// AutoPolicy View - Zero-Trust Policy Learning
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{Gauge, List, ListItem, Paragraph},
    Frame,
};

use super::theme::*;
use crate::ebpf::MapReader;
use crate::modules::autopolicy::AutoPolicy;

pub struct AutoPolicyView;

impl AutoPolicyView {
    pub fn new() -> Self {
        Self
    }

    pub fn render<M: MapReader>(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        autopolicy: Option<&AutoPolicy<M>>,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(10), // Header & Progress
                Constraint::Min(8),     // Learned Patterns
                Constraint::Length(8),  // Generated Policies
            ])
            .split(area);

        // Header
        self.render_header(f, chunks[0], autopolicy);

        // Learned Patterns
        self.render_patterns(f, chunks[1], autopolicy);

        // Generated Policies
        self.render_policies(f, chunks[2], autopolicy);
    }

    fn render_header<M: MapReader>(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        autopolicy: Option<&AutoPolicy<M>>,
    ) {
        let stats = if let Some(engine) = autopolicy {
            let s = engine.stats();
            format!(
                "🤖 Zero-Trust Policy Learning\n\n\
                State:             {:?}\n\
                Total Observations:{}\n\
                Unique Patterns:   {}\n\
                Policies Generated:{}\n\
                Confidence:        {:.1}%\n\n\
                Recording all traffic patterns...",
                s.state,
                s.total_observations,
                s.unique_patterns,
                s.policies_generated,
                s.avg_confidence * 100.0,
            )
        } else {
            "AutoPolicy not initialized".to_string()
        };

        let content = Paragraph::new(stats)
            .style(Style::default().fg(INFO_COLOR))
            .block(bordered_block("Learning Status"));

        f.render_widget(content, area);
    }

    fn render_patterns<M: MapReader>(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        autopolicy: Option<&AutoPolicy<M>>,
    ) {
        let items: Vec<ListItem> = if let Some(engine) = autopolicy {
            let observations = engine.observations();
            let mut obs_vec: Vec<_> = observations.iter().collect();
            obs_vec.sort_by_key(|a| std::cmp::Reverse(a.1.count)); // Sort by count descending

            obs_vec
                .iter()
                .take(10)
                .map(|(pattern, obs)| {
                    // Get source labels
                    let src_label = pattern
                        .src_labels
                        .get("app")
                        .map(|s| s.as_str())
                        .unwrap_or("unknown");

                    // Get dest labels
                    let dst_label = pattern
                        .dst_labels
                        .get("app")
                        .map(|s| s.as_str())
                        .unwrap_or("unknown");

                    // Format pattern
                    let pattern_str = format!(
                        "{}/{}[app={}] → {}/{}[app={}]",
                        pattern.src_namespace,
                        if src_label != "unknown" {
                            src_label
                        } else {
                            "?"
                        },
                        src_label,
                        pattern.dst_namespace,
                        if dst_label != "unknown" {
                            dst_label
                        } else {
                            "?"
                        },
                        dst_label
                    );

                    // Calculate confidence based on observation count
                    let confidence = ((obs.count as f32).min(100.0) / 100.0 * 100.0) as u32;

                    // Color based on confidence
                    let color = confidence_color(confidence as f64 / 100.0);

                    let content = format!(
                        "{:<50} {:>6}:{:<3} {:>4} obs  Conf: {:>3}%",
                        pattern_str,
                        pattern.port,
                        pattern.protocol.to_string(),
                        obs.count,
                        confidence
                    );

                    ListItem::new(Line::from(Span::styled(
                        content,
                        Style::default().fg(color),
                    )))
                })
                .collect()
        } else {
            vec![ListItem::new("No patterns learned yet")]
        };

        let title = if let Some(engine) = autopolicy {
            format!(
                "Communication Patterns ({} unique)",
                engine.observations().len()
            )
        } else {
            "Communication Patterns".to_string()
        };

        let list = List::new(items).block(bordered_block(&title));

        f.render_widget(list, area);
    }

    fn render_policies<M: MapReader>(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        autopolicy: Option<&AutoPolicy<M>>,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(area);

        // Policy List
        let policy_text = if let Some(engine) = autopolicy {
            let policies = engine.policies();
            if policies.is_empty() {
                "📋 Generated Policies:\n\n\
                No policies generated yet.\n\
                Waiting for sufficient observations...\n\n\
                Press 'g' to generate policies from learned patterns."
                    .to_string()
            } else {
                let mut text = "📋 Generated Policies (ML-Enhanced):\n\n".to_string();
                for (idx, policy) in policies.iter().take(5).enumerate() {
                    // Use ML-based confidence scoring
                    use crate::modules::autopolicy::confidence::ConfidenceScorer;

                    let confidence_pct = policy.confidence * 100.0;
                    let confidence_level = ConfidenceScorer::confidence_level(policy.confidence);
                    let recommendation = ConfidenceScorer::get_recommendation(policy.confidence);

                    text.push_str(&format!(
                        "{}. {} (namespace: {})\n   Confidence: {:.0}% ({})\n   Patterns: {}  |  {}\n",
                        idx + 1,
                        policy.name,
                        policy.namespace,
                        confidence_pct,
                        confidence_level,
                        policy.patterns.len(),
                        recommendation
                    ));
                }
                if policies.len() > 5 {
                    text.push_str(&format!("\n... and {} more policies", policies.len() - 5));
                }
                text
            }
        } else {
            "AutoPolicy not initialized".to_string()
        };

        let policies = Paragraph::new(policy_text)
            .style(Style::default().fg(TEXT_COLOR))
            .block(bordered_block("Generated Policies"));

        f.render_widget(policies, chunks[0]);

        // Readiness Gauge
        let (readiness, gauge_color) = if let Some(engine) = autopolicy {
            let stats = engine.stats();
            let readiness = stats.avg_confidence;
            let color = confidence_color(readiness as f64);
            (readiness, color)
        } else {
            (0.0, UNKNOWN_STATUS_COLOR)
        };

        let gauge = Gauge::default()
            .block(bordered_block("Confidence"))
            .gauge_style(Style::default().fg(gauge_color))
            .percent(((readiness * 100.0) as u16).min(100))
            .label(format!("{:.0}%", readiness * 100.0));

        f.render_widget(gauge, chunks[1]);
    }
}
