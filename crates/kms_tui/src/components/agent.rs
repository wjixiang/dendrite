use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::chat::ChatMessage;
use crate::state::{App, Panel};
use crate::theme::Theme;

const SPINNER_FRAMES: &[&str] = &[
    "\u{2807}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}",
];

/// Count the number of **post-wrap** visual rows a list of source
/// `Line`s would occupy when wrapped to `inner_width` columns.
///
/// This mirrors what `Paragraph::render` does internally (see
/// `WordWrapper::next_line` in ratatui's `paragraph.rs`). It is
/// intentionally character-based rather than word-based so it never
/// *under*-counts: word wrap can never insert more breaks than
/// naive character wrap, so a char-wrap total is a safe upper bound
/// on the actual wrap total. An over-estimate of `max_scroll` is
/// harmless (extra empty rows at the top); an under-estimate hides
/// real content, which is the bug we are guarding against.
///
/// Used to compute the maximum `Paragraph::scroll.y` value for the
/// chat panel, so the auto-scroll pin and the user-driven `j`/`k`
/// scroll both bottom out at the actual last visible row.
fn wrapped_line_count(lines: &[Line<'_>], inner_width: usize) -> usize {
    if inner_width == 0 {
        return lines.len();
    }
    lines
        .iter()
        .map(|line| {
            let width = line.width();
            // 0-width lines (blank dividers, empty assistant
            // messages) still occupy one visual row.
            width.div_ceil(inner_width).max(1)
        })
        .sum()
}

/// Extra messages to include above the viewport start. Covers the
/// gap between our cheap `estimate_lines()` (which counts pre-wrap
/// Line objects) and Paragraph's actual post-wrap visual rows.
const VIEWPORT_MSG_BUFFER: usize = 3;

/// Maximum number of messages to render per frame. When the total
/// message count exceeds this threshold, viewport culling kicks in:
/// only the messages near the current scroll position are converted
/// to render lines. For short sessions (<= this value) all messages
/// are rendered — identical to the pre-optimization behavior.
const RENDER_ALL_THRESHOLD: usize = 50;

/// Find the range of message indices `[start, end)` that should be
/// rendered given the current scroll position. Uses
/// `ChatMessage::estimate_lines()` as a cheap proxy for visual
/// height to walk messages until the cumulative estimate exceeds
/// `scroll_y`.
///
/// When the total message count is small (below `RENDER_ALL_THRESHOLD`),
/// returns `(0, len)` — no culling, identical to the old behavior.
fn visible_message_range(
    messages: &[ChatMessage],
    scroll_y: usize,
) -> (usize, usize) {
    let len = messages.len();
    if len <= RENDER_ALL_THRESHOLD {
        return (0, len);
    }

    let mut cumulative: usize = 0;
    let mut start: usize = 0;
    for (i, msg) in messages.iter().enumerate() {
        if cumulative >= scroll_y {
            start = i.saturating_sub(VIEWPORT_MSG_BUFFER);
            break;
        }
        cumulative += msg.estimate_lines();
        // If we exhausted all messages without reaching scroll_y,
        // the scroll is pinned to the bottom. Show the tail.
        if i + 1 == len {
            start = len.saturating_sub(RENDER_ALL_THRESHOLD);
        }
    }

    let end = (start + RENDER_ALL_THRESHOLD).min(len);
    (start, end)
}

pub fn render_agent(f: &mut Frame, app: &mut App, theme: &Theme, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let kind_label = app.agent_kind.label();
    let title = if app.providers.is_empty() {
        // No providers configured yet. The TUI started in
        // "needs configuration" mode — surface that clearly.
        format!(" Agent [{}] (no providers) ", kind_label)
    } else if app.pool_entries.is_empty() {
        format!(" Agent [{}] (pool empty) ", kind_label)
    } else if app.pool_entries.len() == 1 {
        let e = &app.pool_entries[0];
        let prov = app
            .providers
            .iter()
            .find(|p| p.id == e.provider_id)
            .map(|p| p.short_label())
            .unwrap_or_else(|| e.provider_id.clone());
        format!(" Agent [{}] ({}/{}) ", kind_label, prov, e.model)
    } else {
        format!(
            " Agent [{}] ({} models) ",
            kind_label,
            app.pool_entries.len()
        )
    };

    let (rendered_lines, skipped_rows): (Vec<Line<'static>>, usize) =
        if app.providers.is_empty() {
        // Empty-pool first-run hint instead of the normal chat history.
        (vec![
            Line::from(Span::styled(
                "  No LLM providers configured.",
                Style::default()
                    .fg(theme.warning)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  All provider credentials live in data/settings.json and are",
                Style::default().fg(theme.text_muted),
            )),
            Line::from(Span::styled(
                "  managed through the in-TUI settings form. Environment",
                Style::default().fg(theme.text_muted),
            )),
            Line::from(Span::styled(
                "  variables like MIMO_API_KEY / MINIMAX_* are not used.",
                Style::default().fg(theme.text_muted),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  \u{2795}  Press [s] to open Settings, then [n] to add a provider.",
                Style::default().fg(theme.accent),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "    \u{2022} Type:   cycle with \u{2191}/\u{2193} (mimo, minimax, ...)",
                Style::default().fg(theme.text_secondary),
            )),
            Line::from(Span::styled(
                "    \u{2022} Name:   free text (e.g. \"mimo-prod\")",
                Style::default().fg(theme.text_secondary),
            )),
            Line::from(Span::styled(
                "    \u{2022} API key:  required, masked while you type",
                Style::default().fg(theme.text_secondary),
            )),
            Line::from(Span::styled(
                "    \u{2022} URL:    optional, pre-filled with the provider default",
                Style::default().fg(theme.text_secondary),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Multiple providers of the same type (e.g. two mimo entries",
                Style::default().fg(theme.text_muted),
            )),
            Line::from(Span::styled(
                "  with different API keys) are supported to fan out TPM.",
                Style::default().fg(theme.text_muted),
            )),
        ], 0)
    } else {
        let panel_state = app.parallel_panel.as_ref();
        let to_lines_width = area.width.saturating_sub(4) as usize;
        let messages = app.agent_messages();

        // --- Viewport culling: two-phase rendering ---
        //
        // Phase A: cheap pre-scan with estimate_lines() to find the
        // visible message range and compute scroll bounds.
        let inner_height = chunks[0].height.saturating_sub(2) as usize;
        let est_total: usize = messages.iter().map(|m| m.estimate_lines()).sum();
        let max_scroll = est_total.saturating_sub(inner_height);
        let scroll_y: usize = if app.agent_auto_scroll {
            max_scroll
        } else {
            (app.agent_scroll as usize).min(max_scroll)
        };

        let (start, end) = visible_message_range(messages, scroll_y);
        let skipped_rows: usize = messages[..start]
            .iter()
            .map(|m| m.estimate_lines())
            .sum();

        // Phase B: expensive to_lines() only on the visible slice.
        let lines: Vec<Line<'static>> = messages[start..end]
            .iter()
            .flat_map(|msg| msg.to_lines(theme, panel_state, to_lines_width))
            .collect();

        (lines, skipped_rows)
    };

    let conv_block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(theme.focused_border_style(app.focused == Panel::Agent));

    // The Paragraph's scroll offset must be relative to the rendered
    // slice (which starts at message `start`, not message 0). We
    // subtract the estimated rows occupied by skipped messages.
    //
    // `estimate_lines()` counts `Line` objects, not post-wrap visual
    // rows, so `skipped_rows` is an *under*-estimate of the actual
    // visual rows skipped. This means `paragraph_scroll` may be
    // slightly *over*-estimated — a few extra lines at the top are
    // shown, clipped by the viewport. The safe direction.
    let inner_width = chunks[0].width.saturating_sub(2) as usize;
    let rendered_max = wrapped_line_count(&rendered_lines, inner_width);
    let paragraph_scroll = (skipped_rows as usize).min(rendered_max) as u16;

    let conv = Paragraph::new(rendered_lines)
        .block(conv_block)
        .wrap(Wrap { trim: false })
        .scroll((paragraph_scroll, 0));
    f.render_widget(conv, chunks[0]);

    let status_line = if app.providers.is_empty() {
        // No providers — no input, just the hint to open settings.
        Line::from(vec![Span::styled(
            "  [s] open settings  \u{2022}  [q] quit",
            Style::default().fg(theme.text_muted),
        )])
    } else if app.agent_running {
        let spinner_char = SPINNER_FRAMES[app.spinner_tick % SPINNER_FRAMES.len()];
        let usage_suffix = match app.agent_usage_tokens {
            Some(tokens) => format!(" ({} tokens)", tokens),
            None => String::new(),
        };
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(spinner_char.to_string(), Style::default().fg(theme.spinner)),
            Span::styled(
                format!(" Agent [{}] running{} ", kind_label, usage_suffix),
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
            Span::styled("[a] switch", Style::default().fg(theme.text_muted)),
        ])
    };

    let input_bg = if app.agent_input_active {
        theme.input_bg
    } else {
        ratatui::style::Color::Black
    };
    let input_line = Paragraph::new(status_line).style(Style::default().bg(input_bg));
    f.render_widget(input_line, chunks[1]);

    if app.agent_input_active && app.focused == Panel::Agent && !app.providers.is_empty() {
        let cursor_x = chunks[1].x + 2 + app.agent_input.len() as u16;
        let cursor_y = chunks[1].y;
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

#[cfg(test)]
mod wrap_tests {
    use super::*;

    /// Regression guard for the "auto-scroll pin stops short of the
    /// real bottom" bug. Before the fix, the chat scroll math used
    /// `rendered_lines.len()` directly, but `Paragraph` interprets
    /// `scroll.y` as an offset into the **post-wrap** line stream.
    /// When any source `Line` is wider than the viewport, the wrap
    /// produces extra rows and the old formula hid them at the
    /// bottom of the chat. This test pins a 30-char line inside a
    /// 10-col viewport to 3 wrapped rows so a future change can't
    /// silently regress to `line_count = lines.len()`.
    #[test]
    fn long_line_wraps_into_multiple_rows() {
        let lines = vec![Line::from("a".repeat(30))];
        assert_eq!(wrapped_line_count(&lines, 10), 3);
    }

    #[test]
    fn line_fits_in_viewport_counts_as_one() {
        let lines = vec![Line::from("hello world")];
        assert_eq!(wrapped_line_count(&lines, 80), 1);
    }

    #[test]
    fn empty_line_still_occupies_a_row() {
        // A blank divider is a `Line` with zero width; the renderer
        // still emits one row for it. Guard against a naive
        // `width / inner_width` that would round 0/80 down to 0.
        let lines = vec![Line::from("")];
        assert_eq!(wrapped_line_count(&lines, 80), 1);
    }

    #[test]
    fn inner_width_zero_falls_back_to_line_count() {
        // Defensive: if the panel is so narrow that the border
        // consumes the entire width, we hand back 1 row per line
        // rather than divide-by-zero. The renderer will just show
        // an empty Paragraph in that case anyway.
        let lines = vec![Line::from("a"), Line::from("bb")];
        assert_eq!(wrapped_line_count(&lines, 0), 2);
    }

    #[test]
    fn mixed_short_and_long_lines_sum() {
        // Mix of one short, one empty, one wrapping. The total must
        // be the sum of each line's wrapped count, not the line
        // count.
        let lines = vec![Line::from("ok"), Line::from(""), Line::from("z".repeat(25))];
        // 1 (ok) + 1 (empty) + ceil(25/10) = 1+1+3 = 5
        assert_eq!(wrapped_line_count(&lines, 10), 5);
    }

    // ---- Viewport culling tests ----

    #[test]
    fn short_history_renders_all() {
        let messages: Vec<ChatMessage> = (0..30).map(|_| ChatMessage::Divider).collect();
        let (start, end) = visible_message_range(&messages, 0);
        assert_eq!(start, 0);
        assert_eq!(end, 30);
    }

    #[test]
    fn long_history_at_top_renders_first_batch() {
        let messages: Vec<ChatMessage> = (0..200).map(|_| ChatMessage::Divider).collect();
        let (start, end) = visible_message_range(&messages, 0);
        assert_eq!(start, 0);
        assert_eq!(end, RENDER_ALL_THRESHOLD);
    }

    #[test]
    fn long_history_at_bottom_includes_last_messages() {
        let messages: Vec<ChatMessage> = (0..200).map(|_| ChatMessage::Divider).collect();
        let est_total: usize = messages.iter().map(|m| m.estimate_lines()).sum();
        let max_scroll = est_total.saturating_sub(24);
        let (_start, end) = visible_message_range(&messages, max_scroll);
        assert_eq!(end, 200); // always includes the last message
    }

    #[test]
    fn visible_range_never_exceeds_threshold() {
        let messages: Vec<ChatMessage> = (0..500).map(|_| ChatMessage::Divider).collect();
        for scroll in [0, 100, 250, 499] {
            let (start, end) = visible_message_range(&messages, scroll);
            assert!(
                end - start <= RENDER_ALL_THRESHOLD,
                "scroll={scroll}: range [{start},{end}) exceeds threshold",
            );
        }
    }

    #[test]
    fn empty_history_returns_empty_range() {
        let messages: Vec<ChatMessage> = vec![];
        let (start, end) = visible_message_range(&messages, 0);
        assert_eq!(start, 0);
        assert_eq!(end, 0);
    }

    #[test]
    fn threshold_boundary_exact() {
        let messages: Vec<ChatMessage> = (0..RENDER_ALL_THRESHOLD).map(|_| ChatMessage::Divider).collect();
        let (_start, end) = visible_message_range(&messages, 0);
        assert_eq!(end, RENDER_ALL_THRESHOLD);
    }

    #[test]
    fn one_over_threshold_triggers_culling() {
        let messages: Vec<ChatMessage> = (0..RENDER_ALL_THRESHOLD + 1).map(|_| ChatMessage::Divider).collect();
        let (start, end) = visible_message_range(&messages, 0);
        assert_eq!(start, 0);
        assert_eq!(end, RENDER_ALL_THRESHOLD);
    }
}
