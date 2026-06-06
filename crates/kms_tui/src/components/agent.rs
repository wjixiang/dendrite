use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::state::{App, Panel};
use crate::theme::Theme;

const SPINNER_FRAMES: &[&str] = &["\u{2807}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}"];

/// Conservative safety margin subtracted from the scroll offset before
/// handing it to `Paragraph::scroll`. ratatui internally does
/// `area.height + scroll.y` to decide when to stop rendering; if the
/// sum overflows `u16` the widget panics. The margin guarantees we
/// stay well below that ceiling even with edge-case frame timing.
const SCROLL_OVERFLOW_MARGIN: u16 = 1;

pub fn render_agent(f: &mut Frame, app: &mut App, theme: &Theme, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let rendered_lines: Vec<Line<'static>> = app
        .agent_messages()
        .iter()
        .flat_map(|msg| msg.to_lines(theme))
        .collect();

    let kind_label = app.agent_kind.label();
    let conv_block = Block::default()
        .title(format!(" Agent [{}] ", kind_label))
        .borders(Borders::ALL)
        .border_style(theme.focused_border_style(app.focused == Panel::Agent));

    // Use ratatui's own `Paragraph::line_count` to learn the exact
    // post-wrap visual line count for the conversation. This is the
    // same `WordWrapper` that `Paragraph` uses at draw time, so the
    // value is precise. The width we pass is the full `chunks[0]`
    // area width (border included) because `line_count` accounts for
    // the block's inner space automatically.
    let total_visual = Paragraph::new(rendered_lines.clone())
        .block(conv_block.clone())
        .wrap(Wrap { trim: false })
        .line_count(chunks[0].width.max(1));

    // The visible viewport height is the *inner* height (the area
    // available for text once the border is removed). `line_count`
    // already added the two border rows, so subtracting `area.height`
    // yields the maximum legal scroll offset (0 when content fits).
    let max_scroll = total_visual.saturating_sub(chunks[0].height as usize) as u16;
    let safe_max = max_scroll.saturating_sub(SCROLL_OVERFLOW_MARGIN);

    let scroll = app.agent_scroll.min(safe_max);
    app.agent_scroll = scroll;

    let conv = Paragraph::new(rendered_lines)
        .block(conv_block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(conv, chunks[0]);

    let status_line = if app.agent_running {
        let spinner_char = SPINNER_FRAMES[app.spinner_tick % SPINNER_FRAMES.len()];
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(spinner_char.to_string(), Style::default().fg(theme.spinner)),
            Span::styled(
                format!(" Agent [{}] running... ", kind_label),
                Style::default().fg(theme.spinner),
            ),
        ])
    } else if app.agent_input_active {
        Line::from(vec![Span::raw(format!("> {}", app.agent_input))])
    } else {
        Line::from(vec![
            Span::styled(
                format!("  (Enter to type) [{}] ", kind_label),
                Style::default().fg(theme.text_muted),
            ),
            Span::styled(
                "[a] switch",
                Style::default().fg(theme.text_muted),
            ),
        ])
    };

    let input_bg = if app.agent_input_active {
        theme.input_bg
    } else {
        ratatui::style::Color::Black
    };
    let input_line = Paragraph::new(status_line).style(Style::default().bg(input_bg));
    f.render_widget(input_line, chunks[1]);

    if app.agent_input_active && app.focused == Panel::Agent {
        let cursor_x = chunks[1].x + 2 + app.agent_input.len() as u16;
        let cursor_y = chunks[1].y;
        f.set_cursor_position((cursor_x, cursor_y));
    }
}
