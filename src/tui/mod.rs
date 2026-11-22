// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    Terminal,
};
use std::io;
use std::time::Duration;

mod app;
mod events;
mod handlers;
mod render;
mod render_flows;
mod render_infra;
mod render_modules;
pub mod tabs;

mod autopolicy_view;
mod canary_view;
mod chaos_view;
mod help_overlay;
mod multicluster_view;
mod replay_view;
mod rootcause_view;
mod simulator_view;
mod theme;

pub use app::TuiApp;
#[allow(unused_imports)]
pub use tabs::TabIndex;

use app::ModuleContainer;

impl TuiApp {
    pub async fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Install a panic hook that restores the terminal before printing the panic
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
            original_hook(panic_info);
        }));

        let res = self.run_app(&mut terminal).await;

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        if let Err(err) = res {
            println!("Error: {:?}", err);
        }

        Ok(())
    }

    async fn run_app<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()>
    where
        B::Error: Send + Sync + 'static,
    {
        let mut flow_fetch_timer = std::time::Instant::now();
        loop {
            // Clear expired status messages (older than 5 seconds)
            if self.status_message.is_some() && self.status_message_time.elapsed().as_secs() >= 5 {
                self.status_message = None;
            }

            // Fetch latest flows every 2 seconds instead of every 250ms loop iteration
            if flow_fetch_timer.elapsed().as_secs() >= 2 {
                match self.hubble_client.get_flows().await {
                    Ok(flows) => {
                        self.flows = flows;
                        if self.flows.is_empty() {
                            self.selected_flow_index = 0;
                        } else if self.selected_flow_index >= self.flows.len() {
                            self.selected_flow_index = self.flows.len() - 1;
                        }
                    }
                    Err(e) => {
                        self.set_status_message(&format!("Flow fetch failed: {}", e));
                    }
                }
                flow_fetch_timer = std::time::Instant::now();
            }

            // Update module data based on selected tab
            match self.selected_tab {
                1 => {
                    // Connections - enriched with K8s data
                    if let Some(provider) = &self.integrated_provider {
                        if let Ok(connections) = provider.get_enriched_connections().await {
                            self.enriched_connections = connections;
                        }
                    }
                }
                2 => {
                    // Endpoints
                    if let Ok(endpoints) = self.endpoint_manager.discover_endpoints().await {
                        self.endpoints = endpoints;
                    }
                }
                5 => {
                    // Healer - active problem detection (every 30 seconds)
                    if self.last_healer_run.elapsed().as_secs() >= 30 {
                        match &mut self.modules {
                            ModuleContainer::Enriched { healer, .. } => {
                                if let Err(e) = healer.run_enriched().await {
                                    tracing::warn!("Healer enriched run failed: {}", e);
                                }
                            }
                            ModuleContainer::Mock { healer, .. } => {
                                if let Err(e) = healer.run().await {
                                    tracing::warn!("Healer run failed: {}", e);
                                }
                            }
                        }
                        self.last_healer_run = std::time::Instant::now();
                    }
                }
                6 => {
                    // AutoPolicy - active learning (every 5 minutes)
                    if self.last_autopolicy_update.elapsed().as_secs() >= 300 {
                        match &mut self.modules {
                            ModuleContainer::Enriched { autopolicy, .. } => {
                                if let Err(e) = autopolicy.update_enriched().await {
                                    tracing::warn!("AutoPolicy enriched update failed: {}", e);
                                }
                            }
                            ModuleContainer::Mock { autopolicy, .. } => {
                                if let Err(e) = autopolicy.update().await {
                                    tracing::warn!("AutoPolicy update failed: {}", e);
                                }
                            }
                        }
                        self.last_autopolicy_update = std::time::Instant::now();
                    }
                }
                7 => {
                    // RootCause - updates on-demand
                }
                8 => {
                    // Simulator - run simulation when triggered by user
                    if self.simulator_view.should_simulate {
                        self.simulator_view.should_simulate = false;
                        // Run simulation using the simulator module
                        // For now just mark as not running since the simulator needs wiring
                        self.simulator_view.simulation_running = false;
                    }
                }
                9 => {
                    // Replay - updates on-demand
                }
                _ => {}
            }

            terminal.draw(|f| self.ui(f))?;

            // Handle input
            if event::poll(Duration::from_millis(250))? {
                if let Event::Key(key) = event::read()? {
                    // Handle synchronous key events
                    match events::handle_key_event(self, key.code) {
                        events::KeyAction::Quit => return Ok(()),
                        events::KeyAction::Continue => {}
                    }

                    // Handle async key events (healer detect, autopolicy update)
                    events::handle_key_event_async(self, key.code).await;
                }
            }
        }
    }
}
