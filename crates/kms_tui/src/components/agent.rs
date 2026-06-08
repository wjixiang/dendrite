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

/// Cap the user-driven `agent_scroll` at the actual history bottom
/// and return the row that should be displayed this frame.
///
/// Two responsibilities:
///   * Auto-scroll → always returns `max_scroll` (pin to bottom).
///   * Manual scroll → returns `min(agent_scroll, max_scroll)` so
///     the stored value never drifts past the real history end.
///
/// The caller writes the result back to `app.agent_scroll`. That
/// writeback lets `j`/`k` from auto-scroll pick up at the bottom
/// instead of jumping to the top from a stale `agent_scroll = 0`.
fn resolve_global_scroll(agent_scroll: u16, auto_scroll: bool, max_scroll: usize) -> usize {
    if auto_scroll {
        max_scroll
    } else {
        (agent_scroll as usize).min(max_scroll)
    }
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
    let inner_width = chunks[0].width.saturating_sub(2) as u16;

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

        // --- Flatten the full chat history into a Vec<Line> ---
        //
        // No more message-window culling: we always render the whole
        // history, then ask Paragraph itself how tall it ends up.
        // This eliminates the "estimate wrap, then translate" step
        // that was the source of the unit-mismatch bug.
        let cache_hit = matches!(
            &app.cached_agent_lines,
            Some((ver, _lines)) if *ver == msg_version
        );
        let lines: Vec<Line<'static>> = if cache_hit {
            app.cached_agent_lines.as_ref().unwrap().1.clone()
        } else {
            let lines: Vec<Line<'static>> = app
                .agent_messages()
                .iter()
                .flat_map(|m| m.to_lines(theme))
                .collect();
            app.cached_agent_lines = Some((msg_version, lines.clone()));
            lines
        };

        // --- Get the *exact* post-wrap visual row count ---
        //
        // `Paragraph::line_count(width)` walks the same `WordWrapper`
        // the renderer uses, so the number it returns is the number
        // of visual rows the user will actually see — no char-based
        // upper bound, no per-message rollup, no approximation. We
        // cache it on `(msg_version, inner_width)` so a stable frame
        // skips the second `WordWrapper` pass (one for `line_count`,
        // one for `render`).
        let total_visual_rows: usize = match &app.cached_estimates {
            Some((ver, w, rows)) if *ver == msg_version && *w == inner_width => *rows,
            _ => {
                let probe = Paragraph::new(lines.clone()).wrap(Wrap { trim: false });
                let rows = probe.line_count(inner_width);
                app.cached_estimates = Some((msg_version, inner_width, rows));
                rows
            }
        };

        // --- Clamp `agent_scroll` to the real history bottom ---
        //
        // Both operands are visual rows (Paragraph's native unit), so
        // the subtraction is exact. `max_scroll` is the largest valid
        // value for `Paragraph::scroll.y`; passing it makes the last
        // visual row visible at the bottom of the panel.
        let max_scroll = total_visual_rows.saturating_sub(inner_height);
        let global_scroll =
            resolve_global_scroll(app.agent_scroll, app.agent_auto_scroll, max_scroll);
        app.agent_scroll = (global_scroll.min(u16::MAX as usize)) as u16;

        (lines, app.agent_scroll)
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
mod scroll_tests {
    use super::*;

    // ---- resolve_global_scroll ----

    #[test]
    fn resolve_auto_scroll_pins_to_max() {
        // Auto-scroll ignores `agent_scroll` and returns `max_scroll`
        // so the bottom row is always visible while streaming.
        assert_eq!(resolve_global_scroll(0, true, 100), 100);
        assert_eq!(resolve_global_scroll(42, true, 100), 100);
        assert_eq!(resolve_global_scroll(999, true, 100), 100);
    }

    #[test]
    fn resolve_manual_scroll_clamps_to_max() {
        // Manual scroll respects `agent_scroll` but never lets it
        // exceed the real history end.
        assert_eq!(resolve_global_scroll(0, false, 100), 0);
        assert_eq!(resolve_global_scroll(50, false, 100), 50);
        assert_eq!(resolve_global_scroll(100, false, 100), 100);
        assert_eq!(resolve_global_scroll(101, false, 100), 100);
        assert_eq!(resolve_global_scroll(u16::MAX, false, 100), 100);
    }

    #[test]
    fn resolve_max_scroll_zero_always_returns_zero() {
        // Empty / shorter-than-viewport content: max is 0 and both
        // modes must land at 0.
        assert_eq!(resolve_global_scroll(0, true, 0), 0);
        assert_eq!(resolve_global_scroll(5, true, 0), 0);
        assert_eq!(resolve_global_scroll(0, false, 0), 0);
        assert_eq!(resolve_global_scroll(5, false, 0), 0);
    }

    // ---- Paragraph::line_count unit-symmetry regression ----
    //
    // Before the rewrite, the renderer stored pre-wrap `Line` counts
    // and computed `max_scroll = total - inner_height`. Wide lines
    // that wrapped to many visual rows would push `total` below
    // `inner_height` and `saturating_sub` clamped max_scroll to 0 —
    // locking the user out of the overflow. The rewrite uses
    // `Paragraph::line_count(width)` directly, which walks the same
    // `WordWrapper` the renderer uses, so the number is exact and
    // both operands of the subtract are in the same unit.
    //
    // These tests exercise `Paragraph::line_count` (the source of
    // truth in the new design) so a future change can't silently
    // switch back to a pre-wrap proxy.

    /// `Paragraph::line_count` on a 30-char line inside a 10-col
    /// viewport must be 3, not 1. A regression to `lines.len()`
    /// would shrink this to 1 and re-open the unit-mismatch bug.
    #[test]
    fn paragraph_line_count_wraps_wide_line() {
        let lines = vec![Line::from("a".repeat(30))];
        let p = Paragraph::new(lines).wrap(Wrap { trim: false });
        assert_eq!(p.line_count(10), 3);
    }

    /// Short, empty, and wrapping lines must sum: 1 + 1 + 3 = 5 at
    /// 10-col width. Pins that `line_count` is the visual-row total,
    /// not the source-Line count.
    #[test]
    fn paragraph_line_count_sums_mixed_lines() {
        let lines = vec![
            Line::from("ok"),
            Line::from(""),
            Line::from("z".repeat(25)),
        ];
        let p = Paragraph::new(lines).wrap(Wrap { trim: false });
        assert_eq!(p.line_count(10), 5);
    }

    /// The whole point of the rewrite: a single wide line that
    /// overflows the viewport must produce a *positive* max_scroll
    /// when subtracted from a 1-row viewport. The pre-fix formula
    /// gave 0; the new one gives `line_count − inner_height`.
    #[test]
    fn max_scroll_is_positive_for_overflowing_wide_line() {
        let lines = vec![Line::from("a".repeat(50))]; // 5 visual rows at width 10
        let p = Paragraph::new(lines).wrap(Wrap { trim: false });
        let total = p.line_count(10);
        let inner_height: usize = 1;
        let max_scroll = total.saturating_sub(inner_height);
        assert_eq!(max_scroll, 4);
    }

    /// Content that fits inside the viewport must yield max_scroll=0
    /// — `Paragraph` would otherwise skip rows that are all visible.
    #[test]
    fn max_scroll_is_zero_when_content_fits() {
        let lines = vec![Line::from("hello")];
        let p = Paragraph::new(lines).wrap(Wrap { trim: false });
        let total = p.line_count(80);
        let inner_height: usize = 24;
        assert_eq!(total.saturating_sub(inner_height), 0);
    }
}
