// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Per-tab key event handlers extracted from events.rs

use super::app::{ModuleContainer, TuiApp};
use crossterm::event::KeyCode;

/// Root-cause fix names displayed in the UI and used for navigation bounds.
/// IMPORTANT: Keep in sync with TuiApp::get_fix_policy() in app.rs.
pub(crate) const FIX_NAMES: &[&str] = &[
    "allow-8080 policy",
    "DNS egress policy",
    "MTU adjustment",
    "DB access policy",
];

/// Number of available fixes (used for bounds checks).
pub(crate) const FIX_COUNT: usize = FIX_NAMES.len();

/// Handle keyboard events for the AutoPolicy tab (tab index 6).
/// Returns `true` if the key was handled, `false` otherwise.
pub(crate) async fn handle_autopolicy_keys(app: &mut TuiApp, key: KeyCode) -> bool {
    match key {
        KeyCode::Char('u') if !app.show_help && app.selected_tab == 6 => {
            // Update learning manually (AutoPolicy tab) - handled in async handler
            // This arm is intentionally empty; the async version handles it.
            true
        }
        KeyCode::Char('g') if !app.show_help && app.selected_tab == 6 => {
            // Generate policies (AutoPolicy tab)
            let result = match &mut app.modules {
                ModuleContainer::Enriched { autopolicy, .. } => autopolicy.generate_policies(),
                ModuleContainer::Mock { autopolicy, .. } => autopolicy.generate_policies(),
            };

            match result {
                Ok(policies) => {
                    if policies.is_empty() {
                        app.set_status_message(
                            "No policies generated. Need more observations (min 10).",
                        );
                        tracing::info!("No policies generated - insufficient observations");
                    } else {
                        // Save policies to files
                        match app.save_policies(&policies) {
                            Ok(count) => {
                                app.set_status_message(&format!(
                                    "Generated {} policies -> saved to ./policies/",
                                    count
                                ));
                                tracing::info!("Generated and saved {} policies", count);
                            }
                            Err(e) => {
                                app.set_status_message(&format!("Error saving policies: {}", e));
                                tracing::warn!("Failed to save policies: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    app.set_status_message(&format!("Policy generation failed: {}", e));
                    tracing::warn!("Policy generation failed: {}", e);
                }
            }
            true
        }
        KeyCode::Char('A')
            if !app.show_help && app.selected_tab == 6 && !app.policy_detail_mode =>
        {
            // Batch apply all unapplied policies
            let policies = match &app.modules {
                ModuleContainer::Enriched { autopolicy, .. } => autopolicy.policies(),
                ModuleContainer::Mock { autopolicy, .. } => autopolicy.policies(),
            };

            let unapplied_count = policies
                .iter()
                .filter(|p| !app.applied_policies.contains(&p.name))
                .count();

            if unapplied_count == 0 {
                app.set_status_message("All policies already applied");
            } else {
                app.policy_batch_apply_confirmation = true;
                app.set_status_message(&format!(
                    "Apply {} policies? Press 'y' to confirm, 'n' to cancel",
                    unapplied_count
                ));
            }
            true
        }
        KeyCode::Char('R')
            if !app.show_help && app.selected_tab == 6 && !app.policy_detail_mode =>
        {
            // Batch rollback all applied policies
            let applied_count = app.applied_policies.len();

            if applied_count == 0 {
                app.set_status_message("No policies applied to rollback");
            } else {
                app.policy_batch_rollback_confirmation = true;
                app.set_status_message(&format!(
                    "Rollback {} policies? Press 'y' to confirm, 'n' to cancel",
                    applied_count
                ));
            }
            true
        }
        KeyCode::Char('y')
            if !app.show_help && app.selected_tab == 6 && app.policy_batch_apply_confirmation =>
        {
            // Confirm batch apply
            app.policy_batch_apply_confirmation = false;

            let policies = match &app.modules {
                ModuleContainer::Enriched { autopolicy, .. } => autopolicy.policies(),
                ModuleContainer::Mock { autopolicy, .. } => autopolicy.policies(),
            };

            let mut applied = 0;
            let mut failed = 0;

            for policy in policies.iter() {
                if !app.applied_policies.contains(&policy.name) {
                    let policy_name = policy.name.clone();
                    let policy_yaml = policy.yaml.clone();

                    match app.apply_policy_kubectl(&policy_name, &policy_yaml).await {
                        Ok(_) => {
                            app.applied_policies.insert(policy_name.clone());
                            applied += 1;
                            tracing::info!("Applied policy: {}", policy_name);
                        }
                        Err(e) => {
                            failed += 1;
                            tracing::error!("Failed to apply policy '{}': {}", policy_name, e);
                        }
                    }
                }
            }

            if failed == 0 {
                app.set_status_message(&format!("Applied {} policies successfully", applied));
            } else {
                app.set_status_message(&format!("Applied: {}, Failed: {}", applied, failed));
            }
            true
        }
        KeyCode::Char('y')
            if !app.show_help
                && app.selected_tab == 6
                && app.policy_batch_rollback_confirmation =>
        {
            // Confirm batch rollback
            app.policy_batch_rollback_confirmation = false;

            let policies = match &app.modules {
                ModuleContainer::Enriched { autopolicy, .. } => autopolicy.policies(),
                ModuleContainer::Mock { autopolicy, .. } => autopolicy.policies(),
            };

            let mut rolled_back = 0;
            let mut failed = 0;
            let applied_names: Vec<String> = app.applied_policies.iter().cloned().collect();

            for policy_name in applied_names {
                // Find the policy to get its namespace
                if let Some(policy) = policies.iter().find(|p| p.name == policy_name) {
                    match app
                        .rollback_policy_kubectl(&policy.name, &policy.namespace)
                        .await
                    {
                        Ok(_) => {
                            app.applied_policies.remove(&policy.name);
                            rolled_back += 1;
                            tracing::info!("Rolled back policy: {}", policy.name);
                        }
                        Err(e) => {
                            failed += 1;
                            tracing::error!("Failed to rollback policy '{}': {}", policy.name, e);
                        }
                    }
                }
            }

            if failed == 0 {
                app.set_status_message(&format!(
                    "Rolled back {} policies successfully",
                    rolled_back
                ));
            } else {
                app.set_status_message(&format!(
                    "Rolled back: {}, Failed: {}",
                    rolled_back, failed
                ));
            }
            true
        }
        KeyCode::Char('n')
            if !app.show_help
                && app.selected_tab == 6
                && (app.policy_batch_apply_confirmation
                    || app.policy_batch_rollback_confirmation) =>
        {
            // Cancel batch operation
            if app.policy_batch_apply_confirmation {
                app.policy_batch_apply_confirmation = false;
                app.set_status_message("Batch apply cancelled");
            } else if app.policy_batch_rollback_confirmation {
                app.policy_batch_rollback_confirmation = false;
                app.set_status_message("Batch rollback cancelled");
            }
            true
        }
        KeyCode::Char('v') if !app.show_help && app.selected_tab == 6 => {
            // Toggle policy detail view (AutoPolicy tab)
            app.policy_detail_mode = !app.policy_detail_mode;
            if app.policy_detail_mode {
                app.set_status_message("Policy detail view (Up/Down: navigate, Esc: exit)");
            } else {
                app.selected_policy_index = 0;
            }
            true
        }
        KeyCode::Up if !app.show_help && app.selected_tab == 6 && app.policy_detail_mode => {
            // Navigate policies up
            if app.selected_policy_index > 0 {
                app.selected_policy_index -= 1;
            }
            true
        }
        KeyCode::Down if !app.show_help && app.selected_tab == 6 && app.policy_detail_mode => {
            // Navigate policies down
            let policy_count = match &app.modules {
                ModuleContainer::Enriched { autopolicy, .. } => autopolicy.policies().len(),
                ModuleContainer::Mock { autopolicy, .. } => autopolicy.policies().len(),
            };
            if app.selected_policy_index + 1 < policy_count {
                app.selected_policy_index += 1;
            }
            true
        }
        KeyCode::Esc if !app.show_help && app.selected_tab == 6 => {
            // Exit or cancel confirmations
            if app.policy_apply_confirmation {
                app.policy_apply_confirmation = false;
                app.set_status_message("Policy application cancelled");
            } else if app.policy_rollback_confirmation {
                app.policy_rollback_confirmation = false;
                app.set_status_message("Policy rollback cancelled");
            } else if app.policy_batch_apply_confirmation {
                app.policy_batch_apply_confirmation = false;
                app.set_status_message("Batch apply cancelled");
            } else if app.policy_batch_rollback_confirmation {
                app.policy_batch_rollback_confirmation = false;
                app.set_status_message("Batch rollback cancelled");
            } else if app.policy_detail_mode {
                app.policy_detail_mode = false;
                app.selected_policy_index = 0;
            }
            true
        }
        KeyCode::Char('a')
            if !app.show_help
                && app.selected_tab == 6
                && app.policy_detail_mode
                && !app.policy_apply_confirmation =>
        {
            // Trigger policy application confirmation
            let policies = match &app.modules {
                ModuleContainer::Enriched { autopolicy, .. } => autopolicy.policies(),
                ModuleContainer::Mock { autopolicy, .. } => autopolicy.policies(),
            };

            if !policies.is_empty() {
                let idx = app.selected_policy_index.min(policies.len() - 1);
                let policy_name = &policies[idx].name;
                if app.applied_policies.contains(policy_name) {
                    app.set_status_message(&format!("Policy '{}' already applied", policy_name));
                } else {
                    app.policy_apply_confirmation = true;
                    app.set_status_message(&format!(
                        "Apply policy '{}'? Press 'y' to confirm, 'n' to cancel",
                        policy_name
                    ));
                }
            }
            true
        }
        KeyCode::Char('y')
            if !app.show_help && app.selected_tab == 6 && app.policy_apply_confirmation =>
        {
            // Confirm and apply policy
            app.policy_apply_confirmation = false;

            let policies = match &app.modules {
                ModuleContainer::Enriched { autopolicy, .. } => autopolicy.policies(),
                ModuleContainer::Mock { autopolicy, .. } => autopolicy.policies(),
            };

            if !policies.is_empty() {
                let idx = app.selected_policy_index.min(policies.len() - 1);
                let policy = &policies[idx];
                // Clone policy data to avoid borrow checker issues
                let policy_name = policy.name.clone();
                let policy_yaml = policy.yaml.clone();

                match app.apply_policy_kubectl(&policy_name, &policy_yaml).await {
                    Ok(_) => {
                        app.applied_policies.insert(policy_name.clone());
                        app.set_status_message(&format!(
                            "Policy '{}' applied successfully",
                            policy_name
                        ));
                        tracing::info!("Applied policy: {}", policy_name);
                    }
                    Err(e) => {
                        app.set_status_message(&format!("Failed to apply policy: {}", e));
                        tracing::error!("Failed to apply policy '{}': {}", policy_name, e);
                    }
                }
            }
            true
        }
        KeyCode::Char('n')
            if !app.show_help && app.selected_tab == 6 && app.policy_apply_confirmation =>
        {
            // Cancel policy application
            app.policy_apply_confirmation = false;
            app.set_status_message("Policy application cancelled");
            true
        }
        KeyCode::Char('r')
            if !app.show_help
                && app.selected_tab == 6
                && app.policy_detail_mode
                && !app.policy_apply_confirmation
                && !app.policy_rollback_confirmation =>
        {
            // Trigger policy rollback confirmation
            let policies = match &app.modules {
                ModuleContainer::Enriched { autopolicy, .. } => autopolicy.policies(),
                ModuleContainer::Mock { autopolicy, .. } => autopolicy.policies(),
            };

            if !policies.is_empty() {
                let idx = app.selected_policy_index.min(policies.len() - 1);
                let policy_name = &policies[idx].name;
                if !app.applied_policies.contains(policy_name) {
                    app.set_status_message(&format!(
                        "Policy '{}' not applied, cannot rollback",
                        policy_name
                    ));
                } else {
                    app.policy_rollback_confirmation = true;
                    app.set_status_message(&format!(
                        "Rollback policy '{}'? Press 'y' to confirm, 'n' to cancel",
                        policy_name
                    ));
                }
            }
            true
        }
        KeyCode::Char('y')
            if !app.show_help && app.selected_tab == 6 && app.policy_rollback_confirmation =>
        {
            // Confirm and rollback policy
            app.policy_rollback_confirmation = false;

            let policies = match &app.modules {
                ModuleContainer::Enriched { autopolicy, .. } => autopolicy.policies(),
                ModuleContainer::Mock { autopolicy, .. } => autopolicy.policies(),
            };

            if !policies.is_empty() {
                let idx = app.selected_policy_index.min(policies.len() - 1);
                let policy = &policies[idx];
                let policy_name = policy.name.clone();
                let policy_namespace = policy.namespace.clone();

                match app
                    .rollback_policy_kubectl(&policy_name, &policy_namespace)
                    .await
                {
                    Ok(_) => {
                        app.applied_policies.remove(&policy_name);
                        app.set_status_message(&format!(
                            "Policy '{}' rolled back successfully",
                            policy_name
                        ));
                        tracing::info!("Rolled back policy: {}", policy_name);
                    }
                    Err(e) => {
                        app.set_status_message(&format!("Failed to rollback policy: {}", e));
                        tracing::error!("Failed to rollback policy '{}': {}", policy_name, e);
                    }
                }
            }
            true
        }
        KeyCode::Char('n')
            if !app.show_help && app.selected_tab == 6 && app.policy_rollback_confirmation =>
        {
            // Cancel policy rollback
            app.policy_rollback_confirmation = false;
            app.set_status_message("Policy rollback cancelled");
            true
        }
        _ => false,
    }
}

/// Handle keyboard events for the RootCause tab (tab index 7).
/// Returns `true` if the key was handled, `false` otherwise.
pub(crate) async fn handle_rootcause_keys(app: &mut TuiApp, key: KeyCode) -> bool {
    match key {
        KeyCode::Up if !app.show_help && app.selected_tab == 7 => {
            // Navigate fixes up
            if app.selected_fix_index > 0 {
                app.selected_fix_index -= 1;
            }
            true
        }
        KeyCode::Down if !app.show_help && app.selected_tab == 7 => {
            // Navigate fixes down (matches fix_names array in 'a' handler)
            if app.selected_fix_index < FIX_NAMES.len() - 1 {
                app.selected_fix_index += 1;
            }
            true
        }
        KeyCode::Char('a')
            if !app.show_help && app.selected_tab == 7 && !app.fix_apply_confirmation =>
        {
            // Trigger fix application confirmation
            app.fix_apply_confirmation = true;
            let name = FIX_NAMES.get(app.selected_fix_index).unwrap_or(&"unknown");
            app.set_status_message(&format!(
                "Apply fix '{}'? Press 'y' to confirm, 'n' to cancel",
                name
            ));
            true
        }
        KeyCode::Char('y')
            if !app.show_help && app.selected_tab == 7 && app.fix_apply_confirmation =>
        {
            // Confirm and apply fix
            app.fix_apply_confirmation = false;

            let (fix_name, policy_yaml) = app.get_fix_policy(app.selected_fix_index);

            match app.apply_policy_kubectl(&fix_name, &policy_yaml).await {
                Ok(_) => {
                    app.set_status_message(&format!("Applied fix: {}", fix_name));
                    tracing::info!("Applied RootCause fix policy: {}", fix_name);
                }
                Err(e) => {
                    app.set_status_message(&format!("Failed to apply fix: {}", e));
                    tracing::error!("Failed to apply RootCause fix '{}': {}", fix_name, e);
                }
            }
            true
        }
        KeyCode::Char('n')
            if !app.show_help && app.selected_tab == 7 && app.fix_apply_confirmation =>
        {
            // Cancel fix application
            app.fix_apply_confirmation = false;
            app.set_status_message("Fix application cancelled");
            true
        }
        KeyCode::Esc if !app.show_help && app.selected_tab == 7 && app.fix_apply_confirmation => {
            // Cancel fix application with Esc
            app.fix_apply_confirmation = false;
            app.set_status_message("Fix application cancelled");
            true
        }
        _ => false,
    }
}
