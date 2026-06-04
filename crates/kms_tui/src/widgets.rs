use ratatui::{
    layout::{Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::layout::{BOTTOM_H_CONSTRAINTS, LAYOUT_CONSTRAINTS, TOP_H_CONSTRAINTS};
use crate::state::{App, Panel};

fn focused_border(panel: Panel, focused: Panel) -> Style {
    let color = if panel == focused { Color::Cyan } else { Color::DarkGray };
    Style::default().fg(color)
}

pub fn render_tree(items: &[ListItem<'static>], focused: Panel) -> List<'static> {
    let block = Block::default()
        .title(" Tree ")
        .borders(Borders::ALL)
        .border_style(focused_border(Panel::Tree, focused));
    List::new(items.to_vec())
        .block(block)
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .scroll_padding(2)
}

pub fn render_knowledge<'a>(app: &'a App) -> Paragraph<'a> {
    let block = Block::default()
        .title(" Knowledge ")
        .borders(Borders::ALL)
        .border_style(focused_border(Panel::Knowledge, app.focused));
    Paragraph::new(app.knowledge_lines.clone())
        .block(block)
        .wrap(Wrap { trim: false })
}

pub fn render_entity<'a>(app: &'a App) -> Paragraph<'a> {
    let block = Block::default()
        .title(" Entity ")
        .borders(Borders::ALL)
        .border_style(focused_border(Panel::Entity, app.focused));
    Paragraph::new(app.entity_lines.clone())
        .block(block)
        .wrap(Wrap { trim: false })
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

pub fn render_agent<'a>(app: &'a App) -> Paragraph<'a> {
    let block = Block::default()
        .title(" Agent ")
        .borders(Borders::ALL)
        .border_style(focused_border(Panel::Agent, app.focused));
    Paragraph::new(app.agent_lines.clone())
        .block(block)
        .scroll((app.agent_scroll, 0))
        .wrap(Wrap { trim: false })
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
    f.render_widget(render_knowledge(app), top_row[1]);
    f.render_widget(render_entity(app), top_row[2]);
    f.render_widget(render_diagnostics(app), bottom_row[0]);
    f.render_widget(render_agent(app), bottom_row[1]);
    f.render_widget(crate::layout::render_help_bar(), vertical[2]);
}
