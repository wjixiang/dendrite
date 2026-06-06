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
use crate::settings::save_settings;
use crate::state::{Action, AgentKind, App, Panel, SettingsPane};
use crate::widgets::ui;

fn agent_event_to_message(event: AgentUiEvent) -> Option<ChatMessage> {
    match event {
        AgentUiEvent::LlmResponse(text) => Some(ChatMessage::Assistant { text }),
        AgentUiEvent::Thinking(text) => Some(ChatMessage::Thinking { text }),
        AgentUiEvent::ToolCall { name, input } => Some(ChatMessage::ToolCall { name, input }),
        AgentUiEvent::ToolResult { ok, content } => Some(ChatMessage::ToolResult { ok, content }),
        AgentUiEvent::Done => Some(ChatMessage::Done),
        AgentUiEvent::Error(msg) => Some(ChatMessage::Error { message: msg }),
        AgentUiEvent::Requesting => None,
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
                    SettingsPane::Model => SettingsPane::Provider,
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
            let mode = LayoutMode::from_width(0);
            let order = mode.panel_order();
            let idx = order.iter().position(|&p| p == app.focused).unwrap_or(0);
            let next = (idx + 1) % order.len();
            app.focused = order[next];
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
        } => handle_scroll_down(app),
        KeyEvent {
            code: KeyCode::Char('k') | KeyCode::Up,
            ..
        } => handle_scroll_up(app),
        KeyEvent {
            code: KeyCode::End, ..
        } => handle_end(app),
        KeyEvent {
            code: KeyCode::Char('G'),
            ..
        } if app.focused == Panel::Agent && !app.agent_input_active => {
            // "G" — re-anchor the Agent panel to the bottom and
            // re-enable auto-follow, in one keystroke. The render
            // pass clamps the oversized scroll offset back to the
            // real last line, so the user always lands at the
            // tail.
            app.agent_auto_follow = true;
            app.agent_scroll = u16::MAX;
            Action::None
        }
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
        Panel::Agent => {
            // Any manual scroll disengages auto-follow. The user
            // has expressed an intent to inspect older messages;
            // silently re-snapping to the bottom on the next
            // event would fight that intent.
            app.agent_auto_follow = false;
            // Bound checked at render time. u16::MAX is impossible
            // to reach via single `+1` increments from a u16::MAX-1
            // starting point, so no overflow here.
            app.agent_scroll = app.agent_scroll.saturating_add(1);
            Action::None
        }
    }
}

fn handle_scroll_up(app: &mut App) -> Action {
    match app.focused {
        Panel::Tree => {
            if let Some(sel) = app.tree_state.selected() {
                app.tree_state.select(Some(sel.saturating_sub(1)));
                Action::TreeChanged
            } else {
                Action::None
            }
        }
        Panel::KnowledgeEntity => {
            app.ke_scroll = app.ke_scroll.saturating_sub(1);
            Action::None
        }
        Panel::Diagnostics => {
            app.scroll_diag = app.scroll_diag.saturating_sub(1);
            Action::None
        }
        Panel::Agent => {
            app.agent_auto_follow = false;
            app.agent_scroll = app.agent_scroll.saturating_sub(1);
            Action::None
        }
    }
}

