use ratatui::{
    layout::Constraint,
    style::{Color, Style},
    text::{Line, Span},
};

/// Layout constraints for the main vertical split:
/// 70% top row, 30% bottom row, 1 row help bar.
pub const LAYOUT_CONSTRAINTS: [Constraint; 3] = [
    Constraint::Percentage(70),
    Constraint::Percentage(30),
    Constraint::Min(1),
];

/// Top row: Tree / Knowledge / Entity — three equal columns.
pub const TOP_H_CONSTRAINTS: [Constraint; 3] = [
    Constraint::Percentage(33),
    Constraint::Percentage(34),
    Constraint::Percentage(33),
];

/// Bottom row: Diagnostics / Agent — two equal columns.
pub const BOTTOM_H_CONSTRAINTS: [Constraint; 2] = [
    Constraint::Percentage(50),
    Constraint::Percentage(50),
];

/// Help bar text shown at the bottom of the screen.
pub fn help_text() -> Line<'static> {
    Line::from(vec![
        Span::styled(" [↑↓] Scroll ", Style::default().fg(Color::DarkGray)),
        Span::styled(" [Tab] Panel ", Style::default().fg(Color::DarkGray)),
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

