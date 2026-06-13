use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::state::{Action, App, ChatFocus, Panel, SettingsPane};

/// Half-page scroll step for list panels (approximation of
/// half the visible area).
const HALF_PAGE: usize = 5;

/// Full-page scroll step for list panels.
const FULL_PAGE: usize = 10;

pub fn handle_key_event(key: KeyEvent, app: &mut App) -> Action {
    // Handle `gg` pending key for vim-style jump-to-top.
    // Clear the pending state on every new key event; if it was 'g'
    // and the current key is also 'g', execute the jump.
    let pending_was_g = app.pending_key.take() == Some('g');
    if pending_was_g
        && matches!(
            key,
            KeyEvent {
                code: KeyCode::Char('g'),
                modifiers: KeyModifiers::NONE,
                ..
            }
        )
    {
        return handle_home(app);
    }

    if app.focused == Panel::Agent && app.agent_input_active() && !app.agent_running {
        return match key {
            KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                let (input, pastes) = app.take_agent_input_with_pastes();
                if input.is_empty() {
                    app.set_agent_input_active(false);
                    Action::None
                } else {
                    app.set_agent_input_active(false);
                    Action::SubmitAgent(input, pastes)
                }
            }
            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                app.clear_agent_input_text();
                app.set_agent_input_active(false);
                Action::None
            }
            KeyEvent {
                code: KeyCode::Backspace,
                ..
            } => {
                app.pop_input_char();
                Action::None
            }
            KeyEvent {
                code: KeyCode::Char(c),
                ..
            } => {
                app.push_input_char(c);
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
        } if app.focused == Panel::Agent && !app.agent_input_active() && !app.agent_running => {
            Action::SwitchAgent
        }
        KeyEvent {
            code: KeyCode::Tab, ..
        } => {
            // When the Agent panel is focused, `Tab` first tries to
            // step into the embedded sub-agent list (if there's
            // anything to focus on). This keeps the previous
            // panel-cycling behavior for everyone else, and gives
            // the user a discoverable way to reach the sub-agent
            // list without remembering a new key. When the sub-list
            // is empty or focus is already on it, fall through to
            // the normal panel cycle.
            if app.focused == Panel::Agent
                && app.chat_focus == ChatFocus::Messages
                && !app.agent_panel.agents.is_empty()
            {
                app.chat_focus = ChatFocus::AgentsPanel;
                return Action::None;
            }
            let mode = crate::layout::LayoutMode::from_width(0);
            let order = mode.panel_order();
            let idx = order.iter().position(|&p| p == app.focused).unwrap_or(0);
            let next = (idx + 1) % order.len();
            app.focused = order[next];
            // Reset sub-focus to Messages when leaving the Agent
            // panel so the next visit starts on the chat history.
            if order[next] != Panel::Agent {
                app.chat_focus = ChatFocus::Messages;
            }
            Action::None
        }
        KeyEvent {
            code: KeyCode::BackTab,
            ..
        } => {
            // Symmetric with Tab: when on the AgentsPanel sub-focus,
            // step back into the Messages sub-focus first.
            if app.focused == Panel::Agent
                && app.chat_focus == ChatFocus::AgentsPanel
            {
                app.chat_focus = ChatFocus::Messages;
                return Action::None;
            }
            let mode = crate::layout::LayoutMode::from_width(0);
            let order = mode.panel_order();
            let idx = order.iter().position(|&p| p == app.focused).unwrap_or(0);
            let prev = if idx == 0 { order.len() - 1 } else { idx - 1 };
            app.focused = order[prev];
            if order[prev] != Panel::Agent {
                app.chat_focus = ChatFocus::Messages;
            }
            Action::None
        }
        KeyEvent {
            code: KeyCode::Char('H'),
            ..
        } => {
            let mode = crate::layout::LayoutMode::from_width(0);
            let order = mode.panel_order();
            let idx = order.iter().position(|&p| p == app.focused).unwrap_or(0);
            let prev = if idx == 0 { order.len() - 1 } else { idx - 1 };
            app.focused = order[prev];
            if order[prev] != Panel::Agent {
                app.chat_focus = ChatFocus::Messages;
            }
            Action::None
        }
        KeyEvent {
            code: KeyCode::Char('L'),
            ..
        } => {
            // Mirror Tab: when on Messages sub-focus and the sub-list
            // is non-empty, step into it; otherwise cycle panels.
            if app.focused == Panel::Agent
                && app.chat_focus == ChatFocus::Messages
                && !app.agent_panel.agents.is_empty()
            {
                app.chat_focus = ChatFocus::AgentsPanel;
                return Action::None;
            }
            let mode = crate::layout::LayoutMode::from_width(0);
            let order = mode.panel_order();
            let idx = order.iter().position(|&p| p == app.focused).unwrap_or(0);
            let next = (idx + 1) % order.len();
            app.focused = order[next];
            if order[next] != Panel::Agent {
                app.chat_focus = ChatFocus::Messages;
            }
            Action::None
        }
        // --- Sub-agent list keybindings (when sub-focus is AgentsPanel) ---
        KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            ..
        } if app.focused == Panel::Agent
            && app.chat_focus == ChatFocus::AgentsPanel
            && !app.agent_panel.agents.is_empty() =>
        {
            app.agent_panel.toggle_selected();
            Action::None
        }
        KeyEvent {
            code: KeyCode::Char('j') | KeyCode::Down,
            ..
        } if app.focused == Panel::Agent
            && app.chat_focus == ChatFocus::AgentsPanel
            && !app.agent_panel.agents.is_empty() =>
        {
            app.agent_panel.move_selection(1);
            Action::None
        }
        KeyEvent {
            code: KeyCode::Char('k') | KeyCode::Up,
            ..
        } if app.focused == Panel::Agent
            && app.chat_focus == ChatFocus::AgentsPanel
            && !app.agent_panel.agents.is_empty() =>
        {
            app.agent_panel.move_selection(-1);
            Action::None
        }
        KeyEvent {
            code: KeyCode::Char('e'),
            modifiers: KeyModifiers::NONE,
            ..
        } if app.focused == Panel::Agent
            && app.chat_focus == ChatFocus::AgentsPanel
            && !app.agent_panel.agents.is_empty() =>
        {
            app.agent_panel.expand_all();
            Action::None
        }
        KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::NONE,
            ..
        } if app.focused == Panel::Agent
            && app.chat_focus == ChatFocus::AgentsPanel
            && !app.agent_panel.agents.is_empty() =>
        {
            app.agent_panel.collapse_all();
            Action::None
        }
        // --- Agent panel: enter input mode ---
        KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            ..
        } if app.focused == Panel::Agent && !app.agent_running => {
            app.set_agent_input_active(true);
            Action::None
        }
        // --- KE panel: toggle knowledge/entity tab ---
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
        // ---------------------------------------------------------------
        // Tree panel: vim-style keybindings
        // ---------------------------------------------------------------
        // `g` (lowercase) on Tree — first press of `gg` (jump to top).
        KeyEvent {
            code: KeyCode::Char('g'),
            modifiers: KeyModifiers::NONE,
            ..
        } if app.focused == Panel::Tree => {
            app.pending_key = Some('g');
            Action::None
        }
        // Ctrl+d — half-page down.
        KeyEvent {
            code: KeyCode::Char('d'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => handle_half_page_down(app),
        // Ctrl+u — half-page up.
        KeyEvent {
            code: KeyCode::Char('u'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => handle_half_page_up(app),
        // Ctrl+f — full page down.
        KeyEvent {
            code: KeyCode::Char('f'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => handle_page_down(app),
        // Ctrl+b — full page up.
        KeyEvent {
            code: KeyCode::Char('b'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => handle_page_up(app),
        // --- Generic scroll bindings (for panels that aren't handled above) ---
        KeyEvent {
            code: KeyCode::Char('j') | KeyCode::Down,
            ..
        } => handle_scroll_down(app),
        KeyEvent {
            code: KeyCode::Char('k') | KeyCode::Up,
            ..
        } => handle_scroll_up(app),
        KeyEvent {
            code: KeyCode::PageDown,
            ..
        } => handle_page_down(app),
        KeyEvent {
            code: KeyCode::PageUp,
            ..
        } => handle_page_up(app),
        KeyEvent {
            code: KeyCode::Home,
            ..
        } => handle_home(app),
        KeyEvent {
            code: KeyCode::End, ..
        } => handle_end(app),
        // Shift+G scrolls to the bottom of the focused panel (vim-style).
        KeyEvent {
            code: KeyCode::Char('G'),
            modifiers: KeyModifiers::SHIFT,
            ..
        } => handle_end(app),
        _ => Action::None,
    }
}

// ---- Tree helpers ----

/// Move the tree selection by `delta` items (clamped to bounds).
fn tree_move(app: &mut App, delta: isize) -> Action {
    if let Some(sel) = app.tree_state.selected() {
        let new = if delta >= 0 {
            sel.saturating_add(delta as usize)
                .min(app.tree_items.len().saturating_sub(1))
        } else {
            sel.saturating_sub(delta.unsigned_abs())
        };
        app.tree_state.select(Some(new));
        Action::TreeChanged
    } else {
        Action::None
    }
}

// ---- Page / half-page scroll (works on all list panels) ----

fn handle_page_down(app: &mut App) -> Action {
    match app.focused {
        Panel::Tree => tree_move(app, FULL_PAGE as isize),
        Panel::Agent => {
            app.chat_panel.disable_auto_scroll();
            app.chat_panel
                .set_scroll(app.chat_panel.scroll().saturating_add(FULL_PAGE as u16));
            Action::None
        }
        Panel::KnowledgeEntity => {
            let lines = match app.ke_tab {
                crate::state::KeTab::Knowledge => &app.knowledge_lines,
                crate::state::KeTab::Entity => &app.entity_lines,
            };
            app.ke_scroll =
                (app.ke_scroll as usize + FULL_PAGE).min(lines.len().saturating_sub(1)) as u16;
            Action::None
        }
        Panel::Diagnostics => {
            app.scroll_diag = (app.scroll_diag as usize + FULL_PAGE)
                .min(app.diagnostic_lines.len().saturating_sub(1))
                as u16;
            Action::None
        }
    }
}

fn handle_page_up(app: &mut App) -> Action {
    match app.focused {
        Panel::Tree => tree_move(app, -(FULL_PAGE as isize)),
        Panel::Agent => {
            app.chat_panel.disable_auto_scroll();
            app.chat_panel
                .set_scroll(app.chat_panel.scroll().saturating_sub(FULL_PAGE as u16));
            Action::None
        }
        Panel::KnowledgeEntity => {
            app.ke_scroll = app.ke_scroll.saturating_sub(FULL_PAGE as u16);
            Action::None
        }
        Panel::Diagnostics => {
            app.scroll_diag = app.scroll_diag.saturating_sub(FULL_PAGE as u16);
            Action::None
        }
    }
}

fn handle_half_page_down(app: &mut App) -> Action {
    match app.focused {
        Panel::Tree => tree_move(app, HALF_PAGE as isize),
        Panel::Agent => {
            app.chat_panel.disable_auto_scroll();
            app.chat_panel
                .set_scroll(app.chat_panel.scroll().saturating_add(HALF_PAGE as u16));
            Action::None
        }
        Panel::KnowledgeEntity => {
            let lines = match app.ke_tab {
                crate::state::KeTab::Knowledge => &app.knowledge_lines,
                crate::state::KeTab::Entity => &app.entity_lines,
            };
            app.ke_scroll = (app.ke_scroll as usize + HALF_PAGE)
                .min(lines.len().saturating_sub(1)) as u16;
            Action::None
        }
        Panel::Diagnostics => {
            app.scroll_diag = (app.scroll_diag as usize + HALF_PAGE)
                .min(app.diagnostic_lines.len().saturating_sub(1)) as u16;
            Action::None
        }
    }
}

fn handle_half_page_up(app: &mut App) -> Action {
    match app.focused {
        Panel::Tree => tree_move(app, -(HALF_PAGE as isize)),
        Panel::Agent => {
            app.chat_panel.disable_auto_scroll();
            app.chat_panel
                .set_scroll(app.chat_panel.scroll().saturating_sub(HALF_PAGE as u16));
            Action::None
        }
        Panel::KnowledgeEntity => {
            app.ke_scroll = app.ke_scroll.saturating_sub(HALF_PAGE as u16);
            Action::None
        }
        Panel::Diagnostics => {
            app.scroll_diag = app.scroll_diag.saturating_sub(HALF_PAGE as u16);
            Action::None
        }
    }
}

// ---- Line-by-line scroll (works on all panels) ----

fn handle_scroll_down(app: &mut App) -> Action {
    match app.focused {
        Panel::Tree => tree_move(app, 1),
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
        Panel::Agent => handle_agent_scroll_down(app),
    }
}

fn handle_agent_scroll_down(app: &mut App) -> Action {
    app.chat_panel.disable_auto_scroll();
    app.chat_panel
        .set_scroll(app.chat_panel.scroll().saturating_add(1));
    Action::None
}

fn handle_scroll_up(app: &mut App) -> Action {
    match app.focused {
        Panel::Tree => tree_move(app, -1),
        Panel::KnowledgeEntity => {
            app.ke_scroll = app.ke_scroll.saturating_sub(1);
            Action::None
        }
        Panel::Diagnostics => {
            app.scroll_diag = app.scroll_diag.saturating_sub(1);
            Action::None
        }
        Panel::Agent => {
            app.chat_panel.disable_auto_scroll();
            app.chat_panel
                .set_scroll(app.chat_panel.scroll().saturating_sub(1));
            Action::None
        }
    }
}

// ---- Jump to top / bottom ----

fn handle_home(app: &mut App) -> Action {
    match app.focused {
        Panel::Tree => {
            app.tree_state.select(Some(0));
            Action::TreeChanged
        }
        Panel::KnowledgeEntity => {
            app.ke_scroll = 0;
            Action::None
        }
        Panel::Diagnostics => {
            app.scroll_diag = 0;
            Action::None
        }
        Panel::Agent => {
            app.chat_panel.disable_auto_scroll();
            app.chat_panel.set_scroll(0);
            Action::None
        }
    }
}

fn handle_end(app: &mut App) -> Action {
    match app.focused {
        Panel::Tree => {
            let last = app.tree_items.len().saturating_sub(1);
            app.tree_state.select(Some(last));
            Action::TreeChanged
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
        Panel::Agent => {
            app.chat_panel.enable_auto_scroll();
            Action::None
        }
    }
}
