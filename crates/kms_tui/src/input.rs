mod agent;
mod events;
mod keys;
mod paste;
#[cfg(test)]
mod tests;

pub use agent::spawn_agent_task;
pub use keys::handle_key_event;
pub use paste::summarize_paste;

use std::time::Duration;

use agentik_types::AgentEvent;
use crossterm::event::Event;
use ratatui::Terminal;

use crate::state::{Action, App, Panel, SettingsPane};
use crate::widgets::ui;

use crate::CrosstermBackend;
use crate::settings::save_settings;

pub async fn run_app(
    terminal: &mut Terminal<CrosstermBackend>,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let mut pending_refresh = false;
        let old_version = app.message_version;

        // Collect all pending events from the agent channel.
        let agent_events: Vec<AgentEvent> = if let Some(rx) = &mut app.agent_event_rx {
            let mut events = Vec::new();
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
            events
        } else {
            Vec::new()
        };

        let mut had_events = false;

        for event in agent_events {
            tracing::debug!("{:?}", &event);
            had_events = true;
            match event {
                AgentEvent::Done => {
                    let kind = app.agent_kind;
                    let history = app.agent_messages_map.get_mut(&kind).unwrap();
                    events::finalize_streaming_history(history);
                    history.push(crate::chat::ChatMessage::Done);
                    app.agent_running = false;
                    app.agent_requesting = false;
                    app.agent_usage_tokens = None;
                    app.bump_message_version();
                    pending_refresh = true;
                }
                AgentEvent::Requesting => {
                    app.agent_requesting = true;
                }
                AgentEvent::TextDelta(token) => {
                    app.agent_requesting = false;
                    events::append_to_streaming_assistant(app, &token);
                    app.bump_message_version();
                }
                AgentEvent::ThinkingDelta(token) => {
                    app.agent_requesting = false;
                    events::append_to_streaming_thinking(app, &token);
                    app.bump_message_version();
                }
                AgentEvent::UsageUpdate { output_tokens, .. } => {
                    app.agent_usage_tokens = Some(output_tokens);
                }
                AgentEvent::StreamStart { .. }
                | AgentEvent::ContentBlockStart { .. }
                | AgentEvent::ContentBlockStop { .. }
                | AgentEvent::StreamDelta { .. } => {}
                event @ AgentEvent::ToolResult { .. } => {
                    app.agent_requesting = false;
                    events::handle_final_event(app, event);
                    app.bump_message_version();
                    pending_refresh = true;
                }
                AgentEvent::ToolCall { .. } => {
                    // Tool is about to execute — keep the spinner spinning
                    // so the user knows the agent is still working.
                    app.agent_requesting = true;
                    events::handle_final_event(app, event);
                    app.bump_message_version();
                }
                event => {
                    app.agent_requesting = false;
                    events::handle_final_event(app, event);
                    app.bump_message_version();
                }
            }
        }

        // Drain ProcessManager events into AgentPanelState.
        let mut had_process_events = false;
        if let Some(rx) = &mut app.process_event_rx {
            while let Ok(event) = rx.try_recv() {
                had_process_events = true;
                // Register new agents on first sight.
                match &event {
                    agentik_core::process::ProcessEvent::StateChanged { agent_id, .. }
                    | agentik_core::process::ProcessEvent::Agent { agent_id, .. } => {
                        if !app.agent_panel.agents.iter().any(|e| e.agent_id == *agent_id) {
                            // Look up the title from the shared map.
                            let title = app
                                .agent_titles
                                .read()
                                .ok()
                                .and_then(|map| map.get(agent_id).cloned())
                                .unwrap_or_else(|| format!("Agent {}", &agent_id.to_string()[..8]));
                            app.agent_panel.add_agent(*agent_id, title);
                        }
                    }
                    _ => {}
                }
                app.agent_panel.apply_process_event(&event);
                // Trigger tree refresh when a sub-agent exits.
                if let agentik_core::process::ProcessEvent::ProcessExited { .. } = &event {
                    pending_refresh = true;
                }
            }
        }

        // Advance the spinner before rendering.
        if app.agent_requesting {
            app.spinner_tick = (app.spinner_tick + 1) % 8;
            app.needs_render = true;
        }

        // Mark dirty if any events arrived or messages changed.
        if had_events || had_process_events || app.message_version != old_version {
            app.needs_render = true;
        }

        // Always tick the toast (advances auto-expire timer) even when
        // not rendering, so toasts don't get stuck on screen.
        app.toast.tick();

        // Render ONLY when state has changed — avoids the expensive
        // to_lines() + wrapped_line_count() pipeline on idle frames.
        if app.needs_render {
            // Render FIRST so every panel reflects the latest state.
            terminal.draw(|f| ui(f, app))?;
            app.needs_render = false;
        }

        if pending_refresh {
            const REFRESH_DEBOUNCE: Duration = Duration::from_millis(200);
            let should_refresh = match app.last_tree_refresh_at {
                Some(t) => t.elapsed() >= REFRESH_DEBOUNCE,
                None => true,
            };
            if should_refresh {
                app.refresh_tree().await;
                app.last_tree_refresh_at = Some(std::time::Instant::now());
            }
        }

        if crossterm::event::poll(Duration::from_millis(100))? {
            let mut had_input = false;
            loop {
                let event = crossterm::event::read()?;
                match event {
                    Event::Key(key) => {
                        had_input = true;
                        match handle_key_event(key, app) {
                        Action::Quit => app.should_quit = true,
                        Action::TreeChanged => app.on_tree_select().await,
                        Action::SubmitAgent(input) => spawn_agent_task(app, input),
                        Action::OpenSettings => {
                            app.settings_modal_open = true;
                        }
                        Action::SwitchAgent => {
                            let next = app.agent_kind.toggle();
                            app.agent_kind = next;
                            app.bump_message_version();
                            app.toast
                                .info(format!("Switched to {} agent", app.agent_kind.label()));
                        }
                        Action::SettingsNav(pane, delta) => match pane {
                            SettingsPane::Provider => {
                                let max = app.providers.len().saturating_sub(1);
                                app.settings_selected_provider =
                                    (app.settings_selected_provider as isize + delta)
                                        .clamp(0, max as isize)
                                    as usize;
                                app.settings_selected_model = 0;
                            }
                            SettingsPane::Model => {
                                if let Some(provider) =
                                    app.providers.get(app.settings_selected_provider)
                                {
                                    let max = provider.models.len().saturating_sub(1);
                                    app.settings_selected_model =
                                        (app.settings_selected_model as isize + delta)
                                            .clamp(0, max as isize)
                                    as usize;
                                }
                            }
                            SettingsPane::Pool => {
                                let max = app.pool_entries.len().saturating_sub(1);
                                app.settings_selected_pool = (app.settings_selected_pool as isize
                                    + delta)
                                    .clamp(0, max as isize)
                                    as usize;
                            }
                        },
                        Action::SettingsSwitchPane(pane) => {
                            app.settings_pane = pane;
                        }
                        Action::SettingsTogglePool => {
                            let pair = app.providers.get(app.settings_selected_provider).map(|p| {
                                let pid = p.id.clone();
                                let mname = p
                                    .models
                                    .get(app.settings_selected_model)
                                    .cloned()
                                    .unwrap_or_default();
                                (pid, mname)
                            });
                            if let Some((provider_id, model_name)) = pair {
                                let was_in = app.is_in_pool(&provider_id, &model_name);
                                app.toggle_pool_entry(&provider_id, &model_name);
                                save_settings(&app.provider_configs, &app.pool_entries);
                                let label = app
                                    .providers
                                    .iter()
                                    .find(|p| p.id == provider_id)
                                    .map(|p| p.display_name.clone())
                                    .unwrap_or_else(|| provider_id.clone());
                                if was_in {
                                    app.toast.info(format!("Removed {} / {}", label, model_name));
                                } else {
                                    app.toast.info(format!("Added {} / {}", label, model_name));
                                }
                            }
                        }
                        Action::SettingsRemovePool => {
                            let idx = app.settings_selected_pool;
                            if idx < app.pool_entries.len() {
                                let removed = app.pool_entries[idx].clone();
                                app.remove_pool_entry(idx);
                                save_settings(&app.provider_configs, &app.pool_entries);
                                let max = app.pool_entries.len().saturating_sub(1);
                                if app.settings_selected_pool > max {
                                    app.settings_selected_pool = max;
                                }
                                let label = app
                                    .providers
                                    .iter()
                                    .find(|p| p.id == removed.provider_id)
                                    .map(|p| p.display_name.clone())
                                    .unwrap_or_else(|| removed.provider_id.clone());
                                app.toast.info(format!("Removed {} / {}", label, removed.model));
                            }
                        }
                        Action::SettingsConfirm => {
                            if app.pool_entries.is_empty() {
                                app.toast.warning("Pool is empty — no changes applied");
                            } else {
                                let new_entries = app.pool_entries.clone();
                                if let Some(pool) = crate::settings::build_pool_from_entries(
                                    &new_entries,
                                    &app.providers,
                                ) {
                                    let pool_arc = std::sync::Arc::new(pool);
                                    agent::rebuild_all_agents(app, &pool_arc).await;
                                    app.toast.success(format!(
                                        "Pool updated: {} model(s)",
                                        new_entries.len()
                                    ));
                                } else {
                                    app.toast.error("Failed to build model pool from selections");
                                }
                            }
                            app.settings_modal_open = false;
                        }
                        Action::SettingsNewProvider => {
                            if app.settings_pane == SettingsPane::Provider {
                                app.new_provider_form = Some(crate::state::NewProviderForm::new());
                            } else {
                                app.toast.info("Switch to the Providers pane to add a new one");
                            }
                        }
                        Action::SettingsDeleteProvider => {
                            if app.settings_pane == SettingsPane::Provider {
                                let id = app
                                    .providers
                                    .get(app.settings_selected_provider)
                                    .map(|p| p.id.clone());
                                if let Some(id) = id {
                                    if app.remove_custom_provider(&id) {
                                        save_settings(&app.provider_configs, &app.pool_entries);
                                        let max = app.providers.len().saturating_sub(1);
                                        if app.settings_selected_provider > max {
                                            app.settings_selected_provider = max;
                                        }
                                        app.toast.info("Custom provider removed");
                                    } else {
                                        app.toast.warning("Built-in providers cannot be removed");
                                    }
                                }
                            }
                        }
                        Action::SettingsFormCycleField(delta) => {
                            if let Some(form) = app.new_provider_form.as_mut() {
                                form.active_field = if delta > 0 {
                                    (form.active_field + 1) % 4
                                } else {
                                    (form.active_field + 3) % 4
                                };
                            }
                        }
                        Action::SettingsFormCycleType(delta) => {
                            if let Some(form) = app.new_provider_form.as_mut() {
                                if form.active_field == 0 {
                                    let n = crate::settings::BUILTIN_PROVIDER_TYPES.len();
                                    if delta > 0 {
                                        form.type_idx = (form.type_idx + 1) % n;
                                    } else {
                                        form.type_idx = (form.type_idx + n - 1) % n;
                                    }
                                    form.url_preset_idx = form
                                        .presets()
                                        .iter()
                                        .position(|p| !p.is_custom)
                                        .unwrap_or(0);
                                    form.url_custom.clear();
                                    form.error = None;
                                } else if form.active_field == 3 {
                                    let n = form.presets().len().max(1);
                                    if delta > 0 {
                                        form.url_preset_idx = (form.url_preset_idx + 1) % n;
                                    } else {
                                        form.url_preset_idx = (form.url_preset_idx + n - 1) % n;
                                    }
                                    form.error = None;
                                }
                            }
                        }
                        Action::SettingsFormType(c) => {
                            if let Some(form) = app.new_provider_form.as_mut() {
                                form.error = None;
                                match form.active_field {
                                    1 => form.display_name.push(c),
                                    2 => form.api_key.push(c),
                                    3 if form.url_is_custom() => form.url_custom.push(c),
                                    _ => {}
                                }
                            }
                        }
                        Action::SettingsFormBackspace => {
                            if let Some(form) = app.new_provider_form.as_mut() {
                                match form.active_field {
                                    1 => { let _ = form.display_name.pop(); }
                                    2 => { let _ = form.api_key.pop(); }
                                    3 if form.url_is_custom() => { let _ = form.url_custom.pop(); }
                                    _ => {}
                                }
                            }
                        }
                        Action::SettingsFormSubmit => {
                            if let Some(form) = app.new_provider_form.take() {
                                let ptype = crate::settings::BUILTIN_PROVIDER_TYPES[form.type_idx];
                                let display_name = form.display_name.trim().to_string();
                                let api_key = form.api_key.trim().to_string();
                                let base_url = form.resolved_url();
                                if display_name.is_empty() {
                                    app.toast.warning("Display name cannot be empty");
                                    app.new_provider_form = Some(form);
                                } else if api_key.is_empty() {
                                    app.toast.warning("API key cannot be empty");
                                    app.new_provider_form = Some(form);
                                } else if base_url.is_empty() && form.url_is_custom() {
                                    app.toast.warning("Custom URL cannot be empty");
                                    app.new_provider_form = Some(form);
                                } else {
                                    let new_id = app.add_custom_provider(
                                        display_name.clone(),
                                        ptype.to_string(),
                                        api_key,
                                        base_url,
                                    );
                                    save_settings(&app.provider_configs, &app.pool_entries);
                                    app.toast.success(format!(
                                        "Added custom provider: {} ({})",
                                        display_name, ptype
                                    ));
                                    if let Some(pos) =
                                        app.providers.iter().position(|p| p.id == new_id)
                                    {
                                        app.settings_selected_provider = pos;
                                    }
                                }
                            }
                        }
                        Action::SettingsFormCancel => {
                            app.new_provider_form = None;
                        }
                        Action::None => {}
                        }
                    }
                    Event::Paste(s) if app.new_provider_form.is_some() => {
                        had_input = true;
                        if let Some(form) = app.new_provider_form.as_mut() {
                            form.error = None;
                            match form.active_field {
                                1 => form.display_name.push_str(&s),
                                2 => form.api_key.push_str(&s),
                                3 if form.url_is_custom() => form.url_custom.push_str(&s),
                                _ => {}
                            }
                        }
                    }
                    Event::Paste(s)
                        if app.focused == Panel::Agent
                            && app.agent_input_active
                            && !app.agent_running =>
                    {
                        had_input = true;
                        if let Some(placeholder) = summarize_paste(&s) {
                            app.agent_pastes.push((placeholder.clone(), s));
                            app.agent_input.push_str(&placeholder);
                        } else {
                            app.agent_input.push_str(&s);
                        }
                    }
                    _ => {}
                }
                if !crossterm::event::poll(Duration::from_secs(0))? {
                    break;
                }
            }
            if had_input {
                app.needs_render = true;
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
