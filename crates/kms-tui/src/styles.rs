use ratatui::{
    style::{Color, Modifier, Style},
    text::Line,
};

/// Severity-based color and modifier theme.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub fg: Color,
    pub modifier: Modifier,
}

impl Theme {
    pub const fn error() -> Self {
        Self {
            fg: Color::Red,
            modifier: Modifier::BOLD,
        }
    }

    pub const fn warn() -> Self {
        Self {
            fg: Color::Yellow,
            modifier: Modifier::empty(),
        }
    }

    pub const fn info() -> Self {
        Self {
            fg: Color::Cyan,
            modifier: Modifier::empty(),
        }
    }

    pub const fn hint() -> Self {
        Self {
            fg: Color::DarkGray,
            modifier: Modifier::empty(),
        }
    }

    pub const fn indent() -> Self {
        Self {
            fg: Color::DarkGray,
            modifier: Modifier::empty(),
        }
    }
}

/// Style a diagnostic line based on its prefix tags.
pub fn style_diagnostic_line(line: &str) -> Line<'static> {
    if line.starts_with("[ERROR]") {
        let t = Theme::error();
        Line::from(Span::styled(line.to_owned(), Style::default().fg(t.fg).add_modifier(t.modifier)))
    } else if line.starts_with("[WARN]") {
        let t = Theme::warn();
        Line::from(Span::styled(line.to_owned(), Style::default().fg(t.fg)))
    } else if line.starts_with("[INFO]") {
        let t = Theme::info();
        Line::from(Span::styled(line.to_owned(), Style::default().fg(t.fg)))
    } else if line.starts_with("[HINT]") {
        let t = Theme::hint();
        Line::from(Span::styled(line.to_owned(), Style::default().fg(t.fg)))
    } else if line.starts_with("  →") {
        let t = Theme::indent();
        Line::from(Span::styled(line.to_owned(), Style::default().fg(t.fg)))
    } else {
        Line::from(line.to_owned())
    }
}

use ratatui::text::Span;
