use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

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
/// rendered given the current scroll position. Uses pre-computed
/// `estimate_lines()` counts (from the estimate cache) to walk
/// messages until the cumulative estimate exceeds `scroll_y`.
///
/// When the total message count is small (below `RENDER_ALL_THRESHOLD`),
/// returns `(0, total)` — no culling, identical to the old behavior.
fn visible_message_range(estimates: &[usize], scroll_y: usize, total: usize) -> (usize, usize) {
    if total <= RENDER_ALL_THRESHOLD {
        return (0, total);
    }

    let mut cumulative: usize = 0;
    let mut start: usize = 0;
    for (i, &est) in estimates.iter().enumerate() {
        if cumulative >= scroll_y {
            start = i.saturating_sub(VIEWPORT_MSG_BUFFER);
            break;
        }
        cumulative += est;
        // If we exhausted all messages without reaching scroll_y,
        // the scroll is pinned to the bottom. Show the tail.
        if i + 1 == total {
            start = total.saturating_sub(RENDER_ALL_THRESHOLD);
        }
    }

    let end = (start + RENDER_ALL_THRESHOLD).min(total);
    (start, end)
}

/// Translate a **global** wrapped-row scroll offset (an offset into
/// the whole chat history) into the **local** `Paragraph::scroll.y`
/// for the window `[start, end)` that we actually render.
///
/// `Paragraph::scroll.y` is an offset into the rendered `Vec<Line>`
/// slice — NOT into the original history — so when we cull off-screen
/// messages we must subtract the rows that live above the window.
///
/// The result is **not clamped to the window's max**: callers want to
/// let the user scroll past the bottom of the history (the panel goes
/// blank), so any clamp belongs at the call site, not here. Underflow
/// is still guarded by `saturating_sub`.
///
/// `counts` is the per-message post-wrap row count for the full
/// history; `start..end` is the visible window picked by
/// `visible_message_range`; `inner_height` is the viewport height
/// (chat panel minus borders).
fn translate_scroll(
    counts: &[usize],
    start: usize,
    _end: usize,
    global_scroll: usize,
    _inner_height: usize,
) -> usize {
    let rows_before_window: usize = counts[..start].iter().sum();
    global_scroll.saturating_sub(rows_before_window)
}

