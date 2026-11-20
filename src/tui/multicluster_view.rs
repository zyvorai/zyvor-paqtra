// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// Multi-Cluster Autopilot View - Global Orchestration
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::theme::*;
use crate::modules::multicluster::{ClusterState, MultiClusterAutopilot};

pub struct MultiClusterView {
    pub selected_cluster_index: usize,
    pub view_mode: ViewMode,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ViewMode {
    Clusters,   // List of clusters
    Topology,   // Cluster topology map
    Syncs,      // Active policy syncs
    Placements, // Workload placements
}

impl MultiClusterView {
    pub fn new() -> Self {
        Self {
            selected_cluster_index: 0,
            view_mode: ViewMode::Clusters,
        }
    }

    pub fn move_selection_up(&mut self) {
        move_selection_up(&mut self.selected_cluster_index);
    }

    pub fn move_selection_down(&mut self, max: usize) {
        move_selection_down(&mut self.selected_cluster_index, max);
    }

    pub fn cycle_view(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Clusters => ViewMode::Topology,
            ViewMode::Topology => ViewMode::Syncs,
            ViewMode::Syncs => ViewMode::Placements,
            ViewMode::Placements => ViewMode::Clusters,
        };
    }

    pub fn render(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        autopilot: Option<&MultiClusterAutopilot>,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(9), // Header
                Constraint::Min(10),   // Main View
                Constraint::Length(8), // Details
            ])
            .split(area);

        // Header
        self.render_header(f, chunks[0], autopilot);

        // Main view based on mode
        match self.view_mode {
            ViewMode::Clusters => {
                self.render_clusters(f, chunks[1], autopilot);
                self.render_cluster_details(f, chunks[2], autopilot);
            }
            ViewMode::Topology => {
                self.render_topology(f, chunks[1], autopilot);
                self.render_topology_stats(f, chunks[2], autopilot);
            }
            ViewMode::Syncs => {
                self.render_syncs(f, chunks[1], autopilot);
                self.render_sync_details(f, chunks[2]);
            }
            ViewMode::Placements => {
                self.render_placements(f, chunks[1], autopilot);
                self.render_placement_details(f, chunks[2], autopilot);
            }
        }
    }

    fn render_header(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        autopilot: Option<&MultiClusterAutopilot>,
    ) {
        let view_name = match self.view_mode {
            ViewMode::Clusters => "Cluster List",
            ViewMode::Topology => "Topology Map",
            ViewMode::Syncs => "Policy Syncs",
            ViewMode::Placements => "Workload Placements",
        };

        let (total, active, degraded, syncs, regions_str) = if let Some(engine) = autopilot {
            let stats = engine.stats();
            let topology = engine.get_topology();
            let regions: Vec<String> = topology.regions.keys().cloned().collect();
            let r_str = if regions.is_empty() {
                "0".to_string()
            } else {
                format!("{} ({})", regions.len(), regions.join(", "))
            };
            (
                stats.total_clusters,
                stats.active_clusters,
                stats.degraded_clusters,
                stats.active_syncs,
                r_str,
            )
        } else {
            (0, 0, 0, 0, "0".to_string())
        };

        let header_text = format!(
            "  Multi-Cluster Autopilot\n\n\
            View:              {}\n\
            Total Clusters:    {}\n\
            Active:            {}\n\
            Degraded:          {}\n\
            Active Syncs:      {}\n\
            Regions:           {}\n\n\
            Press 'v' to cycle views | 'h' to run health check",
            view_name, total, active, degraded, syncs, regions_str
        );

        let header = Paragraph::new(header_text)
            .style(Style::default().fg(INFO_COLOR))
            .block(bordered_block("Autopilot Status"));

        f.render_widget(header, area);
    }

    fn render_clusters(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        autopilot: Option<&MultiClusterAutopilot>,
    ) {
        let items: Vec<ListItem> = if let Some(engine) = autopilot {
            let mut clusters: Vec<_> = engine.clusters().values().collect();
            clusters.sort_by(|a, b| a.name.cmp(&b.name));

            clusters
                .iter()
                .enumerate()
                .map(|(idx, cluster)| {
                    let is_selected = idx == self.selected_cluster_index;
                    let state_str = match cluster.state {
                        ClusterState::Active => "Active",
                        ClusterState::Degraded => "Degraded",
                        ClusterState::Unreachable => "Unreachable",
                        ClusterState::Syncing => "Syncing",
                        ClusterState::Draining => "Draining",
                    };
                    let color = match cluster.state {
                        ClusterState::Active => SUCCESS_COLOR,
                        ClusterState::Degraded => WARNING_COLOR,
                        ClusterState::Unreachable => ERROR_COLOR,
                        _ => INFO_COLOR,
                    };
                    let style = if is_selected {
                        selected_style()
                    } else {
                        Style::default().fg(color)
                    };

                    let prefix = if is_selected { "> " } else { "  " };
                    let content = format!(
                        "{}{:<20} {:<6} {:<15} [{}]",
                        prefix,
                        cluster.name,
                        cluster.provider.to_string(),
                        cluster.region,
                        state_str,
                    );
                    ListItem::new(Line::from(Span::styled(content, style)))
                })
                .collect()
        } else {
            vec![]
        };

        if items.is_empty() {
            let empty = vec![ListItem::new(Line::from(Span::styled(
                "  No clusters registered. Use the API to register clusters.",
                Style::default().fg(UNKNOWN_STATUS_COLOR),
            )))];
            let list = List::new(empty).block(bordered_block(
                "Managed Clusters [Up/Down: Select | v: Cycle View | h: Health Check]",
            ));
            f.render_widget(list, area);
        } else {
            let list = List::new(items).block(bordered_block(
                "Managed Clusters [Up/Down: Select | v: Cycle View | h: Health Check]",
            ));
            f.render_widget(list, area);
        }
    }

    fn render_cluster_details(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        autopilot: Option<&MultiClusterAutopilot>,
    ) {
        let details = if let Some(engine) = autopilot {
            let mut clusters: Vec<_> = engine.clusters().values().collect();
            clusters.sort_by(|a, b| a.name.cmp(&b.name));

            if let Some(cluster) = clusters.get(self.selected_cluster_index) {
                let connected = cluster.connectivity.connected_clusters.len();
                format!(
                    "Cluster: {}\n\n\
                    Provider:        {}\n\
                    Region:          {}\n\
                    Nodes:           {}/{} healthy\n\
                    Pods:            {}\n\
                    CPU Usage:       {:.0}%\n\
                    Memory Usage:    {:.0}%\n\
                    Network:         {} Connected to {} cluster(s)",
                    cluster.name,
                    cluster.provider,
                    cluster.region,
                    cluster.health.healthy_nodes,
                    cluster.health.node_count,
                    cluster.health.pod_count,
                    cluster.health.cpu_usage_pct,
                    cluster.health.memory_usage_pct,
                    if cluster.health.network_ok {
                        "OK"
                    } else {
                        "DEGRADED"
                    },
                    connected,
                )
            } else {
                "No cluster selected".to_string()
            }
        } else {
            "Autopilot engine not available".to_string()
        };

        let paragraph = Paragraph::new(details)
            .style(Style::default().fg(TEXT_COLOR))
            .block(bordered_block("Cluster Details"));

        f.render_widget(paragraph, area);
    }

    fn render_topology(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        autopilot: Option<&MultiClusterAutopilot>,
    ) {
        let topology_text = if let Some(engine) = autopilot {
            let topo = engine.get_topology();
            let mut lines = vec![format!(
                "  Global Cluster Topology ({} clusters, {} connected pairs)\n",
                topo.total_clusters, topo.connected_pairs
            )];

            for (region, clusters) in &topo.regions {
                lines.push(format!("  Region: {} -> [{}]", region, clusters.join(", ")));
            }

            if topo.total_clusters == 0 {
                lines.push("  No clusters registered yet.".to_string());
            }

            lines.join("\n")
        } else {
            "Autopilot engine not available".to_string()
        };

        let paragraph = Paragraph::new(topology_text)
            .style(Style::default().fg(INFO_COLOR))
            .block(bordered_block("Network Topology"))
            .wrap(Wrap { trim: false });

        f.render_widget(paragraph, area);
    }

    fn render_topology_stats(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        autopilot: Option<&MultiClusterAutopilot>,
    ) {
        let stats = if let Some(engine) = autopilot {
            let topo = engine.get_topology();
            let stats = engine.stats();
            format!(
                "Topology Statistics\n\n\
                Connected Pairs:    {}\n\
                Total Clusters:     {}\n\
                Active Clusters:    {}\n\
                Degraded:           {}\n\
                Regions:            {}",
                topo.connected_pairs,
                stats.total_clusters,
                stats.active_clusters,
                stats.degraded_clusters,
                topo.regions.len(),
            )
        } else {
            "No data available".to_string()
        };

        let paragraph = Paragraph::new(stats)
            .style(Style::default().fg(TEXT_COLOR))
            .block(bordered_block("Stats"));

        f.render_widget(paragraph, area);
    }

    fn render_syncs(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        _autopilot: Option<&MultiClusterAutopilot>,
    ) {
        // Syncs are tracked internally; expose via stats for now
        let items = vec![ListItem::new(Line::from(Span::styled(
            "  No active syncs. Use 's' to trigger a policy sync.",
            Style::default().fg(UNKNOWN_STATUS_COLOR),
        )))];

        let list = List::new(items).block(bordered_block("Active Policy Syncs"));

        f.render_widget(list, area);
    }

    fn render_sync_details(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let details = "Sync Details\n\n\
            No active sync selected.\n\
            Switch to Clusters view and register clusters first,\n\
            then trigger policy syncs with 's'.";

        let paragraph = Paragraph::new(details)
            .style(Style::default().fg(TEXT_COLOR))
            .block(bordered_block("Sync Details"));

        f.render_widget(paragraph, area);
    }

    fn render_placements(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        _autopilot: Option<&MultiClusterAutopilot>,
    ) {
        let items = vec![ListItem::new(Line::from(Span::styled(
            "  No placement recommendations yet. Register clusters first.",
            Style::default().fg(UNKNOWN_STATUS_COLOR),
        )))];

        let list = List::new(items).block(bordered_block("Placement Recommendations"));

        f.render_widget(list, area);
    }

    fn render_placement_details(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        _autopilot: Option<&MultiClusterAutopilot>,
    ) {
        let details = "Placement Details\n\n\
            No placement selected.\n\
            Register clusters and run health checks to get\n\
            intelligent workload placement recommendations.";

        let paragraph = Paragraph::new(details)
            .style(Style::default().fg(TEXT_COLOR))
            .block(bordered_block("Placement Details"));

        f.render_widget(paragraph, area);
    }
}
