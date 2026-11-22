// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

use super::app::{ModuleContainer, TuiApp};
use super::canary_view;
use super::handlers;
use super::tabs::TabIndex;
use crossterm::event::KeyCode;

/// Possible result from handling a key event.
pub(crate) enum KeyAction {
    /// Continue the main loop.
    Continue,
    /// Quit the application.
    Quit,
}

/// Handle a keyboard event, mutating app state as needed.
/// Returns `KeyAction::Quit` if the application should exit.
///
/// Note: Some key handlers are async because they invoke module operations
/// (healer, autopolicy, etc.). This function is kept synchronous for the
/// majority of key handlers. Async operations are handled separately in
/// `handle_key_event_async`.
pub(crate) fn handle_key_event(app: &mut TuiApp, key_code: KeyCode) -> KeyAction {
    match key_code {
        KeyCode::Char('?') => {
            // Toggle help overlay
            app.show_help = !app.show_help;
        }
        KeyCode::Esc if app.show_help => {
            // Close help overlay
            app.show_help = false;
        }
        KeyCode::Char('q') if !app.show_help => return KeyAction::Quit,
        KeyCode::Tab if !app.show_help => {
            let count = TabIndex::count();
            app.selected_tab = (app.selected_tab + 1) % count;
        }
        KeyCode::BackTab if !app.show_help => {
            let count = TabIndex::count();
            app.selected_tab = if app.selected_tab == 0 {
                count - 1
            } else {
                app.selected_tab - 1
            };
        }
        KeyCode::Char('r') if !app.show_help && app.selected_tab == 9 => {
            // Refresh recordings from disk storage
            let count = match &mut app.modules {
                ModuleContainer::Enriched { replay, .. } => {
                    replay.list_recordings().map(|r| r.len())
                }
                ModuleContainer::Mock { replay, .. } => replay.list_recordings().map(|r| r.len()),
            };
            match count {
                Ok(n) => app.set_status_message(&format!("Refreshed: {} recordings found", n)),
                Err(e) => app.set_status_message(&format!("Refresh failed: {}", e)),
            }
        }
        KeyCode::Char('t')
            if !app.show_help && app.selected_tab == 9 && !app.replay_view.time_travel_mode =>
        {
            // Enter time-travel mode and load flows from the selected recording
            app.replay_view.enter_time_travel();
            match &app.modules {
                ModuleContainer::Enriched { replay, .. } => {
                    app.replay_view.load_flows(replay);
                }
                ModuleContainer::Mock { replay, .. } => {
                    app.replay_view.load_flows(replay);
                }
            }
        }
        KeyCode::Esc
            if !app.show_help && app.selected_tab == 9 && app.replay_view.time_travel_mode =>
        {
            // Exit time-travel mode
            app.replay_view.exit_time_travel();
        }
        KeyCode::Left
            if !app.show_help && app.selected_tab == 9 && app.replay_view.time_travel_mode =>
        {
            // Step backward in timeline
            app.replay_view.step_backward();
        }
        KeyCode::Right
            if !app.show_help && app.selected_tab == 9 && app.replay_view.time_travel_mode =>
        {
            // Step forward in timeline
            app.replay_view.step_forward();
        }
        KeyCode::Char(' ')
            if !app.show_help && app.selected_tab == 9 && app.replay_view.time_travel_mode =>
        {
            // Toggle playback
            app.replay_view.toggle_playback();
        }
        KeyCode::Char('[')
            if !app.show_help && app.selected_tab == 9 && app.replay_view.time_travel_mode =>
        {
            // Jump to previous event
            app.replay_view.jump_to_prev_event();
        }
        KeyCode::Char(']')
            if !app.show_help && app.selected_tab == 9 && app.replay_view.time_travel_mode =>
        {
            // Jump to next event
            app.replay_view.jump_to_next_event();
        }
        KeyCode::Char('+')
            if !app.show_help && app.selected_tab == 9 && app.replay_view.time_travel_mode =>
        {
            // Increase playback speed
            app.replay_view.adjust_speed(true);
        }
        KeyCode::Char('-')
            if !app.show_help && app.selected_tab == 9 && app.replay_view.time_travel_mode =>
        {
            // Decrease playback speed
            app.replay_view.adjust_speed(false);
        }
        KeyCode::Up
            if !app.show_help && app.selected_tab == 9 && !app.replay_view.time_travel_mode =>
        {
            // Navigate recordings up
            app.replay_view.move_selection_up();
        }
        KeyCode::Down
            if !app.show_help && app.selected_tab == 9 && !app.replay_view.time_travel_mode =>
        {
            // Navigate recordings down
            let recordings_count = match &mut app.modules {
                ModuleContainer::Enriched { replay, .. } => {
                    replay.list_recordings().map(|r| r.len()).unwrap_or(1)
                }
                ModuleContainer::Mock { replay, .. } => {
                    replay.list_recordings().map(|r| r.len()).unwrap_or(1)
                }
            };
            app.replay_view.move_selection_down(recordings_count.max(1));
        }
        KeyCode::Char('v') if !app.show_help && app.selected_tab == 10 => {
            // Toggle between presets and active experiments
            app.chaos_view.toggle_view();
        }
        KeyCode::Up if !app.show_help && app.selected_tab == 10 => {
            // Navigate chaos experiments/presets up
            app.chaos_view.move_selection_up();
        }
        KeyCode::Down if !app.show_help && app.selected_tab == 10 => {
            // Navigate chaos experiments/presets down
            let max = if app.chaos_view.show_presets {
                super::chaos_view::CHAOS_PRESETS.len()
            } else {
                app.chaos_engine.active_experiments().len().max(1)
            };
            app.chaos_view.move_selection_down(max);
        }
        KeyCode::Enter
            if !app.show_help
                && app.selected_tab == 10
                && app.chaos_view.show_presets
                && !app.chaos_view.confirmation_mode =>
        {
            // Run chaos experiment
            app.chaos_view.trigger_confirmation();
        }
        KeyCode::Char('y')
            if !app.show_help && app.selected_tab == 10 && app.chaos_view.confirmation_mode =>
        {
            // Mark confirmation as accepted — async handler will start the experiment
            app.chaos_view.confirmed = true;
        }
        KeyCode::Char('n')
            if !app.show_help && app.selected_tab == 10 && app.chaos_view.confirmation_mode =>
        {
            // Cancel chaos experiment
            app.chaos_view.cancel_confirmation();
            app.set_status_message("Chaos experiment cancelled");
        }
        KeyCode::Char('s')
            if !app.show_help && app.selected_tab == 10 && !app.chaos_view.show_presets =>
        {
            // Stop selected experiment — handled in async handler
        }
        KeyCode::Char('S')
            if !app.show_help && app.selected_tab == 10 && !app.chaos_view.show_presets =>
        {
            // Stop all experiments — handled in async handler
        }
        KeyCode::Char('b')
            if !app.show_help
                && app.selected_tab == 10
                && !app.chaos_view.circuit_breaker_confirm =>
        {
            // Trigger circuit breaker confirmation
            app.chaos_view.circuit_breaker_confirm = true;
            app.set_status_message("Trigger circuit breaker? Press 'y' to confirm");
        }
        KeyCode::Char('y')
            if !app.show_help
                && app.selected_tab == 10
                && app.chaos_view.circuit_breaker_confirm =>
        {
            // Confirm circuit breaker
            app.chaos_view.circuit_breaker_confirm = false;
            app.chaos_engine
                .trigger_circuit_breaker("Manual trigger from TUI");
            app.set_status_message("Circuit breaker TRIGGERED - all chaos disabled");
        }
        KeyCode::Char('n')
            if !app.show_help
                && app.selected_tab == 10
                && app.chaos_view.circuit_breaker_confirm =>
        {
            app.chaos_view.circuit_breaker_confirm = false;
            app.set_status_message("Circuit breaker cancelled");
        }
        // Canary tab (11) keyboard handlers
        KeyCode::Up if !app.show_help && app.selected_tab == 11 => {
            app.canary_view.move_selection_up();
        }
        KeyCode::Down if !app.show_help && app.selected_tab == 11 => {
            let max = app.canary_engine.active_canaries().len().max(1);
            app.canary_view.move_selection_down(max);
        }
        KeyCode::Char('p') if !app.show_help && app.selected_tab == 11 => {
            app.canary_view
                .trigger_confirmation(canary_view::ConfirmationType::Promote);
        }
        KeyCode::Char('r') if !app.show_help && app.selected_tab == 11 => {
            app.canary_view
                .trigger_confirmation(canary_view::ConfirmationType::Rollback);
        }
        KeyCode::Char('+') if !app.show_help && app.selected_tab == 11 => {
            // Progress canary — handled in async handler
        }
        KeyCode::Char('d') if !app.show_help && app.selected_tab == 11 => {
            app.canary_view.toggle_details();
        }
        KeyCode::Char('y')
            if !app.show_help
                && app.selected_tab == 11
                && app.canary_view.confirmation_mode != canary_view::ConfirmationType::None =>
        {
            // Store the pending action, then clear confirmation.
            // Async handler will pick it up.
            app.canary_pending_action = Some(app.canary_view.confirmation_mode.clone());
            app.canary_view.cancel_confirmation();
        }
        KeyCode::Char('n')
            if !app.show_help
                && app.selected_tab == 11
                && app.canary_view.confirmation_mode != canary_view::ConfirmationType::None =>
        {
            app.canary_view.cancel_confirmation();
            app.set_status_message("Canary action cancelled");
        }
        // MultiCluster tab (12) keyboard handlers
        KeyCode::Up if !app.show_help && app.selected_tab == 12 => {
            app.multicluster_view.move_selection_up();
        }
        KeyCode::Down if !app.show_help && app.selected_tab == 12 => {
            let max = app.multicluster_engine.clusters().len().max(1);
            app.multicluster_view.move_selection_down(max);
        }
        KeyCode::Char('v') if !app.show_help && app.selected_tab == 12 => {
            app.multicluster_view.cycle_view();
        }
        KeyCode::Char('h') if !app.show_help && app.selected_tab == 12 => {
            // Health check all clusters — handled in async handler
        }
        KeyCode::Esc
            if !app.show_help
                && app.selected_tab == 8
                && app.simulator_view.last_simulation.is_some() =>
        {
            // Exit simulation results view
            app.simulator_view.clear_simulation();
        }
        KeyCode::Char('s') if !app.show_help && app.selected_tab == 8 => {
            // Run simulation
            app.simulator_view.trigger_simulation();
        }
        KeyCode::Char('c') if !app.show_help && app.selected_tab == 8 => {
            // Clear simulation results
            app.simulator_view.clear_simulation();
        }
        KeyCode::Up
            if !app.show_help
                && app.selected_tab == 8
                && app.simulator_view.last_simulation.is_none() =>
        {
            // Navigate scenarios up
            app.simulator_view.move_selection_up();
        }
        KeyCode::Down
            if !app.show_help
                && app.selected_tab == 8
                && app.simulator_view.last_simulation.is_none() =>
        {
            // Navigate scenarios down (max 7 scenarios)
            app.simulator_view.move_selection_down(7);
        }
        KeyCode::Char('d') if !app.show_help && app.selected_tab == 5 => {
            // Detect problems manually (Healer tab) - handled in async handler
            // This arm is intentionally empty; the async version handles it.
        }
        // Flows tab (0) keyboard handlers
        KeyCode::Up if !app.show_help && app.selected_tab == 0 && !app.show_packet_explanation => {
            // Navigate flows up
            if app.selected_flow_index > 0 {
                app.selected_flow_index -= 1;
            }
        }
        KeyCode::Down
            if !app.show_help && app.selected_tab == 0 && !app.show_packet_explanation =>
        {
            // Navigate flows down
            let max_index = app.flows.len().saturating_sub(1);
            if app.selected_flow_index < max_index {
                app.selected_flow_index += 1;
            }
        }
        KeyCode::Char('e') if !app.show_help && app.selected_tab == 0 => {
            // Toggle packet explanation
            app.show_packet_explanation = !app.show_packet_explanation;
            if app.show_packet_explanation && !app.flows.is_empty() {
                app.set_status_message("Explaining packet...");
            }
        }
        KeyCode::Esc if !app.show_help && app.selected_tab == 0 && app.show_packet_explanation => {
            // Exit packet explanation
            app.show_packet_explanation = false;
        }
        _ => {
            // Non-async autopolicy/rootcause keys are handled here;
            // async ones (policy apply/rollback) are handled in handle_key_event_async.
        }
    }

    KeyAction::Continue
}

