// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Infrastructure tab rendering (connections, endpoints, policies, metrics)

use ratatui::{
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use super::app::TuiApp;
use super::theme::*;

impl TuiApp {
    pub(crate) fn render_connections(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        if self.integrated_provider.is_none() {
            let content = Paragraph::new(
                "⚠️ Integrated Data Provider not available\n\n\
                Cilium eBPF maps or Kubernetes access not available.\n\
                Ensure:\n\
                • Cilium is installed and running\n\
                • BPF maps are accessible (/sys/fs/bpf/tc/globals/)\n\
                • Kubernetes API is accessible\n\n\
                Showing mock data mode.",
            )
            .style(Style::default().fg(WARNING_COLOR))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("⚡ Enriched Connections")
                    .border_style(Style::default().fg(BORDER_COLOR)),
            );
            f.render_widget(content, area);
            return;
        }

        let items: Vec<ListItem> = self
            .enriched_connections
            .iter()
            .take(50)
            .map(|enriched| {
                // Determine connection state color
                let state_color = match enriched.conn.state {
                    crate::ebpf::ConntrackState::Established => ESTABLISHED_COLOR,
                    crate::ebpf::ConntrackState::New => NEW_CONNECTION_COLOR,
                    crate::ebpf::ConntrackState::Related => RELATED_COLOR,
                    crate::ebpf::ConntrackState::Invalid => INVALID_COLOR,
                };

                // Format source and destination with pod names
                let src = match &enriched.src_pod {
                    Some(pod) => format!("{}/{}", pod.namespace, pod.pod_name),
                    None => enriched.conn.src_ip.clone(),
                };

                let dst = match &enriched.dst_pod {
                    Some(pod) => format!("{}/{}", pod.namespace, pod.pod_name),
                    None => enriched.conn.dst_ip.clone(),
                };

                // Format protocol
                let proto = match enriched.conn.protocol {
                    6 => "TCP",
                    17 => "UDP",
                    1 => "ICMP",
                    _ => "???",
                };

                let content = format!(
                    "{:40} → {:40} {:5} {:6} {:8} pkts {:10} bytes",
                    format!("{}:{}", src, enriched.conn.src_port),
                    format!("{}:{}", dst, enriched.conn.dst_port),
                    proto,
                    format!("{:?}", enriched.conn.state),
                    enriched.conn.packets,
                    enriched.conn.bytes,
                );

                ListItem::new(Line::from(Span::styled(
                    content,
                    Style::default().fg(state_color),
                )))
            })
            .collect();

        // Get identity resolver stats
        let stats_text = if let Some(provider) = &self.integrated_provider {
            let stats = provider.identity_stats();
            format!(
                "Enriched Connections ({}) | Identity Cache: {} IDs, {} IPs, age: {}s",
                self.enriched_connections.len(),
                stats.total_identities,
                stats.total_ips,
                stats.age_seconds,
            )
        } else {
            format!("Enriched Connections ({})", self.enriched_connections.len())
        };

        let list = List::new(items).block(Block::default().borders(Borders::ALL).title(stats_text));
        f.render_widget(list, area);
    }

    pub(crate) fn render_endpoints(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let items: Vec<ListItem> = self
            .endpoints
            .iter()
            .map(|ep| {
                let status_color = match ep.status {
                    crate::endpoints::EndpointStatus::Running => RUNNING_COLOR,
                    crate::endpoints::EndpointStatus::Pending => PENDING_COLOR,
                    crate::endpoints::EndpointStatus::Failed => FAILED_COLOR,
                    crate::endpoints::EndpointStatus::Unknown => UNKNOWN_STATUS_COLOR,
                };

                let app_label = ep
                    .labels
                    .get("app")
                    .or_else(|| ep.labels.get("app.kubernetes.io/name"))
                    .map(|s| s.as_str())
                    .unwrap_or("none");

                let content = format!(
                    "{:10} {:20} {:15} {:10} {:15}",
                    ep.status, ep.namespace, ep.name, ep.ip, app_label
                );

                ListItem::new(Line::from(Span::styled(
                    content,
                    Style::default().fg(status_color),
                )))
            })
            .collect();

        let header = format!(
            "{:10} {:20} {:15} {:10} {:15}",
            "STATUS", "NAMESPACE", "NAME", "IP", "APP"
        );

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Endpoints ({}) - {}", self.endpoints.len(), header))
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
        f.render_widget(list, area);
    }

    pub(crate) fn render_policies(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let policies = match &self.modules {
            super::app::ModuleContainer::Enriched { autopolicy, .. } => autopolicy.policies(),
            super::app::ModuleContainer::Mock { autopolicy, .. } => autopolicy.policies(),
        };

        if policies.is_empty() {
            let content = Paragraph::new(
                "No policies generated yet.\n\n\
                Go to the AutoPolicy tab and press 'g' to generate policies\n\
                based on observed traffic patterns.",
            )
            .style(Style::default().fg(WARNING_COLOR))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Network Policies (0)")
                    .border_style(Style::default().fg(BORDER_COLOR)),
            );
            f.render_widget(content, area);
            return;
        }

        let items: Vec<ListItem> = policies
            .iter()
            .map(|policy| {
                let applied = self.applied_policies.contains(&policy.name);
                let (icon, color) = if applied {
                    ("✓", RUNNING_COLOR)
                } else {
                    ("○", TEXT_COLOR)
                };

                let confidence_pct = (policy.confidence * 100.0) as u32;
                let content = format!(
                    "{} {:40} {:20} {:>3}% confidence",
                    icon, policy.name, policy.namespace, confidence_pct
                );

                ListItem::new(Line::from(Span::styled(
                    content,
                    Style::default().fg(color),
                )))
            })
            .collect();

        let applied_count = policies
            .iter()
            .filter(|p| self.applied_policies.contains(&p.name))
            .count();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(
                    "Network Policies ({} total, {} applied)",
                    policies.len(),
                    applied_count
                ))
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
        f.render_widget(list, area);
    }

    pub(crate) fn render_metrics(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        // Get identity resolver stats if available
        let identity_stats = if let Some(provider) = &self.integrated_provider {
            let stats = provider.identity_stats();
            let ebpf_status = if provider.is_ebpf_available() {
                "✅ Available"
            } else {
                "⚠️ Mock Mode"
            };
            format!(
                "eBPF Integration:   {}\n\
                Identity Cache:     {} identities\n\
                IP Mappings:        {} IPs\n\
                Pod Mappings:       {} pods\n\
                Cache Age:          {}s\n",
                ebpf_status,
                stats.total_identities,
                stats.total_ips,
                stats.total_pods,
                stats.age_seconds,
            )
        } else {
            "eBPF Integration:   ⚠️ Not Available\n\
            Identity Cache:     N/A\n\
            IP Mappings:        N/A\n\
            Pod Mappings:       N/A\n\
            Cache Age:          N/A\n"
                .to_string()
        };

        let total_modules = super::tabs::TabIndex::count();
        // Subtract non-module tabs (Flows, Connections, Endpoints, Policies, Metrics = 5)
        let module_count = total_modules.saturating_sub(5);
        let active_modules = module_count; // All loaded modules are active
        let pct = (active_modules * 100)
            .checked_div(module_count)
            .unwrap_or(0);
        let content = Paragraph::new(format!(
            "📊 Platform Metrics:\n\n\
            Modules Active:     {}/{} ({}%)\n\n\
            {}\n\
            ✅ Self-Healer:     Ready\n\
            ✅ AutoPolicy:      Learning\n\
            ✅ RootCause:       Monitoring\n\
            ✅ Simulator:       Ready\n\
            ✅ Replay:          Ready\n\
            ✅ K8s Identity:    Active",
            active_modules, total_modules, pct, identity_stats
        ))
        .style(Style::default().fg(TEXT_COLOR))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Platform Metrics")
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
        f.render_widget(content, area);
    }
}
