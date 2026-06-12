//! [`render_chat_panel`] — the chat panel's public render entry point.
//!
//! Encapsulates the full pipeline that was previously inline in
//! `kms_tui::components::agent::render_agent`:
//! 1. Flatten the current history into `Vec<Line<'static>>`.
//! 2. Compute the exact post-wrap visual row count via a probe
//!    `Paragraph`.
//! 3. Resolve the scroll position (auto-scroll pins to bottom,
//!    manual is clamped to `max_scroll`).
//! 4. Render the `Paragraph` with wrap and scroll.
//!
//! Both render results are memoized in [`ChatPanelState`]
//! (`cached_lines`, `cached_wrap`) keyed on
//! `message_version` (and width for the wrap count) so a stable
//! frame is a no-op.
//!
//! The function draws a bare `Paragraph` with **no** `Block` /
//! border — the host owns framing. This matches the contract of
//! `render_agent_panel` for the sub-agent list.

use std::fmt::Debug;
use std::hash::Hash;

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use super::state::ChatPanelState;
use super::theme::ChatPanelTheme;

/// Render the chat conversation into `area`.
///
/// `area` is the **inner** rect — the area *inside* any
/// surrounding `Block` border. The host owns framing (the
/// `Block` with the panel title and border style); the renderer
/// just lays out the `Paragraph` and walks the cache. This
/// matches the contract of `render_agent_panel` for the
/// sub-agent list.
///
/// After this returns, `state.scroll` reflects the resolved
/// post-wrap scroll position (clamped to the actual history
/// bottom) and the cache slots are populated for the next
/// frame.
pub fn render_chat_panel<K: Hash + Eq + Clone + Debug>(
    f: &mut Frame,
    state: &mut ChatPanelState<K>,
    theme: &dyn ChatPanelTheme,
    area: Rect,
) {
    let inner_height = area.height as usize;
    let inner_width = area.width;

    // ---- 1. Flatten the current history into Vec<Line>. ----
    let msg_version = state.message_version();
    let cache_hit = matches!(
        state.cached_lines(),
        Some((ver, _lines)) if *ver == msg_version
    );
    let lines: Vec<Line<'static>> = if cache_hit {
        // SAFETY: we just checked Some above.
        state.cached_lines().as_ref().unwrap().1.clone()
    } else {
        let lines: Vec<Line<'static>> = state
            .current_messages()
            .iter()
            .flat_map(|m| m.to_lines(theme))
            .collect();
        state.set_cached_lines(Some((msg_version, lines.clone())));
        lines
    };

    // ---- 2. Get the exact post-wrap visual row count. ----
    // Cached on (version, width) so a stable frame skips the
    // second WordWrapper pass.
    let total_visual_rows: usize = match state.cached_wrap() {
        Some((ver, w, rows)) if *ver == msg_version && *w == inner_width => *rows,
        _ => {
            let probe = Paragraph::new(lines.clone()).wrap(Wrap { trim: false });
            let rows = probe.line_count(inner_width);
            state.set_cached_wrap(Some((msg_version, inner_width, rows)));
            rows
        }
    };

    // ---- 3. Resolve the effective scroll position. ----
    // `max_scroll` is in post-wrap visual rows (Paragraph's
    // native unit), so passing it makes the last visual row
    // visible at the bottom of the panel. Write the clamped
    // value back so the next manual scroll picks up from the
    // bottom, not from some stale offset.
    let max_scroll = total_visual_rows.saturating_sub(inner_height);
    let global_scroll = state.resolve_scroll(max_scroll);
    state.set_scroll((global_scroll.min(u16::MAX as usize)) as u16);

    // ---- 4. Render the Paragraph. ----
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((global_scroll.min(u16::MAX as usize) as u16, 0));
    f.render_widget(paragraph, area);
}
