use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::layout::{BOTTOM_H_CONSTRAINTS, LAYOUT_CONSTRAINTS, top_h_constraints};
use crate::state::{App, KeTab, Panel, SettingsPane};

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

pub fn render_ke(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let border = focused_border(Panel::KnowledgeEntity, app.focused);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);

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
        Span::styled(" [t]", Style::default().fg(Color::DarkGray)),
    ]));
    f.render_widget(tab_line, chunks[0]);

    let content = match app.ke_tab {
        KeTab::Knowledge => app.knowledge_lines.clone(),
        KeTab::Entity => app.entity_lines.clone(),
    };
    let visible_height = chunks[1].height.saturating_sub(2);
    let content_lines = content.len() as u16;
    let max_scroll = content_lines.saturating_sub(visible_height);
    let scroll = app.ke_scroll.min(max_scroll);
    let block = Block::default().borders(Borders::ALL).border_style(border);
    f.render_widget(
        Paragraph::new(content)
            .block(block)
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
}

pub fn render_diagnostics<'a>(app: &'a App, area: ratatui::layout::Rect) -> Paragraph<'a> {
    let block = Block::default()
        .title(" Diagnostics ")
        .borders(Borders::ALL)
        .border_style(focused_border(Panel::Diagnostics, app.focused));
    let visible_height = area.height.saturating_sub(2);
    let content_lines = app.diagnostic_lines.len() as u16;
    let max_scroll = content_lines.saturating_sub(visible_height);
    let scroll = app.scroll_diag.min(max_scroll);
    Paragraph::new(app.diagnostic_lines.clone())
        .block(block)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false })
}

pub fn render_agent(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let content = app.agent_lines.clone();
    let content_lines = content.len() as u16;
    let visible_height = chunks[0].height.saturating_sub(2);
    let max_scroll = content_lines.saturating_sub(visible_height);
    let scroll = app.agent_scroll.min(max_scroll);
    app.agent_scroll = scroll;

    let conv_block = Block::default()
        .title(" Agent ")
        .borders(Borders::ALL)
        .border_style(focused_border(Panel::Agent, app.focused));
    let conv = Paragraph::new(content)
        .block(conv_block)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });
    f.render_widget(conv, chunks[0]);

    let input_label = if app.agent_running {
        let spinner_char = SPINNER_FRAMES[app.spinner_tick % SPINNER_FRAMES.len()];
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(spinner_char, Style::default().fg(Color::Yellow)),
            Span::styled(" Agent running... ", Style::default().fg(Color::Yellow)),
        ])
    } else if app.agent_input_active {
        Line::from(vec![Span::raw(format!("> {}", app.agent_input))])
    } else {
        Line::from(vec![Span::styled("  (Enter to type)", Style::default().fg(Color::DarkGray))])
    };
    let input_line = Paragraph::new(input_label).style(Style::default().bg(
        if app.agent_input_active {
            Color::DarkGray
        } else {
            Color::Black
        },
    ));
    f.render_widget(input_line, chunks[1]);

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
        .constraints(top_h_constraints(&app.top_col_widths))
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
    f.render_widget(render_diagnostics(app, bottom_row[0]), bottom_row[0]);
    f.render_widget(crate::layout::render_help_bar(), vertical[2]);

    if app.settings_modal_open {
        render_settings_modal(f, app);
    }
}

/// Render a two-pane settings modal: provider list (left) + model list (right).
fn render_settings_modal(f: &mut Frame, app: &App) {
    let area = f.area();
    let total_providers = app.providers.len().max(1);
    let current_provider_idx = app
        .providers
        .iter()
        .position(|p| p.name == app.current_provider)
        .unwrap_or(0);
    let current_models: &[String] = app
        .providers
        .get(current_provider_idx)
        .map(|p| p.models.as_slice())
        .unwrap_or(&[]);
    let total_rows = total_providers.max(current_models.len()) + 4;

    let width = 60.min(area.width.saturating_sub(4));
    let height = (total_rows as u16).min(area.height.saturating_sub(2));
    let left = (area.width.saturating_sub(width)) / 2;
    let top = (area.height.saturating_sub(height)) / 2;
    let modal_area = ratatui::layout::Rect::new(left, top, width, height);

    // Darken background
    f.render_widget(
        Paragraph::new("").style(Style::default().bg(Color::Black).fg(Color::Black)),
        area,
    );

    // Modal block
    let block = Block::default()
        .title(" ⚙ Settings ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::DarkGray));
    f.render_widget(block, modal_area);

    let half_width = width / 2;
    let provider_area = ratatui::layout::Rect::new(
        modal_area.x + 1,
        modal_area.y + 1,
        half_width.saturating_sub(1),
        height.saturating_sub(2),
    );
    let model_area = ratatui::layout::Rect::new(
        modal_area.x + half_width,
        modal_area.y + 1,
        half_width.saturating_sub(2),
        height.saturating_sub(2),
    );

    // --- Provider list (left pane) ---
    let provider_pane_label = if app.settings_pane == SettingsPane::Provider {
        " Providers *"
    } else {
        " Providers"
    };
    let provider_lines: Vec<Line> = app
        .providers
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let is_selected =
                i == app.settings_selected_provider && app.settings_pane == SettingsPane::Provider;
            let is_active = p.name == app.current_provider;
            let prefix = if is_active {
                " ● "
            } else if is_selected {
                " > "
            } else {
                "   "
            };
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            Line::from(Span::styled(format!("{}{}", prefix, p.name), style))
        })
        .collect();
    let provider_block = Block::default()
        .title(provider_pane_label)
        .borders(Borders::ALL)
        .border_style(if app.settings_pane == SettingsPane::Provider {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        });
    f.render_widget(
        Paragraph::new(provider_lines)
            .block(provider_block)
            .scroll((0, 0))
            .wrap(Wrap { trim: false }),
        provider_area,
    );

    // --- Model list (right pane) ---
    let model_pane_label = if app.settings_pane == SettingsPane::Model {
        " Models *"
    } else {
        " Models"
    };
    let model_lines: Vec<Line> = current_models
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let is_selected =
                i == app.settings_selected_model && app.settings_pane == SettingsPane::Model;
            let is_current = m == &app.current_model;
            let prefix = if is_current {
                " ● "
            } else if is_selected {
                " > "
            } else {
                "   "
            };
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            Line::from(Span::styled(format!("{}{}", prefix, m), style))
        })
        .collect();
    let model_block = Block::default()
        .title(model_pane_label)
        .borders(Borders::ALL)
        .border_style(if app.settings_pane == SettingsPane::Model {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        });
    f.render_widget(
        Paragraph::new(model_lines)
            .block(model_block)
            .scroll((0, 0))
            .wrap(Wrap { trim: false }),
        model_area,
    );

    // Footer hint
    let footer_y = modal_area.y + height - 1;
    let footer_area = ratatui::layout::Rect::new(modal_area.x + 1, footer_y, width - 2, 1);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            " [Tab] switch pane  ↑/↓ navigate  ·  [Enter] select  ·  [Esc] close",
            Style::default().fg(Color::DarkGray),
        ))),
        footer_area,
    );
}
