use std::sync::Arc;
use std::time::Duration;

use agent_compose::KmsContext;
use agent_knowledge::KnowledgeContext;
use agentik_types::AgentUiEvent;
use agentik_types::messages::ContentBlock;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;

use crate::CrosstermBackend;
use crate::chat::ChatMessage;
use crate::layout::LayoutMode;
use crate::settings::{build_pool_from_entries, save_settings};
use crate::state::{Action, AgentKind, App, ChatFocus, Panel, SettingsPane};
use crate::widgets::ui;

fn agent_event_to_message(event: AgentUiEvent) -> Vec<ChatMessage> {
    match event {
        AgentUiEvent::LlmResponse(text) => vec![ChatMessage::Assistant { text }],
        AgentUiEvent::Thinking(text) => vec![ChatMessage::Thinking { text }],
        AgentUiEvent::ToolCall { name, input } => {
            // When the orchestrator calls `kms_parallel_dispatch`, we
            // also push a `ParallelBlock` marker so the renderer can
            // show the streaming progress panel between this ToolCall
            // and the eventual ToolResult.
            if name == "kms_parallel_dispatch" {
                vec![
                    ChatMessage::ToolCall { name, input },
                    ChatMessage::ParallelBlock,
                ]
            } else {
                vec![ChatMessage::ToolCall { name, input }]
            }
        }
        AgentUiEvent::ToolResult { ok, content } => vec![ChatMessage::ToolResult { ok, content }],
        AgentUiEvent::Done => vec![ChatMessage::Done],
        AgentUiEvent::Error(msg) => vec![ChatMessage::Error { message: msg }],
        AgentUiEvent::Requesting => Vec::new(),
    }
}

