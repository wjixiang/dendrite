use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use crate::state::{App, Panel};
use crate::widgets::ui;
use crate::CrosstermBackend;
use ratatui::Terminal;

const PANEL_ORDER: [Panel; 5] = [
    Panel::Tree,
    Panel::Knowledge,
    Panel::Entity,
    Panel::Diagnostics,
    Panel::Agent,
];

pub fn handle_key_event(key: KeyEvent, app: &mut App) -> bool {
    let mut tree_changed = false;
    match key {
        KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            ..
        } => app.should_quit = true,
        KeyEvent {
            code: KeyCode::Tab,
            ..
        } => {
            let idx = PANEL_ORDER.iter().position(|&p| p == app.focused).unwrap_or(0);
            let next = (idx + 1) % PANEL_ORDER.len();
            app.focused = PANEL_ORDER[next];
        }
        KeyEvent {
            code: KeyCode::BackTab,
            ..
        } => {
            let idx = PANEL_ORDER.iter().position(|&p| p == app.focused).unwrap_or(0);
            let prev = if idx == 0 { PANEL_ORDER.len() - 1 } else { idx - 1 };
            app.focused = PANEL_ORDER[prev];
        }
        KeyEvent {
            code: KeyCode::Char('j') | KeyCode::Down,
            ..
        } => match app.focused {
            Panel::Tree => {
                if let Some(sel) = app.tree_state.selected() {
                    let next = sel.saturating_add(1).min(app.tree_items.len().saturating_sub(1));
                    app.tree_state.select(Some(next));
                    tree_changed = true;
                }
            }
            Panel::Diagnostics => {
                if app.scroll_diag < app.diagnostic_lines.len() as u16 {
                    app.scroll_diag += 1;
                }
            }
            Panel::Agent => {
                if app.agent_scroll < app.agent_lines.len() as u16 {
                    app.agent_scroll += 1;
                }
            }
            _ => {}
        },
        KeyEvent {
            code: KeyCode::Char('k') | KeyCode::Up,
            ..
        } => match app.focused {
            Panel::Tree => {
                if let Some(sel) = app.tree_state.selected() {
                    app.tree_state.select(Some(sel.saturating_sub(1)));
                    tree_changed = true;
                }
            }
            Panel::Diagnostics => {
                app.scroll_diag = app.scroll_diag.saturating_sub(1);
            }
            Panel::Agent => {
                app.agent_scroll = app.agent_scroll.saturating_sub(1);
            }
            _ => {}
        },
        _ => {}
    }
    tree_changed
}

/// Main event loop: draw UI, read events, update state.
pub async fn run_app(
    terminal: &mut Terminal<CrosstermBackend>,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if let Event::Key(key) = crossterm::event::read()? {
            let tree_changed = handle_key_event(key, app);
            if tree_changed {
                app.on_tree_select().await;
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
