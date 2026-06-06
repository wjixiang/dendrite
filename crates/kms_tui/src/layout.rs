use ratatui::{
    layout::Constraint,
    style::{Color, Style},
    text::{Line, Span},
};

/// Layout constraints: main row (4 panels) + help bar.
pub const LAYOUT_CONSTRAINTS: [Constraint; 2] =
    [Constraint::Percentage(95), Constraint::Percentage(5)];

/// Main row: Tree / KnowledgeEntity / Agent / Diagnostics — four columns.
pub const COLUMN_CONSTRAINTS: [Constraint; 3] = [
    Constraint::Percentage(25),
    Constraint::Percentage(50),
    Constraint::Percentage(25),
];

pub const AGENT_DIAG_CONSTRAINTS: [Constraint; 2] =
    [Constraint::Percentage(80), Constraint::Percentage(20)];

/// Help bar text shown at the bottom of the screen.
pub fn help_text() -> Line<'static> {
    Line::from(vec![
        Span::styled(" [↑↓] Scroll ", Style::default().fg(Color::Green)),
        Span::styled(" [G] Bottom ", Style::default().fg(Color::Green)),
        Span::styled(" [⇧H/L] Panel ", Style::default().fg(Color::DarkGray)),
        Span::styled(" [s] Settings ", Style::default().fg(Color::DarkGray)),
        Span::styled(" [q] Quit ", Style::default().fg(Color::DarkGray)),
    ])
}

/// Style for the help bar background.
pub fn help_style() -> Style {
    Style::default()
}

/// Build the help bar widget.
pub fn render_help_bar() -> ratatui::widgets::Paragraph<'static> {
    ratatui::widgets::Paragraph::new(vec![help_text()]).style(help_style())
}