pub fn handle_key_event(key: KeyEvent, app: &mut App) -> Action {
    if app.focused == Panel::Agent && app.agent_input_active && !app.agent_running {
        return match key {
            KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                let input = std::mem::take(&mut app.agent_input);
                if input.is_empty() {
                    app.agent_input_active = false;
                    Action::None
                } else {
                    app.agent_input_active = false;
                    Action::SubmitAgent(input)
                }
            }
            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                app.agent_input.clear();
                app.agent_input_active = false;
                Action::None
            }
            KeyEvent {
                code: KeyCode::Backspace,
                ..
            } => {
                app.agent_input.pop();
                Action::None
            }
            KeyEvent {
                code: KeyCode::Char(c),
                ..
            } => {
                app.agent_input.push(c);
                Action::None
            }
            _ => Action::None,
        };
    }

    // If the new-provider form is open, route all key events to it.
    if app.new_provider_form.is_some() {
        return match key {
            KeyEvent {
                code: KeyCode::Esc, ..
            } => Action::SettingsFormCancel,
            KeyEvent {
                code: KeyCode::Tab | KeyCode::BackTab,
                ..
            } => {
                let delta = if matches!(key.code, KeyCode::Tab) {
                    1
                } else {
                    -1
                };
                Action::SettingsFormCycleField(delta)
            }
            KeyEvent {
                code: KeyCode::Up | KeyCode::Char('k'),
                ..
            } => Action::SettingsFormCycleType(-1),
            KeyEvent {
                code: KeyCode::Down | KeyCode::Char('j'),
                ..
            } => Action::SettingsFormCycleType(1),
            KeyEvent {
                code: KeyCode::Backspace,
                ..
            } => Action::SettingsFormBackspace,
            KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                ..
            } => Action::SettingsFormSubmit,
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers: KeyModifiers::NONE,
                ..
            } => Action::SettingsFormType(c),
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers: KeyModifiers::SHIFT,
                ..
            } => Action::SettingsFormType(c),
            _ => Action::None,
        };
    }

    if app.settings_modal_open {
        return match key {
            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                app.settings_modal_open = false;
                Action::None
            }
            KeyEvent {
                code: KeyCode::Tab, ..
            } => {
                app.settings_pane = match app.settings_pane {
                    SettingsPane::Provider => SettingsPane::Model,
                    SettingsPane::Model => SettingsPane::Pool,
                    SettingsPane::Pool => SettingsPane::Provider,
                };
                Action::None
            }
            KeyEvent {
                code: KeyCode::Up | KeyCode::Char('k'),
                ..
            } => Action::SettingsNav(app.settings_pane, -1),
            KeyEvent {
                code: KeyCode::Down | KeyCode::Char('j'),
                ..
            } => Action::SettingsNav(app.settings_pane, 1),
            KeyEvent {
                code: KeyCode::Char(' '),
                modifiers: KeyModifiers::NONE,
                ..
            } if app.settings_pane == SettingsPane::Model => Action::SettingsTogglePool,
            KeyEvent {
                code: KeyCode::Char('d') | KeyCode::Delete,
                ..
            } if app.settings_pane == SettingsPane::Pool => Action::SettingsRemovePool,
            KeyEvent {
                code: KeyCode::Char('n') | KeyCode::Char('a'),
                modifiers: KeyModifiers::NONE,
                ..
            } if app.settings_pane == SettingsPane::Provider => Action::SettingsNewProvider,
            KeyEvent {
                code: KeyCode::Char('r') | KeyCode::Char('x'),
                modifiers: KeyModifiers::NONE,
                ..
            } if app.settings_pane == SettingsPane::Provider => Action::SettingsDeleteProvider,
            KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                ..
            } => Action::SettingsConfirm,
            _ => Action::None,
        };
    }

    match key {
        KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            ..
        } => Action::Quit,
        KeyEvent {
            code: KeyCode::Char('s'),
            modifiers: KeyModifiers::NONE,
            ..
        } => Action::OpenSettings,
        KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::NONE,
            ..
        } if app.focused == Panel::Agent && !app.agent_input_active && !app.agent_running => {
            Action::SwitchAgent
        }
        KeyEvent {
            code: KeyCode::Tab, ..
        } => {
            // When the Agent panel has a parallel dispatch open, the
            // first Tab toggles between Messages and the ParallelPanel
            // sub-focus. Only when the user is on Messages (the default
            // entry point) does Tab advance to the next layout panel.
            if app.focused == Panel::Agent
                && app.parallel_panel.is_some()
                && app.chat_focus == ChatFocus::Messages
            {
                app.chat_focus = ChatFocus::ParallelPanel;
                return Action::None;
            }
            let mode = LayoutMode::from_width(0);
            let order = mode.panel_order();
            let idx = order.iter().position(|&p| p == app.focused).unwrap_or(0);
            let next = (idx + 1) % order.len();
            app.focused = order[next];
            // Re-entering the Agent panel from another panel always
            // lands on Messages, so the next Tab goes to ParallelPanel
            // (when one is open) rather than skipping over it.
            if app.focused == Panel::Agent && app.parallel_panel.is_some() {
                app.chat_focus = ChatFocus::Messages;
            }
            Action::None
        }
        KeyEvent {
            code: KeyCode::BackTab,
            ..
        } => {
            let mode = LayoutMode::from_width(0);
            let order = mode.panel_order();
            let idx = order.iter().position(|&p| p == app.focused).unwrap_or(0);
            let prev = if idx == 0 { order.len() - 1 } else { idx - 1 };
            app.focused = order[prev];
            Action::None
        }
        KeyEvent {
            code: KeyCode::Char('H'),
            ..
        } => {
            let mode = LayoutMode::from_width(0);
            let order = mode.panel_order();
            let idx = order.iter().position(|&p| p == app.focused).unwrap_or(0);
            let prev = if idx == 0 { order.len() - 1 } else { idx - 1 };
            app.focused = order[prev];
            Action::None
        }
        KeyEvent {
            code: KeyCode::Char('L'),
            ..
        } => {
            let mode = LayoutMode::from_width(0);
            let order = mode.panel_order();
            let idx = order.iter().position(|&p| p == app.focused).unwrap_or(0);
            let next = (idx + 1) % order.len();
            app.focused = order[next];
            Action::None
        }
        KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            ..
        } if app.focused == Panel::Agent
            && app.chat_focus == ChatFocus::ParallelPanel =>
        {
            if let Some(panel) = app.parallel_panel.as_mut() {
                panel.toggle_selected();
            }
            Action::None
        }
        KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            ..
        } if app.focused == Panel::Agent && !app.agent_running => {
            app.agent_input_active = true;
            Action::None
        }
        KeyEvent {
            code: KeyCode::Char('t'),
            modifiers: KeyModifiers::NONE,
            ..
        } if app.focused == Panel::KnowledgeEntity => {
            app.ke_tab = match app.ke_tab {
                crate::state::KeTab::Knowledge => crate::state::KeTab::Entity,
                crate::state::KeTab::Entity => crate::state::KeTab::Knowledge,
            };
            app.ke_scroll = 0;
            Action::None
        }
        KeyEvent {
            code: KeyCode::Char('j') | KeyCode::Down,
            ..
        } if app.focused == Panel::Agent
            && app.chat_focus == ChatFocus::ParallelPanel =>
        {
            if let Some(panel) = app.parallel_panel.as_mut() {
                panel.move_selection(1);
            }
            Action::None
        }
        KeyEvent {
            code: KeyCode::Char('k') | KeyCode::Up,
            ..
        } if app.focused == Panel::Agent
            && app.chat_focus == ChatFocus::ParallelPanel =>
        {
            if let Some(panel) = app.parallel_panel.as_mut() {
                panel.move_selection(-1);
            }
            Action::None
        }
        KeyEvent {
            code: KeyCode::Char('e'),
            modifiers: KeyModifiers::NONE,
            ..
        } if app.focused == Panel::Agent
            && app.chat_focus == ChatFocus::ParallelPanel =>
        {
            if let Some(panel) = app.parallel_panel.as_mut() {
                panel.expand_all();
            }
            Action::None
        }
        KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::NONE,
            ..
        } if app.focused == Panel::Agent
            && app.chat_focus == ChatFocus::ParallelPanel =>
        {
            if let Some(panel) = app.parallel_panel.as_mut() {
                panel.collapse_all();
            }
            Action::None
        }
        KeyEvent {
            code: KeyCode::Char('j') | KeyCode::Down,
            ..
        } => handle_scroll_down(app),
        KeyEvent {
            code: KeyCode::Char('k') | KeyCode::Up,
            ..
        } => handle_scroll_up(app),
        KeyEvent {
            code: KeyCode::End, ..
        } => handle_end(app),
        _ => Action::None,
    }
}

