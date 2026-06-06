use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::theme::Theme;

pub fn style_diagnostic_line(line: &str, theme: &Theme) -> Line<'static> {
    if line.starts_with("[ERROR]") {
        Line::from(Span::styled(
            line.to_owned(),
            Style::default()
                .fg(theme.error)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ))
    } else if line.starts_with("[WARN]") {
        Line::from(Span::styled(
            line.to_owned(),
            Style::default().fg(theme.warning),
        ))
    } else if line.starts_with("[INFO]") {
        Line::from(Span::styled(
            line.to_owned(),
            Style::default().fg(theme.info),
        ))
    } else if line.starts_with("[HINT]") || line.starts_with("  \u{2192}") {
        Line::from(Span::styled(
            line.to_owned(),
            Style::default().fg(theme.text_muted),
        ))
    } else {
        Line::from(line.to_owned())
    }
}