/// Main function of Agent message rendering
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

    let inner_height = chunks[0].height.saturating_sub(2) as usize;
    let inner_width = chunks[0].width.saturating_sub(2) as usize;

    let (rendered_lines, scroll_y): (Vec<Line<'static>>, u16) = if app.providers.is_empty() {
        // Empty-pool first-run hint instead of the normal chat history.
        let lines = vec![
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
        ];
        (lines, 0)
    } else {
        let msg_version = app.message_version;
        let msg_len = app.agent_messages().len();

        // --- Refresh the per-message wrapped-row cache ---
        //
        // Keyed by (message_version, inner_width): the wrap layout
        // changes on resize, so the post-wrap row count for a given
        // message is only stable while the width is unchanged. We
        // always cache counts for the **whole** history (not just the
        // visible window) because the scroll math below needs the
        // global total, not the window-local one.
        let cache_fresh = matches!(
            &app.cached_estimates,
            Some((ver, w, counts)) if *ver == msg_version
                && *w == inner_width
                && counts.len() == msg_len
        );
        if !cache_fresh {
            let counts: Vec<usize> = {
                let msgs = app.agent_messages();
                msgs.iter()
                    .map(|m| wrapped_line_count(&m.to_lines(theme), inner_width))
                    .collect()
            };
            app.cached_estimates = Some((msg_version, inner_width, counts));
        }
        // Snapshot the counts so the subsequent &mut app borrows for
        // the line cache don't conflict with the immutable borrow of
        // the count cache.
        let counts: Vec<usize> = app.cached_estimates.as_ref().unwrap().2.clone();
        let total_rows: usize = counts.iter().sum();
        let global_max_scroll = total_rows.saturating_sub(inner_height);

        // The actual global scroll target for this frame, in post-wrap
        // row units. Auto-scroll pins to the global bottom; otherwise
        // we forward the user's `agent_scroll` verbatim — including
        // values past `global_max_scroll`, which lets the user scroll
        // off the bottom into blank space (intentional: no clamp here).
        let global_scroll = if app.agent_auto_scroll {
            global_max_scroll
        } else {
            app.agent_scroll as usize
        };

        // --- Pick the visible message window around the actual scroll target ---
        let (start, end) = if msg_len > RENDER_ALL_THRESHOLD {
            visible_message_range(&counts, global_scroll, msg_len)
        } else {
            (0, msg_len)
        };

        // --- Render visible messages, with line cache ---
        let cache_hit = matches!(
            &app.cached_agent_lines,
            Some((ver, s, e, _lines)) if *ver == msg_version && *s == start && *e == end
        );
        let lines: Vec<Line<'static>> = if cache_hit {
            app.cached_agent_lines.as_ref().unwrap().3.clone()
        } else {
            let lines: Vec<Line<'static>> = {
                let msgs = app.agent_messages();
                msgs[start..end]
                    .iter()
                    .flat_map(|msg| msg.to_lines(theme))
                    .collect()
            };
            app.cached_agent_lines = Some((msg_version, start, end, lines.clone()));
            lines
        };

        // --- Translate global scroll into a local Paragraph offset ---
        //
        // `lines` only contains the windowed slice [start, end), but
        // `Paragraph::scroll.y` is an offset into THAT slice — not into
        // the whole history. `translate_scroll` subtracts the rows
        // that live above the window and clamps to the window's own
        // max so a stale scroll value (or a window that doesn't fully
        // cover the viewport at the very bottom) can't push content
        // off-screen.
        let local_scroll = translate_scroll(&counts, start, end, global_scroll, inner_height);
        (lines, local_scroll.min(u16::MAX as usize) as u16)
    };

    let conv_block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(theme.focused_border_style(app.focused == Panel::Agent));

    let conv = Paragraph::new(rendered_lines)
        .block(conv_block)
        .wrap(Wrap { trim: false })
        .scroll((scroll_y, 0));
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
        let estimates = vec![1usize; 30];
        let (start, end) = visible_message_range(&estimates, 0, 30);
        assert_eq!(start, 0);
        assert_eq!(end, 30);
    }

    #[test]
    fn long_history_at_top_renders_first_batch() {
        let estimates = vec![1usize; 200];
        let (start, end) = visible_message_range(&estimates, 0, 200);
        assert_eq!(start, 0);
        assert_eq!(end, RENDER_ALL_THRESHOLD);
    }

    #[test]
    fn long_history_at_bottom_includes_last_messages() {
        let n = 200;
        let estimates = vec![1usize; n];
        let est_total: usize = estimates.iter().sum();
        let max_scroll = est_total.saturating_sub(24);
        let (_start, end) = visible_message_range(&estimates, max_scroll, n);
        assert_eq!(end, 200); // always includes the last message
    }

    #[test]
    fn visible_range_never_exceeds_threshold() {
        let n = 500;
        let estimates = vec![1usize; n];
        for scroll in [0, 100, 250, 499] {
            let (start, end) = visible_message_range(&estimates, scroll, n);
            assert!(
                end - start <= RENDER_ALL_THRESHOLD,
                "scroll={scroll}: range [{start},{end}) exceeds threshold",
            );
        }
    }

    #[test]
    fn empty_history_returns_empty_range() {
        let estimates: Vec<usize> = vec![];
        let (start, end) = visible_message_range(&estimates, 0, 0);
        assert_eq!(start, 0);
        assert_eq!(end, 0);
    }

    #[test]
    fn threshold_boundary_exact() {
        let estimates = vec![1usize; RENDER_ALL_THRESHOLD];
        let (_start, end) = visible_message_range(&estimates, 0, RENDER_ALL_THRESHOLD);
        assert_eq!(end, RENDER_ALL_THRESHOLD);
    }

    #[test]
    fn one_over_threshold_triggers_culling() {
        let n = RENDER_ALL_THRESHOLD + 1;
        let estimates = vec![1usize; n];
        let (start, end) = visible_message_range(&estimates, 0, n);
        assert_eq!(start, 0);
        assert_eq!(end, RENDER_ALL_THRESHOLD);
    }

    // ---- Scroll-translation tests (regression: long history bottom) ----

    /// Without culling (window covers the whole history), the local
    /// Paragraph scroll must equal the global scroll. Anything else
    /// would visibly shift the chat for short sessions.
    #[test]
    fn translate_no_culling_passes_scroll_through() {
        let counts = vec![5usize; 10]; // 10 messages × 5 rows = 50
        let local = translate_scroll(&counts, 0, 10, 30, 20);
        assert_eq!(local, 30);
    }

    /// When the window starts at the top of history (start = 0) there
    /// are no rows above the window to subtract, so local == global.
    #[test]
    fn translate_window_at_top_is_identity() {
        let counts = vec![5usize; 100];
        let local = translate_scroll(&counts, 0, 50, 100, 24);
        assert_eq!(local, 100);
    }

    /// **Bug fix guard**: in culling mode, the window's `start` is
    /// not at message 0, so `Paragraph::scroll.y` must be
    /// `global_scroll - rows_before_window`. The pre-fix code passed
    /// `global_scroll` directly into Paragraph, which stranded the
    /// user somewhere in the middle of the window.
    #[test]
    fn translate_culled_window_subtracts_prefix_rows() {
        let counts = vec![5usize; 100]; // total 500 rows
        // Window: messages [40, 90) → rows_before_window = 200.
        // Global scroll at row 250.
        let local = translate_scroll(&counts, 40, 90, 250, 24);
        assert_eq!(local, 50);
    }

    /// When auto-scroll pins to the global bottom, the window covers
    /// the tail and the local offset lands at the window's last
    /// `inner_height` rows.
    #[test]
    fn translate_at_global_bottom_lands_at_window_bottom() {
        let counts = vec![5usize; 100];
        let total: usize = counts.iter().sum(); // 500
        let inner_height = 24;
        let global_scroll = total - inner_height; // 476
        // Tail window: [50, 100) → rows_before_window = 250.
        let local = translate_scroll(&counts, 50, 100, global_scroll, inner_height);
        // 476 - 250 = 226.
        assert_eq!(local, 226);
    }

    /// **No clamp** at the function boundary: callers explicitly want
    /// to let the user scroll past the bottom (the Paragraph then
    /// renders blank rows). Confirms we forward the raw arithmetic
    /// regardless of how far past the window the scroll goes.
    #[test]
    fn translate_overshoot_is_not_clamped() {
        let counts = vec![5usize; 60];
        let local = translate_scroll(&counts, 0, 50, 10_000, 24);
        // 10_000 - 0 = 10_000; window's local_max would have been 226,
        // but we no longer clamp.
        assert_eq!(local, 10_000);
    }

    /// Underflow is still guarded by `saturating_sub`: a global
    /// scroll below `rows_before_window` (transient state during a
    /// key burst) must saturate at 0 rather than wrap around.
    #[test]
    fn translate_undershoot_saturates_at_zero() {
        let counts = vec![5usize; 60];
        // Window starts at message 30 → 150 rows above.
        let local = translate_scroll(&counts, 30, 60, 50, 24);
        assert_eq!(local, 0);
    }

    /// Variable-height messages (the realistic case): the prefix sum
    /// must be exact, not approximated by message count × average.
    #[test]
    fn translate_variable_height_messages() {
        let counts = vec![3usize, 7, 12, 4, 9]; // total = 35
        // Window: [2, 5) → rows_before_window = 3 + 7 = 10.
        let local = translate_scroll(&counts, 2, 5, 22, 8);
        // 22 - 10 = 12 (no clamp).
        assert_eq!(local, 12);
    }

    /// Empty window (start == end) and zero inner height shouldn't
    /// panic. Without a window-max clamp the call now returns the
    /// raw `global - rows_before` — `Paragraph` will render blank.
    #[test]
    fn translate_empty_window_does_not_panic() {
        let counts = vec![3usize, 7, 12];
        let local = translate_scroll(&counts, 1, 1, 5, 0);
        // rows_before_window = 3; 5 - 3 = 2 (no clamp).
        assert_eq!(local, 2);
    }
}