/// Handle async key events that require awaiting module operations.
/// This should be called after `handle_key_event` for keys that need async work.
pub(crate) async fn handle_key_event_async(app: &mut TuiApp, key_code: KeyCode) {
    match key_code {
        KeyCode::Char('d') if !app.show_help && app.selected_tab == 5 => {
            // Detect problems manually (Healer tab)
            match &mut app.modules {
                ModuleContainer::Enriched { healer, .. } => {
                    if let Err(e) = healer.run_enriched().await {
                        tracing::warn!("Healer enriched run failed: {}", e);
                    } else {
                        tracing::info!("Manual problem detection completed");
                    }
                }
                ModuleContainer::Mock { healer, .. } => {
                    if let Err(e) = healer.run().await {
                        tracing::warn!("Healer run failed: {}", e);
                    }
                }
            }
            app.last_healer_run = std::time::Instant::now();
        }
        KeyCode::Char('u') if !app.show_help && app.selected_tab == 6 => {
            // Update learning manually (AutoPolicy tab)
            match &mut app.modules {
                ModuleContainer::Enriched { autopolicy, .. } => {
                    if let Err(e) = autopolicy.update_enriched().await {
                        tracing::warn!("AutoPolicy enriched update failed: {}", e);
                    } else {
                        tracing::info!("Manual policy learning update completed");
                    }
                }
                ModuleContainer::Mock { autopolicy, .. } => {
                    if let Err(e) = autopolicy.update().await {
                        tracing::warn!("AutoPolicy update failed: {}", e);
                    }
                }
            }
            app.last_autopolicy_update = std::time::Instant::now();
        }
        // --- Chaos tab (10) async handlers ---
        KeyCode::Char('y')
            if !app.show_help
                && app.selected_tab == 10
                && app.chaos_view.confirmed
                && !app.chaos_view.circuit_breaker_confirm =>
        {
            // Start chaos experiment from selected preset
            let preset_idx = app.chaos_view.selected_preset_index;
            if let Some(experiment) = super::chaos_view::preset_to_experiment(preset_idx) {
                let name = super::chaos_view::CHAOS_PRESETS
                    .get(preset_idx)
                    .map(|(n, _, _, _)| n.to_string())
                    .unwrap_or_else(|| "Unknown".to_string());
                let target = crate::modules::chaos::ChaosTarget::default();
                let duration = Some(std::time::Duration::from_secs(300));

                match app
                    .chaos_engine
                    .start_experiment(name.clone(), experiment, target, duration)
                    .await
                {
                    Ok(id) => {
                        app.set_status_message(&format!(
                            "Chaos experiment '{}' started: {}",
                            name, id
                        ));
                    }
                    Err(e) => {
                        app.set_status_message(&format!("Failed to start chaos: {}", e));
                    }
                }
            } else {
                app.set_status_message("Invalid preset index");
            }
            app.chaos_view.confirmed = false;
            app.chaos_view.confirmation_mode = false;
        }
        KeyCode::Char('s')
            if !app.show_help && app.selected_tab == 10 && !app.chaos_view.show_presets =>
        {
            // Stop selected experiment
            let experiments = app.chaos_engine.active_experiments();
            if let Some(exp) = experiments.get(app.chaos_view.selected_experiment_index) {
                let id = exp.id.clone();
                match app.chaos_engine.stop_experiment(&id).await {
                    Ok(result) => {
                        app.set_status_message(&format!(
                            "Stopped '{}' ({}s, {} packets affected)",
                            result.experiment.name,
                            result.duration_secs,
                            result.experiment.metrics.packets_affected
                        ));
                    }
                    Err(e) => {
                        app.set_status_message(&format!("Failed to stop experiment: {}", e));
                    }
                }
            } else {
                app.set_status_message("No experiment selected");
            }
        }
        KeyCode::Char('S')
            if !app.show_help && app.selected_tab == 10 && !app.chaos_view.show_presets =>
        {
            // Stop all experiments
            match app.chaos_engine.stop_all().await {
                Ok(count) => {
                    app.set_status_message(&format!("Stopped {} chaos experiments", count));
                }
                Err(e) => {
                    app.set_status_message(&format!("Failed to stop all: {}", e));
                }
            }
        }
        // --- Canary tab (11) async handlers ---
        KeyCode::Char('+') if !app.show_help && app.selected_tab == 11 => {
            // Progress canary traffic
            let canaries = app.canary_engine.active_canaries();
            if let Some(c) = canaries.get(app.canary_view.selected_canary_index) {
                let id = c.id.clone();
                match app.canary_engine.progress_canary(&id).await {
                    Ok(()) => {
                        if let Some(updated) = app
                            .canary_engine
                            .active_canaries()
                            .iter()
                            .find(|c| c.id == id)
                        {
                            app.set_status_message(&format!(
                                "Canary traffic: {}% canary / {}% stable",
                                updated.current_split.canary_pct, updated.current_split.stable_pct
                            ));
                        }
                    }
                    Err(e) => {
                        app.set_status_message(&format!("Failed to progress canary: {}", e));
                    }
                }
            } else {
                app.set_status_message("No canary selected");
            }
        }
        // Handle confirmed canary promote/rollback
        KeyCode::Char('y') if !app.show_help && app.selected_tab == 11 => {
            if let Some(action) = app.canary_pending_action.take() {
                let canaries = app.canary_engine.active_canaries();
                if let Some(c) = canaries.get(app.canary_view.selected_canary_index) {
                    let id = c.id.clone();
                    let name = c.name.clone();
                    match action {
                        canary_view::ConfirmationType::Promote => {
                            match app.canary_engine.promote_canary(&id).await {
                                Ok(()) => {
                                    app.set_status_message(&format!(
                                        "Canary '{}' promoted to stable",
                                        name
                                    ));
                                }
                                Err(e) => {
                                    app.set_status_message(&format!("Failed to promote: {}", e));
                                }
                            }
                        }
                        canary_view::ConfirmationType::Rollback => {
                            match app.canary_engine.rollback_canary(&id).await {
                                Ok(()) => {
                                    app.set_status_message(&format!(
                                        "Canary '{}' rolled back",
                                        name
                                    ));
                                }
                                Err(e) => {
                                    app.set_status_message(&format!("Failed to rollback: {}", e));
                                }
                            }
                        }
                        _ => {}
                    }
                } else {
                    app.set_status_message("No canary selected");
                }
            }
        }
        // --- MultiCluster tab (12) async handlers ---
        KeyCode::Char('h') if !app.show_help && app.selected_tab == 12 => {
            // Health check all clusters
            match app.multicluster_engine.health_check_all().await {
                Ok(()) => {
                    let stats = app.multicluster_engine.stats();
                    app.set_status_message(&format!(
                        "Health check complete: {}/{} active, {} degraded",
                        stats.active_clusters, stats.total_clusters, stats.degraded_clusters
                    ));
                }
                Err(e) => {
                    app.set_status_message(&format!("Health check failed: {}", e));
                }
            }
        }
        _ => {
            // Delegate to autopolicy and rootcause async handlers
            if !handlers::handle_autopolicy_keys(app, key_code).await {
                handlers::handle_rootcause_keys(app, key_code).await;
            }
        }
    }
}
