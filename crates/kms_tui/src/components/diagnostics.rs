use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::state::{App, Panel};
use crate::theme::Theme;

pub fn render_diagnostics<'a>(
    app: &'a App,
    theme: &Theme,
    area: Rect,
) -> Paragraph<'a> {
    let block = Block::default()
        .title(" Diagnostics ")
        .borders(Borders::ALL)
        .border_style(theme.focused_border_style(app.focused == Panel::Diagnostics));
    let visible_height = area.height.saturating_sub(2);
    let content_lines = app.diagnostic_lines.len() as u16;
    let max_scroll = content_lines.saturating_sub(visible_height);
    let scroll = app.scroll_diag.min(max_scroll);
    Paragraph::new(app.diagnostic_lines.clone())
        .block(block)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false })
}
