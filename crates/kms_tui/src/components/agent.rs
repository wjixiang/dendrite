use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::agent_panel;
use crate::state::{App, ChatFocus, Panel};
use crate::theme::Theme;
use crate::widgets::SPINNER_FRAMES;

/// Maximum height (in visual rows) the embedded sub-agent list can
/// grow to. The list auto-shrinks to fit, but this caps it so a
/// chat panel that's the only thing on screen still leaves a
/// reasonable amount of room for the conversation. The minimum is
/// 3 rows (header + one agent row + hint) so even one running
/// sub-agent doesn't look squashed.
const SUB_AGENT_PANEL_MAX_HEIGHT: u16 = 10;
const SUB_AGENT_PANEL_MIN_HEIGHT: u16 = 3;

/// How many visual rows the embedded sub-agent list should occupy
/// given `agent_count` sub-agents and a `panel_height` for the
/// whole Agent panel. Each visible row uses 1 line, plus an extra
/// line for the header and a "… +N more" footer when the list is
/// longer than fits. We also reserve the bottom 1-row status bar
/// and (when shown) the 1-row divider above the list.
fn sub_agent_height_for(agent_count: usize, panel_height: u16) -> u16 {
    // Account for the fixed siblings: 1 status bar + 1 divider
    // (when the sub-list is shown). Add 1 for the list header
    // (the "Agents · N total …" line) and 1 for the "… +N more"
    // footer when the list is long enough to need one.
    let status_bar = 1u16;
    let divider = 1u16;
    let header = 1u16;
    let footer = if agent_count > 3 { 1u16 } else { 0u16 };

    // 1 row per agent (the row itself; running agents also get a
    // peek-line below, but we don't try to budget for that — the
    // Paragraph inside the sub-list handles overflow by clipping
    // at the bottom, which is the same behavior as before).
    let want = header + agent_count as u16 + footer + divider + status_bar;

    // Respect both the global min/max for the sub-section and the
    // hard cap of leaving at least 3 rows for the chat above.
    let max_for_sub = SUB_AGENT_PANEL_MAX_HEIGHT.min(panel_height.saturating_sub(3));
    let bounded_max = max_for_sub.max(SUB_AGENT_PANEL_MIN_HEIGHT);

    want.clamp(SUB_AGENT_PANEL_MIN_HEIGHT, bounded_max.max(SUB_AGENT_PANEL_MIN_HEIGHT))
}

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
    // The Agent panel is now a single bordered group that contains
    // up to three sub-sections stacked vertically:
    //   1. Chat messages (the conversation history) — always shown.
    //   2. Sub-agent status list — embedded inside the same border,
    //      shown when at least one sub-agent is registered.
    //   3. Status / input bar (spinner, phase, prompt) — always shown.
    //
    // We split the area *before* drawing the border so the inner
    // sections sit cleanly under one outer frame.
    let sub_agent_count = app.agent_panel.agents.len();
    let show_sub_agents = sub_agent_count > 0;

    let chunks = if show_sub_agents {
        // The sub-agent section is bounded by MIN/MAX so it never
        // eats the whole chat. The exact split is computed from the
        // number of agents we want to show (capped by what fits).
        let target_h = sub_agent_height_for(sub_agent_count, area.height);
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(target_h),
                Constraint::Length(1),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area)
    };

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

    // Section rects: [messages, sub_agents?, status_bar]
    let (messages_area, sub_area, status_area) = if show_sub_agents {
        (chunks[0], Some(chunks[1]), chunks[2])
    } else {
        (chunks[0], None, chunks[1])
    };

    let inner_height = messages_area.height.saturating_sub(2) as usize;
    let inner_width = messages_area.width.saturating_sub(2) as u16;

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
    f.render_widget(conv, messages_area);

    // --- Embedded sub-agent list ---------------------------------------
    //
    // Rendered inside the same outer border as the chat, on its own
    // dedicated row. A horizontal divider line is drawn at the top
    // of the sub-area so the user can tell the two sections apart.
    if let Some(sub_rect) = sub_area {
        if sub_rect.height >= 1 {
            // Divider: draw a horizontal rule using a muted line that
            // spans the panel width. The block that owns the right
            // border is the chat's, so we just draw an unsized
            // Paragraph with a single line that visually separates
            // the regions.
            if sub_rect.height >= 2 {
                let divider = Paragraph::new(Line::from(Span::styled(
                    "\u{2500}".repeat(sub_rect.width as usize),
                    Style::default().fg(theme.text_muted),
                )));
                f.render_widget(divider, Rect {
                    x: sub_rect.x,
                    y: sub_rect.y,
                    width: sub_rect.width,
                    height: 1,
                });

                // Inner area for the actual list rows, below the divider.
                let list_rect = Rect {
                    x: sub_rect.x,
                    y: sub_rect.y + 1,
                    width: sub_rect.width,
                    height: sub_rect.height.saturating_sub(1),
                };

                // Subtle highlight border around the sub-agent list
                // when it's the active sub-focus, so the user can
                // see where their j/k will land.
                let sub_focused = app.focused == Panel::Agent
                    && app.chat_focus == ChatFocus::AgentsPanel;
                if list_rect.height >= 1 {
                    let sub_block = Block::default()
                        .borders(Borders::LEFT | Borders::RIGHT)
                        .border_style(theme.focused_border_style(sub_focused));
                    let inner = sub_block.inner(list_rect);
                    f.render_widget(sub_block, list_rect);
                    agent_panel::render_agent_panel(
                        f,
                        &app.agent_panel,
                        theme,
                        inner,
                        app.spinner_tick,
                    );
                }
            } else {
                // No divider possible: just use the full 1-row sub-area
                // for the list (it'll get truncated, but at least the
                // header is visible).
                agent_panel::render_agent_panel(
                    f,
                    &app.agent_panel,
                    theme,
                    sub_rect,
                    app.spinner_tick,
                );
            }
        }
    }

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
        // Phase label: the spinner keeps rotating across all three
        // (driven by `agent_running` in input.rs), but the suffix
        // tells the user *which* phase is active right now.
        let phase = if app.agent_streaming {
            "streaming"
        } else if app.agent_requesting {
            "requesting"
        } else {
            "running"
        };
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(spinner_char.to_string(), Style::default().fg(theme.spinner)),
            Span::styled(
                format!(" Agent [{}] {}{} ", kind_label, phase, usage_suffix),
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
    f.render_widget(input_line, status_area);

    if app.agent_input_active && app.focused == Panel::Agent && !app.providers.is_empty() {
        let cursor_x = status_area.x + 2 + app.agent_input.len() as u16;
        let cursor_y = status_area.y;
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
