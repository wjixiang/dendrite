use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::{Line, Span};
use ratatui::style::{Color, Modifier, Style};
use types::messages::ContentBlock;
use types::AgentUiEvent;
use serde_json::Value;
use std::sync::Arc;

use crate::state::{Action, App, Panel, SettingsPane};
use crate::settings::save_settings;
use crate::widgets::ui;
use crate::CrosstermBackend;
use ratatui::Terminal;
use llm_api::provider::LlmProvider;

const PANEL_ORDER: [Panel; 4] = [
    Panel::Tree,
    Panel::KnowledgeEntity,
    Panel::Agent,
    Panel::Diagnostics,
];

const MAX_VAL_LEN: usize = usize::MAX;

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let end = s.char_indices()
        .take_while(|(i, _)| *i < max)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(max);
    format!("{}…", &s[..end])
}

fn format_value(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => truncate(s, MAX_VAL_LEN),
        Value::Array(arr) => {
            if arr.is_empty() {
                "[]".to_string()
            } else if arr.len() == 1 {
                format!("[{}]", format_value(&arr[0]))
            } else {
                format!("[{} items]", arr.len())
            }
        }
        Value::Object(_) => "{…}".to_string(),
    }
}

fn event_to_lines(event: AgentUiEvent) -> Vec<Line<'static>> {
    match event {
        AgentUiEvent::LlmResponse(text) => {
            text.lines()
                .map(|l| Line::from(Span::styled(l.to_string(), Style::default().fg(Color::White))))
                .collect()
        }
        AgentUiEvent::Thinking(text) => {
            let mut lines = vec![Line::from(Span::styled(
                "💭 Thinking:",
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ))];
            for l in text.lines().take(10) {
                lines.push(Line::from(Span::styled(
                    format!("   {}", truncate(l, usize::MAX)),
                    Style::default().fg(Color::Magenta),
                )));
            }
            if text.lines().count() > 10 {
                lines.push(Line::from(Span::styled(
                    format!("   … {} more lines", text.lines().count() - 10),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            lines
        }
        AgentUiEvent::ToolCall { name, input } => {
            let mut lines = vec![Line::from(vec![
                Span::styled("🔧 ", Style::default().fg(Color::Cyan)),
                Span::styled(name.clone(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ])];
            if let Some(obj) = input.as_object() {
                for (key, val) in obj {
                    lines.push(Line::from(vec![
                        Span::styled("    ", Style::default()),
                        Span::styled(format!("{}: ", key), Style::default().fg(Color::Gray)),
                        Span::styled(format_value(val), Style::default().fg(Color::White)),
                    ]));
                }
            } else if !input.is_null() {
                lines.push(Line::from(vec![
                    Span::styled("    ", Style::default()),
                    Span::styled(truncate(&input.to_string(), MAX_VAL_LEN), Style::default().fg(Color::White)),
                ]));
            }
            lines
        }
        AgentUiEvent::ToolResult { ok, content } => {
            let (icon, color) = if ok { ("✓", Color::Green) } else { ("✗", Color::Red) };
            let mut lines = vec![Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(icon, Style::default().fg(color)),
            ])];
            if let Ok(val) = serde_json::from_str::<Value>(&content) {
                match &val {
                    Value::Object(map) => {
                        for (k, v) in map {
                            lines.push(Line::from(vec![
                                Span::styled("    ", Style::default()),
                                Span::styled(format!("{}: ", k), Style::default().fg(Color::DarkGray)),
                                Span::styled(format_value(v), Style::default().fg(color)),
                            ]));
                        }
                    }
                    Value::Array(arr) => {
                        for item in arr.iter().take(8) {
                            let label = if let Some(s) = item.as_str() {
                                truncate(s, MAX_VAL_LEN)
                            } else {
                                format_value(item)
                            };
                            lines.push(Line::from(vec![
                                Span::styled("    ", Style::default()),
                                Span::styled(format!("  • {}", label), Style::default().fg(color)),
                            ]));
                        }
                        if arr.len() > 8 {
                            lines.push(Line::from(Span::styled(
                                format!("    … and {} more", arr.len() - 8),
                                Style::default().fg(Color::DarkGray),
                            )));
                        }
                    }
                    Value::String(s) => {
                        for l in s.lines().take(6) {
                            lines.push(Line::from(vec![
                                Span::styled("    ", Style::default()),
                                Span::styled(truncate(l, usize::MAX), Style::default().fg(color)),
                            ]));
                        }
                    }
                    other => {
                        lines.push(Line::from(vec![
                            Span::styled("    ", Style::default()),
                            Span::styled(truncate(&other.to_string(), MAX_VAL_LEN), Style::default().fg(color)),
                        ]));
                    }
                }
            } else {
                for l in content.lines().take(6) {
                    lines.push(Line::from(vec![
                        Span::styled("    ", Style::default()),
                        Span::styled(truncate(l, usize::MAX), Style::default().fg(color)),
                    ]));
                }
            }
            lines
        }
        AgentUiEvent::Done => {
            vec![Line::from(Span::styled(
                "✅ Agent completed",
                Style::default().fg(Color::Green),
            ))]
        }
        AgentUiEvent::Error(msg) => {
            vec![Line::from(Span::styled(
                format!("❌ {}", msg),
                Style::default().fg(Color::Red),
            ))]
        }
        AgentUiEvent::Requesting => vec![],
    }
}

pub fn handle_key_event(key: KeyEvent, app: &mut App) -> Action {
    // --- Agent input mode ---
    if app.focused == Panel::Agent && app.agent_input_active && !app.agent_running {
        return match key {
            KeyEvent { code: KeyCode::Enter, modifiers: KeyModifiers::NONE, .. } => {
                let input = std::mem::take(&mut app.agent_input);
                if input.is_empty() {
                    app.agent_input_active = false;
                    Action::None
                } else {
                    app.agent_input_active = false;
                    Action::SubmitAgent(input)
                }
            }
            KeyEvent { code: KeyCode::Esc, .. } => {
                app.agent_input.clear();
                app.agent_input_active = false;
                Action::None
            }
            KeyEvent { code: KeyCode::Backspace, .. } => {
                app.agent_input.pop();
                Action::None
            }
            KeyEvent { code: KeyCode::Char(c), .. } => {
                app.agent_input.push(c);
                Action::None
            }
            _ => Action::None,
        };
    }

    // --- Settings modal ---
    if app.settings_modal_open {
        return match key {
            KeyEvent { code: KeyCode::Esc, .. } => {
                app.settings_modal_open = false;
                Action::None
            }
            KeyEvent { code: KeyCode::Tab, .. } => {
                app.settings_pane = match app.settings_pane {
                    SettingsPane::Provider => SettingsPane::Model,
                    SettingsPane::Model => SettingsPane::Provider,
                };
                Action::None
            }
            KeyEvent { code: KeyCode::Up | KeyCode::Char('k'), .. } => {
                Action::SettingsNav(app.settings_pane, -1)
            }
            KeyEvent { code: KeyCode::Down | KeyCode::Char('j'), .. } => {
                Action::SettingsNav(app.settings_pane, 1)
            }
            KeyEvent { code: KeyCode::Enter, modifiers: KeyModifiers::NONE, .. } => {
                Action::SettingsConfirm
            }
            _ => Action::None,
        };
    }

    // --- Normal mode ---
    match key {
        KeyEvent { code: KeyCode::Char('q'), modifiers: KeyModifiers::NONE, .. } => Action::Quit,
        KeyEvent { code: KeyCode::Char('s'), modifiers: KeyModifiers::NONE, .. } => Action::OpenSettings,
        KeyEvent { code: KeyCode::Tab, .. } => {
            let idx = PANEL_ORDER.iter().position(|&p| p == app.focused).unwrap_or(0);
            let next = (idx + 1) % PANEL_ORDER.len();
            app.focused = PANEL_ORDER[next];
            Action::None
        }
        KeyEvent { code: KeyCode::BackTab, .. } => {
            let idx = PANEL_ORDER.iter().position(|&p| p == app.focused).unwrap_or(0);
            let prev = if idx == 0 { PANEL_ORDER.len() - 1 } else { idx - 1 };
            app.focused = PANEL_ORDER[prev];
            Action::None
        }
        KeyEvent { code: KeyCode::Char('H'), .. } => {
            let idx = PANEL_ORDER.iter().position(|&p| p == app.focused).unwrap_or(0);
            let prev = if idx == 0 { PANEL_ORDER.len() - 1 } else { idx - 1 };
            app.focused = PANEL_ORDER[prev];
            Action::None
        }
        KeyEvent { code: KeyCode::Char('L'), .. } => {
            let idx = PANEL_ORDER.iter().position(|&p| p == app.focused).unwrap_or(0);
            let next = (idx + 1) % PANEL_ORDER.len();
            app.focused = PANEL_ORDER[next];
            Action::None
        }
        KeyEvent { code: KeyCode::Enter, modifiers: KeyModifiers::NONE, .. }
            if app.focused == Panel::Agent && !app.agent_running => {
            app.agent_input_active = true;
            Action::None
        }
        KeyEvent { code: KeyCode::Char('t'), modifiers: KeyModifiers::NONE, .. }
            if app.focused == Panel::KnowledgeEntity => {
            app.ke_tab = match app.ke_tab {
                crate::state::KeTab::Knowledge => crate::state::KeTab::Entity,
                crate::state::KeTab::Entity => crate::state::KeTab::Knowledge,
            };
            app.ke_scroll = 0;
            Action::None
        }
        KeyEvent { code: KeyCode::Char('j') | KeyCode::Down, .. } => {
            match app.focused {
                Panel::Tree => {
                    if let Some(sel) = app.tree_state.selected() {
                        let next = sel.saturating_add(1).min(app.tree_items.len().saturating_sub(1));
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
                    app.agent_following = false;
                    if app.agent_scroll < app.agent_lines.len() {
                        app.agent_scroll += 1;
                    }
                    Action::None
                }
            }
        }
        KeyEvent { code: KeyCode::PageDown, .. } => {
            match app.focused {
                Panel::Agent => {
                    app.agent_following = false;
                    let page = app.agent_visible_height.saturating_sub(1).max(1);
                    app.agent_scroll = app.agent_scroll.saturating_add(page);
                    Action::None
                }
                _ => Action::None,
            }
        }
        KeyEvent { code: KeyCode::Char('k') | KeyCode::Up, .. } => {
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
                    app.agent_following = false;
                    app.agent_scroll = app.agent_scroll.saturating_sub(1);
                    Action::None
                }
            }
        }
        KeyEvent { code: KeyCode::PageUp, .. } => {
            match app.focused {
                Panel::Agent => {
                    app.agent_following = false;
                    let page = app.agent_visible_height.saturating_sub(1).max(1);
                    app.agent_scroll = app.agent_scroll.saturating_sub(page);
                    Action::None
                }
                _ => Action::None,
            }
        }
        KeyEvent { code: KeyCode::End, .. } => {
            match app.focused {
                Panel::Agent => {
                    app.agent_following = true;
                    let visible = app.agent_visible_height.max(1);
                    app.agent_scroll = app.agent_lines.len().saturating_sub(visible);
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
        KeyEvent { code: KeyCode::Char('G'), .. }
            if app.focused == Panel::Agent && !app.agent_input_active => {
            app.agent_following = true;
            let visible = app.agent_visible_height.max(1);
            app.agent_scroll = app.agent_lines.len().saturating_sub(visible);
            Action::None
        }
        _ => Action::None,
    }
}

/// Main event loop: draw UI, poll events, drain agent messages.
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
                        app.agent_lines.extend(event_to_lines(event));
                        if app.agent_following && app.agent_lines.len() > 1 {
                            let visible = app.agent_visible_height.max(1);
                            app.agent_scroll = app
                                .agent_lines
                                .len()
                                .saturating_sub(visible);
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
            // Drain all pending events to avoid one-event-per-redraw bottleneck
            // (e.g. terminals that send paste as individual Char events)
            loop {
                let event = crossterm::event::read()?;
                match event {
                    Event::Key(key) => {
                        match handle_key_event(key, app) {
                            Action::Quit => app.should_quit = true,
                            Action::TreeChanged => app.on_tree_select().await,
                            Action::SubmitAgent(input) => spawn_agent_task(app, input),
                            Action::OpenSettings => {
                                app.settings_modal_open = true;
                            }
                            Action::SettingsNav(pane, delta) => {
                                match pane {
                                    SettingsPane::Provider => {
                                        let max = app.providers.len().saturating_sub(1);
                                        app.settings_selected_provider = (app.settings_selected_provider as isize + delta)
                                            .clamp(0, max as isize) as usize;
                                    }
                                    SettingsPane::Model => {
                                        let provider_idx = app.settings_selected_provider;
                                        if let Some(provider) = app.providers.get(provider_idx) {
                                            let max = provider.models.len().saturating_sub(1);
                                            app.settings_selected_model = (app.settings_selected_model as isize + delta)
                                                .clamp(0, max as isize) as usize;
                                        }
                                    }
                                }
                            }
                            Action::SettingsSwitchPane(pane) => {
                                app.settings_pane = pane;
                            }
                            Action::SettingsConfirm => {
                                let provider_idx = app.settings_selected_provider;
                                let model_idx = app.settings_selected_model;

                                let new_provider = app.providers.get(provider_idx).map(|p| p.name.clone());
                                let new_model = app.providers.get(provider_idx)
                                    .and_then(|p| p.models.get(model_idx).cloned());

                                if let (Some(new_provider), Some(new_model)) = (new_provider, new_model) {
                                    if new_provider != app.current_provider || new_model != app.current_model {
                                        if let Some(pool) = build_pool(&new_provider, &new_model) {
                                            let new_agent = agent::Agent::builder()
                                                .with_model_pool(Arc::new(pool))
                                                .with_kms(Arc::new(app.svc.clone()))
                                                .build()
                                                .await
                                                .map_err(|e| e.to_string())?;

                                            let old_agent = std::mem::replace(
                                                &mut *app.agent.blocking_lock(),
                                                new_agent,
                                            );
                                            drop(old_agent);

                                            app.current_provider = new_provider.clone();
                                            app.current_model = new_model.clone();
                                            save_settings(&new_provider, &new_model);
                                        }
                                    }
                                }
                                app.settings_modal_open = false;
                            }
                            Action::None => {}
                        }
                    }
                    Event::Paste(s) => {
                        if app.focused == Panel::Agent && app.agent_input_active && !app.agent_running {
                            app.agent_input.push_str(&s);
                        }
                    }
                    _ => {}
                }
                // Stop draining if no more events are queued
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

fn build_pool(provider: &str, model: &str) -> Option<llm_api::model::model_pool::ModelPool> {
    use llm_api::model::model_pool::ModelPool;
    use llm_api::provider::mimo::MimoProvider;

    match provider {
        "mimo" => {
            let mimo_provider = MimoProvider::new(None, None, None);
            let m = mimo_provider.get_model(model).ok()?;
            let mut pool = ModelPool::new();
            pool.add_model(m);
            Some(pool)
        }
        "minimax" => {
            let minimax_provider = llm_api::provider::minimax::MinimaxProvider::new(None, None, None);
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

    app.agent_lines.push(Line::from(Span::styled(
        format!("> {}", user_input),
        Style::default().fg(Color::Yellow),
    )));
    app.agent_lines.push(Line::from(""));
    app.agent_running = true;
    app.agent_following = true;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    app.agent_event_rx = Some(rx);

    let agent_arc = app.agent.clone();

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