fn handle_end(app: &mut App) -> Action {
    match app.focused {
        Panel::Agent => {
            // End in the Agent panel re-enables auto-follow and
            // snaps to the bottom (overshoot; the render pass
            // clamps to the real last visual line).
            app.agent_auto_follow = true;
            app.agent_scroll = u16::MAX;
            Action::None
        }
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
                        if let Some(msg) = agent_event_to_message(event) {
                            let kind = app.agent_kind;
                            app.agent_messages_map.get_mut(&kind).unwrap().push(msg);
                            // Auto-follow: if the user has not
                            // disengaged, anchor the scroll to the
                            // bottom of the new content. The
                            // render pass will clamp the overshoot
                            // to the true last visual line.
                            if app.agent_auto_follow {
                                app.agent_scroll = u16::MAX;
                            }
                        }
                    }
                }
                pending_refresh = true;
            }
        }

        if pending_refresh {
            app.refresh_tree().await;
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
                            // Reset scroll AND auto-follow when
                            // switching agents; the new
                            // conversation should start at the top
                            // and follow new content.
                            app.agent_scroll = 0;
                            app.agent_auto_follow = true;
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
                            }
                            SettingsPane::Model => {
                                let provider_idx = app.settings_selected_provider;
                                if let Some(provider) = app.providers.get(provider_idx) {
                                    let max = provider.models.len().saturating_sub(1);
                                    app.settings_selected_model =
                                        (app.settings_selected_model as isize + delta)
                                            .clamp(0, max as isize)
                                            as usize;
                                }
                            }
                        },
                        Action::SettingsSwitchPane(pane) => {
                            app.settings_pane = pane;
                        }
                        Action::SettingsConfirm => {
                            let provider_idx = app.settings_selected_provider;
                            let model_idx = app.settings_selected_model;

                            let new_provider =
                                app.providers.get(provider_idx).map(|p| p.name.clone());
                            let new_model = app
                                .providers
                                .get(provider_idx)
                                .and_then(|p| p.models.get(model_idx).cloned());

                            if let (Some(new_provider), Some(new_model)) = (new_provider, new_model)
                                && (new_provider != app.current_provider
                                    || new_model != app.current_model)
                                && let Some(pool) = build_pool(&new_provider, &new_model)
                            {
                                let svc_arc = Arc::new(app.svc.clone());
                                let ctx: Arc<dyn agentik_core::context::AgentContext> =
                                    match app.agent_kind {
                                        AgentKind::Compose => Arc::new(KmsContext::new(svc_arc)),
                                        AgentKind::Knowledge => {
                                            Arc::new(KnowledgeContext::new(svc_arc))
                                        }
                                    };

                                let new_agent = agentik_core::Agent::builder()
                                    .with_model_pool(Arc::new(pool))
                                    .with_context(ctx)
                                    .build()
                                    .await
                                    .map_err(|e| e.to_string())?;

                                app.agents.insert(
                                    app.agent_kind,
                                    Arc::new(tokio::sync::Mutex::new(new_agent)),
                                );

                                app.current_provider = new_provider.clone();
                                app.current_model = new_model.clone();
                                save_settings(&new_provider, &new_model);
                                app.toast.success(format!(
                                    "Switched to {} / {}",
                                    app.current_provider, app.current_model
                                ));
                            }
                            app.settings_modal_open = false;
                        }
                        Action::None => {}
                    },
                    Event::Paste(s)
                        if app.focused == Panel::Agent
                            && app.agent_input_active
                            && !app.agent_running =>
                    {
                        app.agent_input.push_str(&s);
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

fn build_pool(provider: &str, model: &str) -> Option<agentik_sdk::model::model_pool::ModelPool> {
    use agentik_sdk::model::model_pool::ModelPool;
    use agentik_sdk::provider::LlmProvider;

    match provider {
        "mimo" => {
            let mimo_provider = agentik_sdk::provider::mimo::MimoProvider::new(None, None, None);
            let m = mimo_provider.get_model(model).ok()?;
            let mut pool = ModelPool::new();
            pool.add_model(m);
            Some(pool)
        }
        "minimax" => {
            let minimax_provider =
                agentik_sdk::provider::minimax::MinimaxProvider::new(None, None, None);
            let m = minimax_provider.get_model(model).ok()?;
            let mut pool = ModelPool::new();
            pool.add_model(m);
            Some(pool)
        }
        _ => None,
    }
}

fn spawn_agent_task(app: &mut App, user_input: String) {
    if app.agent_running {
        return;
    }

    let kind = app.agent_kind;
    app.agent_messages_mut().push(ChatMessage::User {
        text: user_input.clone(),
    });
    app.agent_messages_mut().push(ChatMessage::Divider);
    app.agent_running = true;
    // Submitting a new task is the canonical signal to follow the
    // tail; the user has explicitly opted in to seeing the
    // conversation unfold.
    app.agent_auto_follow = true;
    app.agent_scroll = u16::MAX;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    app.agent_event_rx = Some(rx);

    let agent_arc = app.agents.get(&kind).cloned().unwrap();

    tokio::spawn(async move {
        let mut agent = agent_arc.lock().await;
        agent.event_tx = Some(tx.clone());

        if let Err(e) = agent.inject_message(vec![ContentBlock::Text { text: user_input }]) {
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
