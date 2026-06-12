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
//! 5. Render a vertical `Scrollbar` on the right edge of the
//!    panel so the user can see scroll position and click-drag
//!    to navigate (mouse drag is handled by the host).
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
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};
use ratatui::Frame;

use super::state::ChatPanelState;
use super::theme::ChatPanelTheme;

/// Width of the scrollbar strip reserved on the right edge of
/// the chat panel. One cell is enough for ratatui's
/// `ScrollbarOrientation::VerticalRight` to draw its track and
/// thumb; 0 would let the scrollbar overlay the rightmost
/// character of the chat text.
pub const SCROLLBAR_WIDTH: u16 = 1;

/// Render the chat conversation into `area`, with a vertical
/// scrollbar on the right edge.
///
/// `area` is the **inner** rect — the area *inside* any
/// surrounding `Block` border. The host owns framing (the
/// `Block` with the panel title and border style); the renderer
/// just lays out the `Paragraph` and walks the cache. This
/// matches the contract of `render_agent_panel` for the
/// sub-agent list.
///
/// The scrollbar's `content_length` is the post-wrap visual
/// row count (the same `total_visual_rows` that drives
/// `Paragraph::scroll.y`); the `position` is the resolved
/// scroll offset. Mouse wheel events on the host side should
/// route to [`ChatPanelState::scroll_up`] /
/// [`ChatPanelState::scroll_down`] (or the `set_scroll` family
/// of methods) to keep the scrollbar in sync.
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

    // ---- 4. Reserve a strip on the right for the scrollbar. ----
    // We only need a scrollbar when there's actually content
    // that overflows the viewport. A scrollbar with
    // `content_length <= viewport_content_length` would be a
    // full-height thumb, which is visually misleading
    // ("there's nothing to scroll"). So when the history fits,
    // skip both the strip reservation and the scrollbar
    // render — the chat text gets the full width.
    let (paragraph_area, scrollbar_area) = if total_visual_rows > inner_height
        && inner_width > SCROLLBAR_WIDTH
    {
        let sb_width = SCROLLBAR_WIDTH;
        let paragraph_rect = Rect {
            x: area.x,
            y: area.y,
            width: area.width.saturating_sub(sb_width),
            height: area.height,
        };
        let scrollbar_rect = Rect {
            x: area.x + area.width.saturating_sub(sb_width),
            y: area.y,
            width: sb_width,
            height: area.height,
        };
        (paragraph_rect, Some(scrollbar_rect))
    } else {
        (area, None)
    };

    // ---- 5. Render the Paragraph. ----
    //
    // We pass the *narrower* `paragraph_area` so the chat text
    // doesn't visually overlap the scrollbar. To keep the
    // cached `total_visual_rows` and the scrollbar in
    // lockstep with what the user actually sees, we recompute
    // the row count against the *narrowed* width when the
    // scrollbar is shown. This is cheap (one
    // `Paragraph::line_count` call) and only runs when the
    // content actually overflows — a no-op for short chats.
    let total_rows_for_sb = if scrollbar_area.is_some() {
        let narrowed = paragraph_area.width;
        if narrowed != inner_width {
            let probe = Paragraph::new(lines.clone()).wrap(Wrap { trim: false });
            probe.line_count(narrowed)
        } else {
            total_visual_rows
        }
    } else {
        total_visual_rows
    };

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((global_scroll.min(u16::MAX as usize) as u16, 0));
    f.render_widget(paragraph, paragraph_area);

    // ---- 6. Render the vertical scrollbar. ----
    //
    // The scrollbar uses the *narrowed* row count so the
    // thumb's vertical extent matches what the user sees in
    // the Paragraph above. The position is the current scroll
    // offset (already in post-wrap visual rows). The viewport
    // length is the visible row count, which is the same as
    // the paragraph's height.
    if let Some(sb_area) = scrollbar_area {
        let mut sb_state = ScrollbarState::new(total_rows_for_sb)
            .position(global_scroll.min(usize::from(u16::MAX)) as usize)
            .viewport_content_length(sb_area.height as usize);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some(" "))
            .thumb_symbol("█")
            .thumb_style(theme.scrollbar_thumb_style())
            .track_style(theme.scrollbar_track_style());
        f.render_stateful_widget(scrollbar, sb_area, &mut sb_state);
    }
}

