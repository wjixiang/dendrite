use ratatui::{
    layout::Constraint,
    style::{Color, Style},
    text::{Line, Span},
};

/// Layout constraints for the main vertical split:
/// 70% top row (4 panels), 30% bottom row (1 panel), 1 row help bar.
pub const LAYOUT_CONSTRAINTS: [Constraint; 3] = [
    Constraint::Percentage(70),
    Constraint::Percentage(30),
    Constraint::Min(1),
];

/// Top row: Tree / KnowledgeEntity / Agent — three columns.
pub fn top_h_constraints(widths: &[u16; 3]) -> [Constraint; 3] {
    [
        Constraint::Percentage(widths[0]),
        Constraint::Percentage(widths[1]),
        Constraint::Percentage(widths[2]),
    ]
}

/// Bottom row: Diagnostics — single full-width panel.
pub const BOTTOM_H_CONSTRAINTS: [Constraint; 1] = [Constraint::Percentage(100)];

/// Help bar text shown at the bottom of the screen.
pub fn help_text() -> Line<'static> {
    Line::from(vec![
        Span::styled(" [↑↓] Scroll ", Style::default().fg(Color::DarkGray)),
        Span::styled(" [⇧H/L] Panel ", Style::default().fg(Color::DarkGray)),
        Span::styled(" [Ctrl+⇧J/K] Resize ", Style::default().fg(Color::DarkGray)),
        Span::styled(" [s] Settings ", Style::default().fg(Color::DarkGray)),
        Span::styled(" [q] Quit ", Style::default().fg(Color::DarkGray)),
    ])
}

/// Style for the help bar background.
pub fn help_style() -> Style {
    Style::default().bg(Color::DarkGray)
}

/// Build the help bar widget.
pub fn render_help_bar() -> ratatui::widgets::Paragraph<'static> {
    ratatui::widgets::Paragraph::new(vec![help_text()]).style(help_style())
}
