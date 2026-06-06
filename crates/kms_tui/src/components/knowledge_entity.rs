use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::state::{App, KeTab, Panel};
use crate::theme::Theme;

pub fn render_knowledge_entity(f: &mut Frame, app: &App, theme: &Theme, area: ratatui::layout::Rect) {
    let border_style = theme.focused_border_style(app.focused == Panel::KnowledgeEntity);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);

    let (knowledge_style, entity_style) = match app.ke_tab {
        KeTab::Knowledge => (
            Style::default()
                .fg(theme.tab_active)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(theme.tab_inactive),
        ),
        KeTab::Entity => (
            Style::default().fg(theme.tab_inactive),
            Style::default()
                .fg(theme.tab_active)
                .add_modifier(Modifier::BOLD),
        ),
    };

    let tab_line = Paragraph::new(Line::from(vec![
        Span::styled(" Knowledge ", knowledge_style),
        Span::styled("\u{2502}", Style::default().fg(theme.text_muted)),
        Span::styled(" Entity ", entity_style),
        Span::styled(" [t]", Style::default().fg(theme.text_muted)),
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

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style);
    f.render_widget(
        Paragraph::new(content)
            .block(block)
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
}
