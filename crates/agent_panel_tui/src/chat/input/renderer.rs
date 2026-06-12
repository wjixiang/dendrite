//! [`render_chat_input`] — frame-side renderer for the chat
//! input / status row.
//!
//! The pure line-builder lives in [`super::build_status_line`].
//! This file owns everything that touches a `Frame` (rendering
//! the `Paragraph`, setting the cursor position).

use std::fmt::Debug;
use std::hash::Hash;

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::{build_status_line, ChatInputStatus, ChatInputTheme};
use crate::chat::state::ChatPanelState;

/// Render the chat input / status row into `area`.
///
/// `state` is borrowed so the renderer can read the input text
/// (for the `InputActive` line and the cursor position) and the
/// `input_version` counter (for the `Line` cache).
///
/// `status` carries the host's runtime state (providers empty,
/// agent running with phase/tokens, input active, or idle).
/// `kind_label` is the human-readable label of the active
/// conversation tab (e.g. "Compose", "Retrieval", "Parallel").
/// `spinner_tick` is the host's monotonic counter; the renderer
/// picks `SPINNER_FRAMES[spinner_tick % 8]` for the running
/// state and ignores it otherwise.
///
/// `focused` should be `true` when the host's Agent panel is the
/// focused top-level panel AND the input is eligible for the
/// cursor (i.e. providers are configured). The renderer still
/// guards on `status == InputActive` internally before
/// positioning the cursor — a non-`InputActive` status is a
/// no-op even when `focused` is true.
///
/// `area` is the **inner** rect (no border drawn around the
/// status row). The host owns any framing.
pub fn render_chat_input<K: Hash + Eq + Clone + Debug>(
    f: &mut Frame,
    state: &mut ChatPanelState<K>,
    status: &ChatInputStatus,
    kind_label: &str,
    spinner_tick: usize,
    theme: &dyn ChatInputTheme,
    area: Rect,
    focused: bool,
) {
    // ---- 1. Build the `Line` for this status. ----
    //
    // The build itself is cheap (≤4 spans; static states use
    // `&'static str`, only the dynamic `format!` cases
    // allocate). We don't cache the result on the state
    // because `ChatPanelState` derives `Clone + Debug` and
    // adding a cached `Line<'static>` would force non-default
    // derives. The host's render loop already short-circuits
    // when nothing changed (via `needs_render`), so the build
    // is skipped on stable frames.
    //
    // We DO capture the line in a binding so the cursor-width
    // computation below can reuse it instead of rebuilding
    // the same `Line` a second time.
    let line: Line<'static> = build_status_line(
        status,
        state.input_text(),
        kind_label,
        spinner_tick,
        theme,
    );

    // ---- 2. Determine the background color. ----
    // Only the `InputActive` state gets the highlighted
    // background; everything else is inline.
    let bg = if matches!(status, ChatInputStatus::InputActive) {
        theme.input_bg()
    } else {
        Color::Reset
    };

    // ---- 3. Render the Paragraph. ----
    let paragraph = Paragraph::new(line.clone()).style(Style::default().bg(bg));
    f.render_widget(paragraph, area);

    // ---- 4. Position the cursor when active + focused. ----
    //
    // The cursor column is computed from the *display width* of
    // the `Line` we just rendered, NOT from the byte length of
    // the input string. Byte length is wrong for any non-ASCII
    // input: each CJK character is 3 bytes in UTF-8 but renders
    // as 2 terminal cells, so byte-based positioning places
    // the cursor 1 cell too far right per CJK char. `Line::width()`
    // walks each `Span` and sums its `unicode-width` (the same
    // metric ratatui uses for wrap calculation), so the cursor
    // lands exactly where the next character would render.
    //
    // We only set the cursor when the chat panel is the
    // focused top-level panel; otherwise the user's typing
    // focus is elsewhere.
    if focused && matches!(status, ChatInputStatus::InputActive) {
        let cursor_x = area.x.saturating_add(line.width() as u16);
        let cursor_y = area.y;
        f.set_cursor_position((cursor_x, cursor_y));
    }
}