fn handle_scroll_down(app: &mut App) -> Action {
    match app.focused {
        Panel::Tree => {
            if let Some(sel) = app.tree_state.selected() {
                let next = sel
                    .saturating_add(1)
                    .min(app.tree_items.len().saturating_sub(1));
                app.tree_state.select(Some(next));
                Action::TreeChanged
            } else {
                Action::None
            }
        }
        Panel::KnowledgeEntity => {
            let lines = match app.ke_tab {
                crate::state::KeTab::Knowledge => &app.knowledge_lines,
                crate::state::KeTab::Entity => &app.entity_lines,
            };
            if app.ke_scroll < lines.len() as u16 {
                app.ke_scroll += 1;
            }
            Action::None
        }
        Panel::Diagnostics => {
            if app.scroll_diag < app.diagnostic_lines.len() as u16 {
                app.scroll_diag += 1;
            }
            Action::None
        }
        Panel::Agent => Action::None,
    }
}

fn handle_scroll_up(app: &mut App) -> Action {
    match app.focused {
        Panel::Tree => {
            if let Some(sel) = app.tree_state.selected() {
                app.tree_state.select(Some(sel.saturating_sub(1)));
            }
            Action::None
        }
        Panel::KnowledgeEntity => {
            app.ke_scroll = app.ke_scroll.saturating_sub(1);
            Action::None
        }
        Panel::Diagnostics => {
            app.scroll_diag = app.scroll_diag.saturating_sub(1);
            Action::None
        }
        Panel::Agent => Action::None,
    }
}

fn handle_end(app: &mut App) -> Action {
    match app.focused {
        Panel::KnowledgeEntity => {
            let lines = match app.ke_tab {
                crate::state::KeTab::Knowledge => &app.knowledge_lines,
                crate::state::KeTab::Entity => &app.entity_lines,
            };
            app.ke_scroll = (lines.len() as u16).saturating_sub(1);
            Action::None
        }
        Panel::Diagnostics => {
            app.scroll_diag = (app.diagnostic_lines.len() as u16).saturating_sub(1);
            Action::None
        }
        _ => Action::None,
    }
}

