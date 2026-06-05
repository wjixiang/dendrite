use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::layout::{BOTTOM_H_CONSTRAINTS, LAYOUT_CONSTRAINTS, TOP_H_CONSTRAINTS};
use crate::state::{App, KeTab, Panel};

/// Unicode braille spinner frames for animation.
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

fn focused_border(panel: Panel, focused: Panel) -> Style {
    let color = if panel == focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    Style::default().fg(color)
}

pub fn render_tree(items: &[ListItem<'static>], focused: Panel) -> List<'static> {
    let block = Block::default()
        .title(" Tree ")
        .borders(Borders::ALL)
        .border_style(focused_border(Panel::Tree, focused));
    List::new(items.to_vec())
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .scroll_padding(2)
}

/// Render the merged Knowledge/Entity panel with a fixed tab bar.
pub fn render_ke(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let is_focused = app.focused == Panel::KnowledgeEntity;
    let border = focused_border(Panel::KnowledgeEntity, app.focused);

    // Split into: tab bar (1 line) + scrollable content
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);

    // --- Fixed tab bar ---
    let (knowledge_style, entity_style) = match app.ke_tab {
        KeTab::Knowledge => (
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(Color::DarkGray),
        ),
        KeTab::Entity => (
            Style::default().fg(Color::DarkGray),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    };
    let tab_line = Paragraph::new(Line::from(vec![
        Span::styled(" Knowledge ", knowledge_style),
        Span::styled("│", Style::default().fg(Color::DarkGray)),
        Span::styled(" Entity ", entity_style),
        Span::styled(
            " [t]",
            Style::default().fg(if is_focused {
                Color::DarkGray
            } else {
                Color::Black
            }),
        ),
    ]));
    f.render_widget(tab_line, chunks[0]);

    // --- Scrollable content with border ---
    let content = match app.ke_tab {
        KeTab::Knowledge => app.knowledge_lines.clone(),
        KeTab::Entity => app.entity_lines.clone(),
    };
    let block = Block::default().borders(Borders::ALL).border_style(border);
    f.render_widget(
        Paragraph::new(content)
            .block(block)
            .scroll((app.ke_scroll, 0))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
}

pub fn render_diagnostics<'a>(app: &'a App) -> Paragraph<'a> {
    let block = Block::default()
        .title(" Diagnostics ")
        .borders(Borders::ALL)
        .border_style(focused_border(Panel::Diagnostics, app.focused));
    Paragraph::new(app.diagnostic_lines.clone())
        .block(block)
        .scroll((app.scroll_diag, 0))
        .wrap(Wrap { trim: false })
}

/// Render the Agent panel: conversation area (scrollable) + input bar (1 line).
pub fn render_agent(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    // Split agent area into: conversation (top) + input bar (bottom 1 line)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    // --- Conversation area ---
    let visible_height = chunks[0].height.saturating_sub(2) as u16; // minus border

    // Build content lines, appending spinner if requesting
    let mut content = app.agent_lines.clone();
    if app.agent_requesting {
        let spinner_char = SPINNER_FRAMES[app.spinner_tick % SPINNER_FRAMES.len()];
        content.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(spinner_char, Style::default().fg(Color::Yellow)),
            Span::styled(" thinking...", Style::default().fg(Color::Yellow)),
        ]));
    }
    let content_lines = content.len() as u16;
    let max_scroll = content_lines.saturating_sub(visible_height) + 2;
    let scroll = app.agent_scroll.min(max_scroll);

    let conv_block = Block::default()
        .title(" Agent ")
        .borders(Borders::ALL)
        .border_style(focused_border(Panel::Agent, app.focused));
    let conv = Paragraph::new(content)
        .block(conv_block)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });
    f.render_widget(conv, chunks[0]);

    // --- Input bar ---
    let input_label = if app.agent_running {
        Span::styled("  Agent running... ", Style::default().fg(Color::Yellow))
    } else if app.agent_input_active {
        Span::raw(format!("> {}", app.agent_input))
    } else {
        Span::styled("  (Enter to type)", Style::default().fg(Color::DarkGray))
    };
    let input_line = Paragraph::new(Line::from(input_label)).style(Style::default().bg(
        if app.agent_input_active {
            Color::DarkGray
        } else {
            Color::Black
        },
    ));
    f.render_widget(input_line, chunks[1]);

    // Set cursor position when input is active
    if app.agent_input_active && app.focused == Panel::Agent {
        let cursor_x = chunks[1].x + 2 + app.agent_input.len() as u16;
        let cursor_y = chunks[1].y;
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

pub fn ui(f: &mut Frame, app: &mut App) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints(LAYOUT_CONSTRAINTS)
        .split(f.area());

    let top_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(TOP_H_CONSTRAINTS)
        .split(vertical[0]);

    let bottom_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(BOTTOM_H_CONSTRAINTS)
        .split(vertical[1]);

    f.render_stateful_widget(
        render_tree(&app.tree_items, app.focused),
        top_row[0],
        &mut app.tree_state,
    );
    render_ke(f, app, top_row[1]);
    render_agent(f, app, top_row[2]);
    f.render_widget(render_diagnostics(app), bottom_row[0]);
    f.render_widget(crate::layout::render_help_bar(), vertical[2]);
}
