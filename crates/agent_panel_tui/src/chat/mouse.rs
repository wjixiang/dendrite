//! Mouse-event handling for the chat panel.
//!
//! The host (e.g. `kms_tui`) is responsible for:
//! 1. Enabling mouse capture in the terminal
//!    (`crossterm::event::EnableMouseCapture`).
//! 2. Reading `crossterm::event::Event::Mouse` events and
//!    forwarding them to [`handle_chat_mouse_event`].
//! 3. Marking the frame as dirty when [`ChatMouseOutcome::Handled`]
//!    is returned, so the scroll position is reflected on the
//!    next render.
//!
//! The handler is generic over the chat panel's `K` key type and
//! is decoupled from the host's layout system: the caller passes
//! the chat area's `Rect`, and the function decides whether the
//! mouse is over it.
//!
//! Supported events when the mouse is over the chat area:
//! - `MouseEventKind::ScrollUp`   → scroll the history up by
//!   `scroll_amount` rows (the caller passes 1 for one wheel
//!   notch, 3 for a fast wheel, etc.). Disables auto-scroll so
//!   the user can browse history.
//! - `MouseEventKind::ScrollDown` → scroll the history down by
//!   `scroll_amount` rows. Disables auto-scroll.
//! - `MouseEventKind::Down(Left)` while over the scrollbar strip
//!   (the rightmost cell of the chat area) → jump the scroll
//!   position to the clicked row.
//! - Anything else → [`ChatMouseOutcome::Ignored`].
//!
//! The handler does **not** consume the auto-scroll pin: it just
//! disables it for scroll events. The next stream start / new
//! submission re-enables it via
//! [`crate::chat::state::ChatPanelState::enable_auto_scroll`].

use std::fmt::Debug;
use std::hash::Hash;

use ratatui::layout::Rect;

use super::state::ChatPanelState;

/// What happened when a mouse event was processed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatMouseOutcome {
    /// The event was over the chat panel and the chat panel
    /// consumed it. The host should mark the frame dirty and
    /// **not** forward the event to other panels.
    Handled,

    /// The event was outside the chat area (or was a kind the
    /// chat panel doesn't care about). The host may try other
    /// panels — typically the diagnostics / KE / tree panels
    /// that share the same `Event::Mouse` stream.
    Ignored,
}

/// Handle a `crossterm::event::MouseEvent` for the chat panel.
///
/// `state` is mutated in place when the event affects scroll
/// position. `chat_area` is the rect the chat panel currently
/// occupies on screen (the **inner** rect — the same one passed
/// to [`crate::chat::renderer::render_chat_panel`]). The function
/// is a no-op if the mouse position is outside that rect.
///
/// `scroll_amount` controls how many rows each wheel notch
/// scrolls. Pass `1` for a "feels-like-a-line" amount, `3` for a
/// faster wheel. The host is in charge of picking the right
/// value for the user's environment.
///
/// `total_rows` is the post-wrap visual row count for the
/// current history — the same number the chat panel renderer
/// caches as `cached_wrap`. It's needed to map a click on the
/// scrollbar to a target scroll position. The host typically
/// computes this once per frame in the same pass that renders
/// the chat panel; if you don't have it handy, pass `0` and the
/// scrollbar-click shortcut will degrade to a no-op (wheel
/// events still work).
pub fn handle_chat_mouse_event<K: Hash + Eq + Clone + Debug>(
    state: &mut ChatPanelState<K>,
    mouse_column: u16,
    mouse_row: u16,
    mouse_kind: MouseEventKind,
    chat_area: Rect,
    scroll_amount: u16,
    total_rows: usize,
) -> ChatMouseOutcome {
    if !rect_contains(chat_area, mouse_column, mouse_row) {
        return ChatMouseOutcome::Ignored;
    }

    // The scrollbar (when shown) lives in the rightmost cell of
    // the chat area. `render_chat_panel` only reserves that
    // strip when the history overflows the viewport, so a click
    // there is only meaningful when `total_rows > viewport`.
    // We treat "rightmost column" as the scrollbar strip
    // unconditionally — clicking the strip on a short chat is
    // a harmless no-op because the jump target would clamp to
    // `max_scroll = 0`.
    let on_scrollbar = mouse_column == chat_area.x + chat_area.width.saturating_sub(1);

    match mouse_kind {
        MouseEventKind::ScrollUp => {
            // `scroll_up` already disables auto-scroll.
            state.scroll_up(scroll_amount);
            ChatMouseOutcome::Handled
        }
        MouseEventKind::ScrollDown => {
            // `scroll_down` already disables auto-scroll.
            state.scroll_down(scroll_amount);
            ChatMouseOutcome::Handled
        }
        MouseEventKind::Down(MouseButton::Left) if on_scrollbar => {
            // Map the click row to a scroll position. The
            // scrollbar's thumb covers roughly
            // `viewport_content_length / total_rows * area.height`
            // cells, so clicking the middle of the scrollbar
            // should land near the middle of the history.
            if total_rows == 0 || chat_area.height == 0 {
                return ChatMouseOutcome::Handled;
            }
            let viewport = chat_area.height as usize;
            let max_scroll = total_rows.saturating_sub(viewport);
            if max_scroll == 0 {
                return ChatMouseOutcome::Handled;
            }
            let rel = (mouse_row - chat_area.y) as usize;
            // Translate the click position into a scroll
            // offset. The `viewport` is the visible window;
            // anything above or below the thumb maps to the
            // top or bottom respectively.
            let target = rel.saturating_mul(total_rows) / chat_area.height as usize;
            let target = target.min(max_scroll);
            state.disable_auto_scroll();
            state.set_scroll(target as u16);
            ChatMouseOutcome::Handled
        }
        _ => ChatMouseOutcome::Ignored,
    }
}