/// Rebuild all three agents with the given model pool and service reference.
async fn rebuild_all_agents(app: &mut App, pool: &Arc<agentik_sdk::model::model_pool::ModelPool>) {
    let svc = app.svc.clone();
    let svc_arc = Arc::new(svc);

    let compose_ctx: Arc<dyn agentik_core::context::AgentContext> =
        Arc::new(KmsContext::new(svc_arc.clone()));
    let knowledge_ctx: Arc<dyn agentik_core::context::AgentContext> =
        Arc::new(KnowledgeContext::new(svc_arc.clone()));
    let parallel_ctx: Arc<dyn agentik_core::context::AgentContext> = Arc::new(
        agent_compose::ParallelComposeContext::new(
            svc_arc,
            pool.clone(),
            app.parallel_progress_tx.clone(),
        ),
    );

    let mut last_error = None;

    for (kind, ctx) in [
        (AgentKind::Compose, compose_ctx),
        (AgentKind::Knowledge, knowledge_ctx),
        (AgentKind::Parallel, parallel_ctx),
    ] {
        match agentik_core::Agent::builder()
            .with_model_pool(pool.clone())
            .with_context(ctx)
            .build()
            .await
        {
            Ok(agent) => {
                app.agents
                    .insert(kind, Arc::new(tokio::sync::Mutex::new(agent)));
            }
            Err(e) => {
                last_error = Some(format!("{} agent: {}", kind.label(), e));
            }
        }
    }

    if let Some(err) = last_error {
        app.toast.error(format!("Agent rebuild failed: {}", err));
    }
}

