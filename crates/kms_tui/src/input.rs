mod agent;
pub mod agent_task {
    pub use super::agent::*;
}
mod events;
mod keys;
mod paste;
#[cfg(test)]
mod tests;

pub use agent::spawn_agent_task;
pub use events::{
    agent_event_to_message, append_to_streaming_assistant, append_to_streaming_thinking,
    finalize_streaming_history, handle_final_event,
};
pub use keys::handle_key_event;
pub use paste::{PASTE_SUMMARY_LEN_THRESHOLD, PASTE_SUMMARY_LINE_THRESHOLD, summarize_paste};

use std::time::Duration;

use agentik_types::AgentUiEvent;
use crossterm::event::Event;
use ratatui::Terminal;

use crate::chat::ChatMessage;
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

        // Collect all pending events from the agent channel, then
        // process them. Draining into a Vec first avoids a mutable
        // borrow conflict: `rx` borrows `app.event_rx` while the
        // helper functions need `&mut app` for the messages map.
        let agent_events: Vec<AgentUiEvent> = if let Some(rx) = &mut app.agent_event_rx {
            let mut events = Vec::new();
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
            events
        } else {
            Vec::new()
        };

        for event in agent_events {
            tracing::debug!("{:?}", &event);
            match event {
                AgentUiEvent::Done => {
                    // Finalize any still-streaming messages
                    let kind = app.agent_kind;
                    let history = app.agent_messages_map.get_mut(&kind).unwrap();
                    finalize_streaming_history(history);
                    history.push(ChatMessage::Done);
                    app.agent_running = false;
                    app.agent_requesting = false;
                    app.agent_usage_tokens = None;
                    // Refresh the tree one final time so the user sees
                    // any knowledge entries created during this run.
                    pending_refresh = true;
                }
                AgentUiEvent::Requesting => {
                    app.agent_requesting = true;
                }
                AgentUiEvent::TextDelta(token) => {
                    app.agent_requesting = false;
                    append_to_streaming_assistant(app, &token);
                }
                AgentUiEvent::ThinkingDelta(token) => {
                    app.agent_requesting = false;
                    append_to_streaming_thinking(app, &token);
                }
                AgentUiEvent::UsageUpdate { output_tokens, .. } => {
                    app.agent_usage_tokens = Some(output_tokens);
                }
                // Stream lifecycle events — absorbed silently.
                AgentUiEvent::StreamStart { .. }
                | AgentUiEvent::ContentBlockStart { .. }
                | AgentUiEvent::ContentBlockStop { .. }
                | AgentUiEvent::StreamDelta { .. } => {}
                // Aggregated responses and tool events.
                // ToolResult may have modified the knowledge tree
                // (e.g. direct kms_create_knowledge), so refresh.
                event @ AgentUiEvent::ToolResult { .. } => {
                    app.agent_requesting = false;
                    handle_final_event(app, event);
                    pending_refresh = true;
                }
                // LlmResponse, Thinking, ToolCall, Error — no tree change.
                event => {
                    app.agent_requesting = false;
                    handle_final_event(app, event);
                }
            }
        }

        // TODO: 改进为后端维护多Agent进程池，以更好的支持多Agent
        //
        // Drain the parallel-dispatch side-channel. Each event
        // updates the panel state machine; we mark `pending_refresh`
        // whenever a sub-agent finishes or fails so the tree view
        // grows incrementally as staging areas get populated.
        //
        // In addition to driving the panel, sub-agent LLM responses
        // are also pushed into the **main chat history** so the user
        // sees the actual work the sub-agents produced, the same way
        // they see the orchestrator's `Assistant` text. Without this,
        // the parallel panel's collapsed-by-default rows hide every
        // sub-agent response and the user is left watching the
        // orchestrator's tool-call marker for minutes with no
        // feedback ("看不到Agent响应的任何内容"). The panel and the
        // chat history are complementary, not exclusive: the panel
        // shows the compact per-sub-agent tree, the chat history
        // shows the full text inline.
        if let Some(rx) = &mut app.parallel_progress_rx {
            while let Ok(event) = rx.try_recv() {
                use dendrite_tools::parallel_progress::ParallelProgress as P;
                if app.parallel_panel.is_none() {
                    // Lazy-init the panel on the first event. We use
                    // `DispatchStarted` to learn the total; if we
                    // somehow receive an event before DispatchStarted
                    // (e.g. a StagingCreated with no preceding start)
                    // we still init the panel with `total = 0` and let
                    // the subsequent DispatchStarted correct it.
                    let total = if let P::DispatchStarted { total } = &event {
                        *total
                    } else {
                        0
                    };
                    app.parallel_panel =
                        Some(crate::parallel_panel::ParallelPanelState::new(total));
                }
                if let Some(panel) = app.parallel_panel.as_mut() {
                    panel.apply(&event);
                }
                // Sub-agent LLM text: surface in the main chat
                // history. The agent is identified by `app.agent_kind`
                // (Parallel at the time the user dispatched), which
                // is the same bucket the orchestrator's events are
                // going into, so the two streams interleave naturally.
                //
                // During streaming, `TextDelta` tokens are appended to
                // the last `SubAgentResponse` with a matching title.
                // When the aggregated `LlmResponse` arrives, it
                // replaces the streaming text with the authoritative
                // full response.
                if let P::SubAgentEvent { title, event } = &event {
                    match event {
                        AgentUiEvent::TextDelta(token) => {
                            let kind = app.agent_kind;
                            if let Some(history) = app.agent_messages_map.get_mut(&kind) {
                                let found = history.iter_mut().rev().find(|m| {
                                    matches!(
                                        m,
                                        ChatMessage::SubAgentResponse {
                                            title: t, ..
                                        } if t == title
                                    )
                                });
                                if let Some(ChatMessage::SubAgentResponse { text, .. }) = found {
                                    text.push_str(token);
                                } else {
                                    history.push(ChatMessage::SubAgentResponse {
                                        title: title.clone(),
                                        text: token.clone(),
                                    });
                                }
                            }
                        }
                        AgentUiEvent::LlmResponse(text) => {
                            let kind = app.agent_kind;
                            if let Some(history) = app.agent_messages_map.get_mut(&kind) {
                                let found = history.iter_mut().rev().find(|m| {
                                    matches!(
                                        m,
                                        ChatMessage::SubAgentResponse {
                                            title: t, ..
                                        } if t == title
                                    )
                                });
                                if let Some(ChatMessage::SubAgentResponse { text, .. }) = found {
                                    // Replace the streaming text with
                                    // the authoritative full response.
                                    *text = text.clone();
                                } else if !text.is_empty() {
                                    history.push(ChatMessage::SubAgentResponse {
                                        title: title.clone(),
                                        text: text.clone(),
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
                match &event {
                    P::SubAgentCompleted { .. } | P::SubAgentFailed { .. } => {
                        pending_refresh = true;
                    }
                    _ => {}
                }
            }
        }

        // Advance the spinner *before* rendering so the latest frame
        // always carries the updated glyph.
        if app.agent_requesting {
            app.spinner_tick = (app.spinner_tick + 1) % 8;
        }

        // Render FIRST so the Agent panel (and every other panel)
        // always reflects the latest event state on every frame.
        // Any pending tree refresh happens *after* the render, so a
        // slow refresh_tree() never delays UI updates.
        terminal.draw(|f| ui(f, app))?;

        if pending_refresh {
            // Debounce: if we refreshed within the last 200ms, defer
            // to the next loop iteration. The next event will re-set
            // `pending_refresh` and we'll check again.
            const REFRESH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(200);
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
            loop {
                let event = crossterm::event::read()?;
                match event {
                    Event::Key(key) => match handle_key_event(key, app) {
                        Action::Quit => app.should_quit = true,
                        Action::TreeChanged => app.on_tree_select().await,
                        Action::SubmitAgent(input) => spawn_agent_task(app, input),
                        Action::OpenSettings => {
                            app.settings_modal_open = true;
                        }
                        Action::SwitchAgent => {
                            let next = app.agent_kind.toggle();
                            app.agent_kind = next;
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
                                // Reset model selection when switching providers.
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
                                // Persist eagerly — the user can quit the
                                // TUI right after this toggle (without
                                // pressing Enter on the modal) and the
                                // change must still be on disk.
                                save_settings(&app.provider_configs, &app.pool_entries);
                                let label = app
                                    .providers
                                    .iter()
                                    .find(|p| p.id == provider_id)
                                    .map(|p| p.display_name.clone())
                                    .unwrap_or_else(|| provider_id.clone());
                                if was_in {
                                    app.toast
                                        .info(format!("Removed {} / {}", label, model_name));
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
                                app.toast
                                    .info(format!("Removed {} / {}", label, removed.model));
                            }
                        }
                        Action::SettingsConfirm => {
                            // Pool entries and providers are persisted
                            // eagerly on every individual mutation, so
                            // Confirm's only remaining job is to push
                            // the assembled pool into the live agents.
                            if app.pool_entries.is_empty() {
                                app.toast
                                    .warning("Pool is empty \u{2014} no changes applied");
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
                                    app.toast
                                        .error("Failed to build model pool from selections");
                                }
                            }
                            app.settings_modal_open = false;
                        }
                        Action::SettingsNewProvider => {
                            if app.settings_pane == SettingsPane::Provider {
                                app.new_provider_form = Some(crate::state::NewProviderForm::new());
                            } else {
                                app.toast
                                    .info("Switch to the Providers pane to add a new one");
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
                                        // `remove_custom_provider` also
                                        // drops the provider's pool
                                        // entries; persist immediately
                                        // so the on-disk file is in sync.
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
                                    // Reset URL preset to the first non-custom
                                    // option for the new provider type.
                                    form.url_preset_idx = form
                                        .presets()
                                        .iter()
                                        .position(|p| !p.is_custom)
                                        .unwrap_or(0);
                                    form.url_custom.clear();
                                    form.error = None;
                                } else if form.active_field == 3 {
                                    // Cycling the URL preset.
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
                                    1 => {
                                        form.display_name.pop();
                                    }
                                    2 => {
                                        form.api_key.pop();
                                    }
                                    3 if form.url_is_custom() => {
                                        form.url_custom.pop();
                                    }
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
                                    // New provider is durable immediately —
                                    // if the user quits before reopening
                                    // settings, it still has to come back.
                                    save_settings(&app.provider_configs, &app.pool_entries);
                                    app.toast.success(format!(
                                        "Added custom provider: {} ({})",
                                        display_name, ptype
                                    ));
                                    // Select the newly added provider.
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
                    },
                    Event::Paste(s) if app.new_provider_form.is_some() => {
                        // Paste inside the new-provider form: drop the
                        // full text into the active field. We do not
                        // summarize here — API keys and URLs are
                        // single-line by nature, so a paste shouldn't
                        // turn into a `[Pasted ~N lines]` placeholder.
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
                        if let Some(placeholder) = summarize_paste(&s) {
                            // Long paste: store full text in a side-channel
                            // and push a compact placeholder into the input
                            // field. The placeholder is substituted back to
                            // the full text at submission time.
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
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
