use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::state::{Action, App, ChatFocus, Panel, SettingsPane};

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
            let mode = crate::layout::LayoutMode::from_width(0);
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
            let mode = crate::layout::LayoutMode::from_width(0);
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
            let mode = crate::layout::LayoutMode::from_width(0);
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
            let mode = crate::layout::LayoutMode::from_width(0);
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
            code: KeyCode::PageDown,
            ..
        } => handle_page_down(app),
        KeyEvent {
            code: KeyCode::PageUp,
            ..
        } => handle_page_up(app),
        KeyEvent {
            code: KeyCode::End, ..
        } => handle_end(app),
        _ => Action::None,
    }
}

/// PageDown on the Agent panel scrolls the chat down by the visible
/// height. This is the natural "scroll one screen" binding in TUI
/// apps; we apply it to the Agent panel only so it doesn't interfere
/// with the tree panel (which uses `j`/`k` to move the selection).
fn handle_page_down(app: &mut App) -> Action {
    if app.focused == Panel::Agent && app.chat_focus == ChatFocus::Messages {
        app.agent_auto_scroll = false;
        // Scroll a typical "page" worth — 10 lines. We don't know the
        // exact visible height at this point in the event flow; 10 is
        // a reasonable guess and matches the bounds of typical TUI
        // chat windows. The renderer clamps to the actual content
        // length on the next frame.
        app.agent_scroll = app.agent_scroll.saturating_add(10);
    }
    Action::None
}

fn handle_page_up(app: &mut App) -> Action {
    if app.focused == Panel::Agent && app.chat_focus == ChatFocus::Messages {
        app.agent_auto_scroll = false;
        app.agent_scroll = app.agent_scroll.saturating_sub(10);
    }
    Action::None
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
        Panel::Agent => handle_agent_scroll_down(app),
    }
}

/// Scroll the chat panel down by one line. Used by `j` / `Down` when
/// the user is on the Messages sub-focus of the Agent panel.
/// Any manual scroll disables auto-follow so the next streamed event
/// doesn't snap the view back to the bottom.
fn handle_agent_scroll_down(app: &mut App) -> Action {
    if app.chat_focus != ChatFocus::Messages {
        // On the ParallelPanel sub-focus, `j` / `k` move the sub-agent
        // selection (handled in the Tab / j/k arms above). When the
        // user is in input mode or the agent is running, no-op.
        return Action::None;
    }
    app.agent_auto_scroll = false;
    app.agent_scroll = app.agent_scroll.saturating_add(1);
    Action::None
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
        Panel::Agent => {
            if app.chat_focus != ChatFocus::Messages {
                return Action::None;
            }
            app.agent_auto_scroll = false;
            app.agent_scroll = app.agent_scroll.saturating_sub(1);
            Action::None
        }
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
        Panel::Agent => {
            // End on the Agent panel re-engages auto-follow. The
            // renderer will pin the scroll to the bottom on the next
            // frame, so any in-flight stream events become visible
            // immediately. The exact y offset is computed in the
            // render path from the current line count + visible
            // area height.
            app.agent_auto_scroll = true;
            Action::None
        }
        Panel::Tree => Action::None,
    }
}