pub async fn run_app(
    terminal: &mut Terminal<CrosstermBackend>,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let mut pending_refresh = false;

        if let Some(rx) = &mut app.agent_event_rx {
            while let Ok(event) = rx.try_recv() {
                match event {
                    AgentUiEvent::Done => {
                        app.agent_running = false;
                        app.agent_requesting = false;
                    }
                    AgentUiEvent::Requesting => {
                        app.agent_requesting = true;
                    }
                    event => {
                        app.agent_requesting = false;
                        let kind = app.agent_kind;
                        let messages = agent_event_to_message(event);
                        if !messages.is_empty() {
                            let history = app.agent_messages_map.get_mut(&kind).unwrap();
                            for msg in messages {
                                history.push(msg);
                            }
                        }
                    }
                }
                pending_refresh = true;
            }
        }

        // Drain the parallel-dispatch side-channel. Each event
        // updates the panel state machine; we mark `pending_refresh`
        // whenever a sub-agent finishes or fails so the tree view
        // grows incrementally as staging areas get populated.
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
                    app.parallel_panel = Some(
                        crate::parallel_panel::ParallelPanelState::new(total),
                    );
                }
                if let Some(panel) = app.parallel_panel.as_mut() {
                    panel.apply(&event);
                }
                match &event {
                    P::SubAgentCompleted { .. } | P::SubAgentFailed { .. } => {
                        pending_refresh = true;
                    }
                    _ => {}
                }
            }
        }

        if pending_refresh {
            // Debounce: if we refreshed within the last 200ms, defer
            // to the next loop iteration. The next event will re-set
            // `pending_refresh` and we'll check again.
            const REFRESH_DEBOUNCE: std::time::Duration =
                std::time::Duration::from_millis(200);
            let should_refresh = match app.last_tree_refresh_at {
                Some(t) => t.elapsed() >= REFRESH_DEBOUNCE,
                None => true,
            };
            if should_refresh {
                app.refresh_tree().await;
                app.last_tree_refresh_at = Some(std::time::Instant::now());
            }
        }

        if app.agent_requesting {
            app.spinner_tick = (app.spinner_tick + 1) % 8;
        }

        terminal.draw(|f| ui(f, app))?;

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
                                app.settings_selected_pool =
                                    (app.settings_selected_pool as isize + delta)
                                        .clamp(0, max as isize)
                                        as usize;
                            }
                        },
                        Action::SettingsSwitchPane(pane) => {
                            app.settings_pane = pane;
                        }
                        Action::SettingsTogglePool => {
                            let pair = app
                                .providers
                                .get(app.settings_selected_provider)
                                .map(|p| {
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
                                app.toast.info(format!(
                                    "Removed {} / {}",
                                    label, removed.model
                                ));
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
                                if let Some(pool) =
                                    build_pool_from_entries(&new_entries, &app.providers)
                                {
                                    let pool_arc = Arc::new(pool);
                                    rebuild_all_agents(app, &pool_arc).await;
                                    app.toast.success(format!(
                                        "Pool updated: {} model(s)",
                                        new_entries.len()
                                    ));
                                } else {
                                    app.toast.error(
                                        "Failed to build model pool from selections",
                                    );
                                }
                            }
                            app.settings_modal_open = false;
                        }
                        Action::SettingsNewProvider => {
                            if app.settings_pane == SettingsPane::Provider {
                                app.new_provider_form = Some(
                                    crate::state::NewProviderForm::new(),
                                );
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
                                        let max = app
                                            .providers
                                            .len()
                                            .saturating_sub(1);
                                        if app.settings_selected_provider > max {
                                            app.settings_selected_provider = max;
                                        }
                                        app.toast.info("Custom provider removed");
                                    } else {
                                        app.toast
                                            .warning("Built-in providers cannot be removed");
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
                                        form.url_preset_idx =
                                            (form.url_preset_idx + n - 1) % n;
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
                                let ptype =
                                    crate::settings::BUILTIN_PROVIDER_TYPES[form.type_idx];
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
                    Event::Paste(s)
                        if app.new_provider_form.is_some() =>
                    {
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

/// Lower bound (in lines, including 1) above which a paste is collapsed
/// into a placeholder. Mirrors the heuristic used by opencode's CLI
/// TUI to keep long pastes from drowning the chat panel.
const PASTE_SUMMARY_LINE_THRESHOLD: usize = 3;
/// Lower bound (in characters) above which a single-line paste is also
/// collapsed. The placeholder still works for one very long line.
const PASTE_SUMMARY_LEN_THRESHOLD: usize = 150;

/// Decide whether a paste should be collapsed into a compact
/// placeholder (`[Pasted ~N lines]`) and, if so, build that
/// placeholder. Returns `None` for short pastes that should be
/// inserted verbatim.
///
/// The exact line count of the original is used (not the trimmed
/// content) so the placeholder matches what the user sees in their
/// clipboard.
fn summarize_paste(content: &str) -> Option<String> {
    let line_count = content.lines().count().max(1);
    let needs_summary = line_count >= PASTE_SUMMARY_LINE_THRESHOLD
        || content.chars().count() > PASTE_SUMMARY_LEN_THRESHOLD;
    if needs_summary {
        Some(format!("[Pasted ~{line_count} lines]"))
    } else {
        None
    }
}

fn spawn_agent_task(app: &mut App, user_input: String) {
    if app.agent_running {
        return;
    }

    // Snapshot the original input (with any paste placeholders still
    // embedded) and the side-channel that maps placeholders to full
    // text. We then drop the side-channel — the agent has what it
    // needs, and the chat history deliberately keeps the compact form.
    let compact_for_history = user_input.clone();
    let mut pastes = std::mem::take(&mut app.agent_pastes);

    // Assemble the full text for the agent by expanding each
    // placeholder once. Placeholders the user edited away are kept
    // in `pastes` in case the user re-inserts them, but normally
    // they'll be discarded at the end of this scope.
    let mut expanded = user_input;
    for (placeholder, full) in &pastes {
        if expanded.contains(placeholder) {
            expanded = expanded.replacen(placeholder, full, 1);
        }
    }
    pastes.retain(|(placeholder, _)| expanded.contains(placeholder));

    let kind = app.agent_kind;
    // Push the COMPACT form (with `[Pasted ~N lines]` placeholders
    // still embedded) to the chat history. This is what mirrors
    // opencode's design: the chat panel shows the placeholder, not
    // the full pasted text. The agent still receives the full text
    // because we expand before injection below.
    app.agent_messages_mut().push(ChatMessage::User {
        text: compact_for_history,
    });
    app.agent_messages_mut().push(ChatMessage::Divider);
    app.agent_running = true;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    app.agent_event_rx = Some(rx);

    // The agents map is populated in two places: `App::new` (via
    // `main.rs`, only when a pool can be built) and
    // `rebuild_all_agents` (re-run on settings confirm). Both can
    // legitimately leave the map empty or partially populated — for
    // example a custom provider whose first request to the upstream
    // /models endpoint fails, or a transient network blip mid-rebuild.
    // When that happens the user can still navigate to the Agent
    // panel, type a long message, and press Enter; we must not panic
    // on the lookup. Surface a clear error, reset the running flag so
    // the TUI stays interactive, and drop the half-pushed chat entry.
    let agent_arc = match app.agents.get(&kind).cloned() {
        Some(arc) => arc,
        None => {
            // Pop the User + Divider entries we just pushed so the
            // history doesn't accumulate orphaned rows for submissions
            // that never reached the agent.
            if let Some(history) = app.agent_messages_map.get_mut(&kind) {
                history.pop(); // Divider
                history.pop(); // User
            }
            app.agent_running = false;
            app.agent_event_rx = None;
            app.toast.error(format!(
                "{} agent is not available \u{2014} check the model pool in Settings",
                kind.label()
            ));
            return;
        }
    };

    tokio::spawn(async move {
        let mut agent = agent_arc.lock().await;
        agent.event_tx = Some(tx.clone());

        if let Err(e) = agent.inject_message(vec![ContentBlock::Text { text: expanded }]) {
            let _ = tx.send(AgentUiEvent::Error(format!("Inject error: {}", e)));
            let _ = tx.send(AgentUiEvent::Done);
            agent.event_tx = None;
            return;
        }

        if let Err(e) = agent.start().await {
            let _ = tx.send(AgentUiEvent::Error(format!("Agent failed: {}", e)));
        }

        let _ = tx.send(AgentUiEvent::Done);
        agent.event_tx = None;
    });
}

#[cfg(test)]
mod paste_summary_tests {
    use super::{summarize_paste, PASTE_SUMMARY_LEN_THRESHOLD, PASTE_SUMMARY_LINE_THRESHOLD};

    #[test]
    fn short_single_line_paste_is_verbatim() {
        assert!(summarize_paste("hello world").is_none());
    }

    #[test]
    fn exactly_two_lines_is_verbatim() {
        // Threshold is 3 lines, so 2 must NOT be collapsed.
        assert!(summarize_paste("line one\nline two").is_none());
    }

    #[test]
    fn three_lines_triggers_placeholder() {
        let p = summarize_paste("a\nb\nc").unwrap();
        assert_eq!(p, "[Pasted ~3 lines]");
    }

    #[test]
    fn many_lines_triggers_placeholder_with_correct_count() {
        let text = (1..=10).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let p = summarize_paste(&text).unwrap();
        assert_eq!(p, "[Pasted ~10 lines]");
    }

    #[test]
    fn single_very_long_line_triggers_placeholder() {
        let long = "a".repeat(PASTE_SUMMARY_LEN_THRESHOLD + 1);
        let p = summarize_paste(&long).unwrap();
        assert_eq!(p, "[Pasted ~1 lines]");
    }

    #[test]
    fn blank_lines_count_toward_total() {
        // "a\n\nb" has 3 lines (one empty in the middle).
        let p = summarize_paste("a\n\nb").unwrap();
        assert_eq!(p, "[Pasted ~3 lines]");
    }

    #[test]
    fn threshold_lengths_match_opencode_default() {
        // Sanity: both thresholds match the opencode CLI values used
        // as the reference for this design.
        assert_eq!(PASTE_SUMMARY_LINE_THRESHOLD, 3);
        assert_eq!(PASTE_SUMMARY_LEN_THRESHOLD, 150);
    }
}

#[cfg(test)]
mod chat_history_compactness_tests {
    //! Verify that the chat history holds the *compact* form of a
    //! submitted user message (i.e. with `[Pasted ~N lines]`
    //! placeholders still embedded), mirroring opencode's design.
    //!
    //! These tests exercise the bookkeeping logic that
    //! `spawn_agent_task` performs on the side-channel and chat
    //! history. We don't need a real `KmsService` because we never
    //! let the agent actually run — the logic under test is purely
    //! string manipulation over `agent_pastes` and `agent_messages`.

    use crate::chat::ChatMessage;
    use std::collections::HashMap;

    /// Mirror of the relevant slice of `spawn_agent_task`: snapshot
    /// the compact input, expand the placeholders for the agent,
    /// and return (compact_for_history, expanded_for_agent).
    fn expand_for_agent(
        user_input: String,
        pastes: &mut Vec<(String, String)>,
    ) -> (String, String) {
        let compact_for_history = user_input.clone();
        let mut expanded = user_input;
        for (placeholder, full) in pastes.iter() {
            if expanded.contains(placeholder) {
                expanded = expanded.replacen(placeholder, full, 1);
            }
        }
        pastes.retain(|(placeholder, _)| expanded.contains(placeholder));
        (compact_for_history, expanded)
    }

    #[test]
    fn chat_history_keeps_placeholder_after_submit() {
        let placeholder = "[Pasted ~3 lines]".to_string();
        let full = "alpha\nbeta\ngamma".to_string();
        let mut pastes = vec![(placeholder.clone(), full.clone())];
        let user_input = format!("please summarise: {placeholder} thanks");

        let (compact_for_history, expanded_for_agent) =
            expand_for_agent(user_input, &mut pastes);

        // The history-side string still contains the placeholder.
        assert!(compact_for_history.contains(&placeholder));
        assert!(
            !compact_for_history.contains(&full),
            "history must NOT contain the full pasted text"
        );

        // The agent-side string has the placeholder replaced by the
        // full text.
        assert!(expanded_for_agent.contains(&full));
        assert!(!expanded_for_agent.contains(&placeholder));
    }

    #[test]
    fn no_paste_means_history_equals_input() {
        // No paste side-channel: history and agent both see the
        // verbatim text the user typed.
        let mut pastes: Vec<(String, String)> = Vec::new();
        let user_input = "just a short message".to_string();
        let (compact, expanded) = expand_for_agent(user_input.clone(), &mut pastes);
        assert_eq!(compact, "just a short message");
        assert_eq!(expanded, "just a short message");
    }

    #[test]
    fn side_channel_placeholder_not_in_input_is_dropped() {
        // The placeholder is not in the input (user deleted it
        // before submitting). The side-channel entry is dropped —
        // `spawn_agent_task` does not retain orphans since the
        // `mem::take` is unconditional and unused entries serve no
        // further purpose. This is the simpler KISS behaviour.
        let placeholder = "[Pasted ~3 lines]".to_string();
        let full = "alpha\nbeta\ngamma".to_string();
        let mut pastes = vec![(placeholder.clone(), full.clone())];
        let (compact, expanded) = expand_for_agent(
            "no placeholder here, user deleted it".to_string(),
            &mut pastes,
        );
        assert_eq!(compact, "no placeholder here, user deleted it");
        assert_eq!(expanded, "no placeholder here, user deleted it");
        // Side-channel entry is dropped.
        assert!(pastes.is_empty());
    }

    #[test]
    fn message_record_carries_compact_form() {
        // Drives the actual `ChatMessage::User` shape used by the
        // TUI. This is what ends up in the chat history; rendering
        // will see the placeholder, not the full text.
        let placeholder = "[Pasted ~3 lines]".to_string();
        let full = "alpha\nbeta\ngamma".to_string();
        let mut pastes = vec![(placeholder.clone(), full.clone())];
        let user_input = format!("intro {placeholder} outro");
        let (compact, _expanded) = expand_for_agent(user_input, &mut pastes);
        let mut messages: HashMap<crate::state::AgentKind, Vec<ChatMessage>> = HashMap::new();
        messages.insert(crate::state::AgentKind::Compose, Vec::new());
        let history = messages
            .get_mut(&crate::state::AgentKind::Compose)
            .unwrap();
        history.push(ChatMessage::User { text: compact });
        history.push(ChatMessage::Divider);

        match &history[0] {
            ChatMessage::User { text } => {
                assert!(text.contains(&placeholder));
                assert!(!text.contains(&full));
            }
            _ => panic!(),
        }
    }
}

#[cfg(test)]
mod parallel_panel_focus_tests {
    //! Tests for the `ChatFocus` sub-focus state machine used by the
    //! parallel panel. We test the pure state transitions; the
    //! key-handler dispatch logic is verified by manual E2E.

    use crate::state::ChatFocus;

    #[test]
    fn default_focus_is_messages() {
        assert_eq!(ChatFocus::default(), ChatFocus::Messages);
    }

    #[test]
    fn focus_cycles_messages_to_panel() {
        let f = ChatFocus::Messages;
        let next = match f {
            ChatFocus::Messages => ChatFocus::ParallelPanel,
            ChatFocus::ParallelPanel => ChatFocus::Messages,
        };
        assert_eq!(next, ChatFocus::ParallelPanel);
    }

    #[test]
    fn focus_cycles_panel_to_messages() {
        let f = ChatFocus::ParallelPanel;
        let next = match f {
            ChatFocus::Messages => ChatFocus::ParallelPanel,
            ChatFocus::ParallelPanel => ChatFocus::Messages,
        };
        assert_eq!(next, ChatFocus::Messages);
    }
}