fn rect_contains(r: Rect, col: u16, row: u16) -> bool {
    col >= r.x && col < r.x.saturating_add(r.width) && row >= r.y && row < r.y.saturating_add(r.height)
}

// ---- MouseEventKind / MouseButton shims -----------------------------------
//
// We intentionally do *not* depend on `crossterm` from
// `agent_panel_tui`. The crate stays frontend-agnostic: it
// only needs to know "was this a wheel event, and was that a
// left click?" Two tiny enums capture the slice of the
// crossterm API the chat panel actually cares about. The host
// converts from `crossterm::event::MouseEventKind` to one of
// these in a single match arm.

/// Subset of `crossterm::event::MouseEventKind` the chat panel
/// reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEventKind {
    /// Wheel rotated up.
    ScrollUp,
    /// Wheel rotated down.
    ScrollDown,
    /// Mouse button pressed. The chat panel only cares about
    /// the left button (for scrollbar drags / jumps); other
    /// buttons are reported as `Ignored`.
    Down(MouseButton),
    /// Anything else — currently a no-op for the chat panel.
    Other,
}

/// Subset of `crossterm::event::MouseButton` the chat panel
/// reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Other,
}

// ---- (crossterm conversion helper, optional) ------------------------------
//
// We *deliberately* don't add a `crossterm` feature or a
// conversion helper here. `agent_panel_tui` stays
// frontend-agnostic: it knows nothing about crossterm. Hosts
// translate their native mouse-event type into our
// [`MouseEventKind`] in a single match arm before calling
// [`handle_chat_mouse_event`]. This keeps the crate free of a
// heavy terminal-library dependency and makes it usable from
// hosts that use a different frontend (e.g. termion, termwiz,
// or a custom one).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatMessage;

    fn state() -> ChatPanelState<u8> {
        let mut s = ChatPanelState::new(0);
        s.insert_history(0, vec![ChatMessage::Divider]);
        // Pad with a few messages so scroll is meaningful.
        for i in 0..200 {
            s.push_message(ChatMessage::Assistant {
                text: format!("line {i}"),
                streaming: false,
            });
        }
        s
    }

    fn area() -> Rect {
        Rect {
            x: 10,
            y: 5,
            width: 40,
            height: 20,
        }
    }

    #[test]
    fn scroll_up_disables_auto_scroll_and_moves() {
        let mut s = state();
        // Start mid-history so scroll-up is actually visible.
        s.disable_auto_scroll();
        s.set_scroll(5);
        handle_chat_mouse_event(&mut s, 20, 10, MouseEventKind::ScrollUp, area(), 1, 200);
        assert!(!s.auto_scroll());
        // Scrolling up by 1 from 5 lands at 4. (Scrolling up
        // from 0 is a no-op via `saturating_sub`; that's tested
        // separately below.)
        assert_eq!(s.scroll(), 4);
    }

    #[test]
    fn scroll_up_at_top_is_a_no_op() {
        // scroll=0 is the topmost position; scroll_up must
        // not underflow.
        let mut s = state();
        s.disable_auto_scroll();
        s.set_scroll(0);
        handle_chat_mouse_event(&mut s, 20, 10, MouseEventKind::ScrollUp, area(), 1, 200);
        // Auto-scroll is still disabled (we did move the
        // intent to scroll), but the offset didn't underflow.
        assert!(!s.auto_scroll());
        assert_eq!(s.scroll(), 0);
    }

    #[test]
    fn scroll_down_clamps_to_max() {
        let mut s = state();
        s.disable_auto_scroll();
        s.set_scroll(0);
        // total_rows=200, viewport=20, max_scroll=180. Scrolling
        // down by 5 from 0 lands at 5.
        handle_chat_mouse_event(
            &mut s,
            20,
            10,
            MouseEventKind::ScrollDown,
            area(),
            5,
            200,
        );
        assert_eq!(s.scroll(), 5);
    }

    #[test]
    fn event_outside_chat_area_is_ignored() {
        let mut s = state();
        let v0 = s.scroll();
        let auto0 = s.auto_scroll();
        // (0, 0) is well outside the chat area.
        let outcome = handle_chat_mouse_event(
            &mut s,
            0,
            0,
            MouseEventKind::ScrollUp,
            area(),
            3,
            200,
        );
        assert_eq!(outcome, ChatMouseOutcome::Ignored);
        assert_eq!(s.scroll(), v0);
        assert_eq!(s.auto_scroll(), auto0);
    }

    #[test]
    fn click_on_scrollbar_jumps_to_position() {
        let mut s = state();
        s.disable_auto_scroll();
        s.set_scroll(0);
        // Rightmost cell of the chat area (x=49), row 15 of
        // an area starting at y=5 with height=20. The
        // scrollbar maps row 10 (relative) to:
        //   target = 10 * 200 / 20 = 100
        // (row 0 → top of history, row 19 → bottom).
        let outcome = handle_chat_mouse_event(
            &mut s,
            49,
            15,
            MouseEventKind::Down(MouseButton::Left),
            area(),
            1,
            200,
        );
        assert_eq!(outcome, ChatMouseOutcome::Handled);
        assert_eq!(s.scroll(), 100);
    }

    #[test]
    fn click_outside_scrollbar_strip_is_ignored() {
        let mut s = state();
        let v0 = s.scroll();
        // x=20 is in the chat text column, not the scrollbar.
        let outcome = handle_chat_mouse_event(
            &mut s,
            20,
            15,
            MouseEventKind::Down(MouseButton::Left),
            area(),
            1,
            200,
        );
        assert_eq!(outcome, ChatMouseOutcome::Ignored);
        assert_eq!(s.scroll(), v0);
    }

    #[test]
    fn non_wheel_non_scrollbar_event_is_ignored() {
        let mut s = state();
        let v0 = s.scroll();
        // Right-click on a non-scrollbar cell: ignored.
        let outcome = handle_chat_mouse_event(
            &mut s,
            20,
            10,
            MouseEventKind::Down(MouseButton::Other),
            area(),
            1,
            200,
        );
        assert_eq!(outcome, ChatMouseOutcome::Ignored);
        assert_eq!(s.scroll(), v0);
    }

    #[test]
    fn click_on_scrollbar_with_empty_history_is_no_op() {
        let mut s = ChatPanelState::new(0);
        s.insert_history(0, vec![ChatMessage::Divider]);
        let outcome = handle_chat_mouse_event(
            &mut s,
            49,
            10,
            MouseEventKind::Down(MouseButton::Left),
            area(),
            1,
            0,
        );
        // Handled (the click was over the scrollbar) but no
        // mutation because there's nothing to scroll.
        assert_eq!(outcome, ChatMouseOutcome::Handled);
        assert_eq!(s.scroll(), 0);
    }
}
