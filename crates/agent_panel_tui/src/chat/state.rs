//! Chat panel state: one independent message history per `K` key,
//! plus scroll, auto-scroll, and a two-level render cache.
//!
//! The cache is keyed on `message_version` for the flattened
//! `Vec<Line>` and on `(message_version, inner_width)` for the
//! post-wrap visual row count returned by
//! `Paragraph::line_count(width)`. The renderer owns these caches
//! during a frame; this struct just stores and exposes them.

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;

use ratatui::text::Line;

use super::ChatMessage;

/// Monotonic version counter. Bumped on every mutation that could
/// change the rendered output (push, replace, active-key change).
/// Both cache slots are keyed on this so a stable frame is a free
/// no-op.
pub type LinesCache = Option<(u64, Vec<Line<'static>>)>;

/// `(message_version, inner_width_u16, post_wrap_visual_row_count)`.
/// Re-keyed on width because wrap layout depends on viewport
/// width, so a terminal resize invalidates only this slot.
pub type WrapCache = Option<(u64, u16, usize)>;

/// Chat panel state, generic over the key type the host uses to
/// distinguish independent conversation histories.
///
/// `kms_tui` instantiates `ChatPanelState<AgentKind>` so the three
/// tabs (Compose / Knowledge / Parallel) keep separate histories;
/// another host might use a `String` channel id or a `usize` index.
///
/// The input text buffer and activation flag are **shared across
/// keys** (not per-key) so a Compose-tab draft is still visible
/// after switching to Knowledge — matching the legacy
/// `kms_tui::App::agent_input` behavior.
#[derive(Debug, Clone)]
pub struct ChatPanelState<K: Hash + Eq + Clone + Debug> {
    histories: HashMap<K, Vec<ChatMessage>>,
    active_key: K,

    /// Monotonic version counter. Bumped on every mutation.
    message_version: u64,

    /// Cached flattened history. Keyed on `message_version`.
    cached_lines: LinesCache,

    /// Cached post-wrap visual row count. Keyed on
    /// `(message_version, inner_width)`.
    cached_wrap: WrapCache,

    /// Vertical scroll offset in **post-wrap visual rows** (the
    /// unit `Paragraph::scroll.y` consumes). 0 = top.
    scroll: u16,

    /// When `true`, the renderer pins the scroll to the bottom of
    /// the history on every frame. Disabled the moment the user
    /// scrolls up; re-enabled on `End` or when starting a new
    /// submission.
    auto_scroll: bool,

    /// Current text in the input prompt. Owned here so the
    /// renderer can read it for the active prompt line and the
    /// cursor position. Shared across all `K` keys.
    input: String,

    /// `true` when the user is in input mode (the prompt shows
    /// `> {text}` with a highlighted background). Shared across
    /// all `K` keys.
    input_active: bool,

    /// Monotonic version counter for the input row. Bumped on
    /// any change to `input` or `input_active` (typing, backspace,
    /// esc, submit, activation toggle). The input renderer keys
    /// its `Line` cache on this so a stable frame is a free
    /// no-op.
    input_version: u64,
}

impl<K: Hash + Eq + Clone + Debug> ChatPanelState<K> {
    /// Build a new state with the given initial active key. Use
    /// [`insert_history`](Self::insert_history) to populate histories
    /// for each key.
    pub fn new(active_key: K) -> Self {
        Self {
            histories: HashMap::new(),
            active_key,
            message_version: 0,
            cached_lines: None,
            cached_wrap: None,
            scroll: 0,
            auto_scroll: true,
            input: String::new(),
            input_active: false,
            input_version: 0,
        }
    }

    /// Insert a history for `key`. Used at startup to seed each
    /// tab with a single `Divider` placeholder.
    pub fn insert_history(&mut self, key: K, initial: Vec<ChatMessage>) {
        self.histories.insert(key, initial);
    }

    /// Switch the active key. The next `current_messages` call
    /// returns the new key's history; the renderer will recompute
    /// the cache because `message_version` is bumped.
    pub fn set_active(&mut self, key: K) {
        if self.active_key != key {
            self.active_key = key;
            self.bump_version();
        }
    }

    /// Return the active key.
    pub fn active_key(&self) -> &K {
        &self.active_key
    }

    /// Borrow the current key's message history.
    pub fn current_messages(&self) -> &[ChatMessage] {
        // If a host forgets to `insert_history` for the active key
        // we still want a sensible empty slice rather than a panic.
        self.histories
            .get(&self.active_key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Mutably borrow the current key's message history. The
    /// caller is expected to bump `message_version` (via
    /// [`bump_version`](Self::bump_version) or
    /// [`push_message`](Self::push_message)) after mutating.
    pub fn current_messages_mut(&mut self) -> &mut Vec<ChatMessage> {
        self.histories
            .entry(self.active_key.clone())
            .or_insert_with(Vec::new)
    }

    /// Append a message to the current history and bump the
    /// version counter, invalidating the render caches.
    pub fn push_message(&mut self, msg: ChatMessage) {
        self.current_messages_mut().push(msg);
        self.bump_version();
    }

    /// Current version counter. The renderer compares this against
    /// its cached slot to decide whether to recompute.
    pub fn message_version(&self) -> u64 {
        self.message_version
    }

    /// Increment the version counter, invalidating all caches.
    pub fn bump_version(&mut self) {
        self.message_version = self.message_version.wrapping_add(1);
    }

    /// Borrow the flattened-line cache slot.
    pub fn cached_lines(&self) -> &LinesCache {
        &self.cached_lines
    }

    /// Replace the flattened-line cache slot.
    pub fn set_cached_lines(&mut self, cache: LinesCache) {
        self.cached_lines = cache;
    }

    /// Borrow the wrap-row-count cache slot.
    pub fn cached_wrap(&self) -> &WrapCache {
        &self.cached_wrap
    }

    /// Replace the wrap-row-count cache slot.
    pub fn set_cached_wrap(&mut self, cache: WrapCache) {
        self.cached_wrap = cache;
    }

    /// Current scroll offset in post-wrap visual rows.
    pub fn scroll(&self) -> u16 {
        self.scroll
    }

    /// Replace the scroll offset. The renderer writes back the
    /// clamped value after `resolve_scroll` so the next user
    /// scroll picks up from the bottom of the history, not from
    /// some stale position.
    pub fn set_scroll(&mut self, val: u16) {
        self.scroll = val;
    }

    /// Current auto-scroll flag.
    pub fn auto_scroll(&self) -> bool {
        self.auto_scroll
    }

    /// Replace the auto-scroll flag.
    pub fn set_auto_scroll(&mut self, val: bool) {
        self.auto_scroll = val;
    }

    /// Pin to the bottom. Also resets `scroll` to 0 so when the
    /// user later disables auto-scroll, manual scrolling picks
    /// up from the bottom, not a stale offset.
    pub fn enable_auto_scroll(&mut self) {
        self.auto_scroll = true;
        self.scroll = 0;
    }

    /// Disable auto-scroll. Manual `j`/`k` motions stop pinning
    /// to the bottom.
    pub fn disable_auto_scroll(&mut self) {
        self.auto_scroll = false;
    }

    /// Move the scroll position down by `amount` rows. Auto-scroll
    /// is disabled because the user explicitly asked to move.
    pub fn scroll_down(&mut self, amount: u16) {
        self.auto_scroll = false;
        self.scroll = self.scroll.saturating_add(amount);
    }

    /// Move the scroll position up by `amount` rows. Auto-scroll
    /// is disabled because the user explicitly asked to move.
    pub fn scroll_up(&mut self, amount: u16) {
        self.auto_scroll = false;
        self.scroll = self.scroll.saturating_sub(amount);
    }

    /// Jump to the top. Disables auto-scroll.
    pub fn scroll_to_top(&mut self) {
        self.auto_scroll = false;
        self.scroll = 0;
    }

    /// Compute the effective scroll position for this frame.
    /// `max_scroll` is the largest valid value the renderer can
    /// pass to `Paragraph::scroll.y`; it is the caller's job to
    /// compute it from `inner_height` and the wrapped row count.
    pub fn resolve_scroll(&self, max_scroll: usize) -> usize {
        if self.auto_scroll {
            max_scroll
        } else {
            (self.scroll as usize).min(max_scroll)
        }
    }

    // ---- Input state accessors ----
    //
    // The chat panel owns the input text buffer and the
    // input-mode flag. The host's key handler mutates these via
    // the typed accessors below; the renderer reads them through
    // `input_text()` (or borrows mutably for the cursor
    // positioning path) and `input_active()`.

    /// Borrow the current input text.
    pub fn input_text(&self) -> &str {
        &self.input
    }

    /// Mutably borrow the current input text. The caller is
    /// expected to bump `input_version` (via `push_input_char`,
    /// `pop_input_char`, etc.) after mutating.
    pub fn input_text_mut(&mut self) -> &mut String {
        &mut self.input
    }

    /// `true` when the user is in input mode (the prompt shows
    /// `> {text}` with a highlighted background).
    pub fn input_active(&self) -> bool {
        self.input_active
    }

    /// Set the input-mode flag. Bumps `input_version`.
    pub fn set_input_active(&mut self, v: bool) {
        if self.input_active != v {
            self.input_active = v;
            self.bump_input_version();
        }
    }

    /// Consume the input text, leaving the buffer empty. Used
    /// on Enter to hand the text to the submit pipeline. Bumps
    /// `input_version` so the cached `Line` is invalidated.
    pub fn take_input_text(&mut self) -> String {
        if self.input.is_empty() {
            return String::new();
        }
        let out = std::mem::take(&mut self.input);
        self.bump_input_version();
        out
    }

    /// Clear the input text but **keep** the input-mode flag.
    /// Used by Esc to wipe the buffer without leaving input mode.
    /// (The legacy code used Esc to also deactivate, but the
    /// host's key handler is responsible for the deactivation;
    /// this accessor is the building block for "clear text
    /// only".)
    pub fn clear_input_text(&mut self) {
        if !self.input.is_empty() {
            self.input.clear();
            self.bump_input_version();
        }
    }

    /// Push a single typed character. Bumps `input_version`.
    pub fn push_input_char(&mut self, c: char) {
        self.input.push(c);
        self.bump_input_version();
    }

    /// Pop the last typed character. No-op if the input is
    /// empty. Bumps `input_version` when it actually pops.
    pub fn pop_input_char(&mut self) {
        if self.input.pop().is_some() {
            self.bump_input_version();
        }
    }

    /// Append a string slice (used by paste handling).
    pub fn push_input_str(&mut self, s: &str) {
        if !s.is_empty() {
            self.input.push_str(s);
            self.bump_input_version();
        }
    }

    /// Current input version. The renderer compares this against
    /// its cached slot to decide whether to recompute the
    /// `Line`.
    pub fn input_version(&self) -> u64 {
        self.input_version
    }

    /// Bump the input version counter, invalidating the
    /// input-row `Line` cache.
    fn bump_input_version(&mut self) {
        self.input_version = self.input_version.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Hash, Eq, PartialEq, Clone, Debug)]
    enum Tab {
        A,
        B,
    }

    fn new_state() -> ChatPanelState<Tab> {
        let mut s = ChatPanelState::new(Tab::A);
        s.insert_history(Tab::A, vec![ChatMessage::Divider]);
        s.insert_history(Tab::B, vec![ChatMessage::Divider]);
        s
    }

    #[test]
    fn new_state_has_auto_scroll_enabled() {
        let s = ChatPanelState::new(Tab::A);
        assert!(s.auto_scroll());
        assert_eq!(s.scroll(), 0);
    }

    #[test]
    fn push_message_bumps_version() {
        let mut s = new_state();
        let v0 = s.message_version();
        s.push_message(ChatMessage::User { text: "hi".into() });
        assert_ne!(s.message_version(), v0);
    }

    #[test]
    fn set_active_bumps_version() {
        let mut s = new_state();
        let v0 = s.message_version();
        s.set_active(Tab::B);
        assert_ne!(s.message_version(), v0);
    }

    #[test]
    fn set_active_to_same_key_does_not_bump() {
        let mut s = new_state();
        let v0 = s.message_version();
        s.set_active(Tab::A);
        assert_eq!(s.message_version(), v0);
    }

    #[test]
    fn enable_auto_scroll_resets_scroll() {
        let mut s = new_state();
        s.set_scroll(42);
        s.enable_auto_scroll();
        assert!(s.auto_scroll());
        assert_eq!(s.scroll(), 0);
    }

    #[test]
    fn scroll_down_disables_auto_scroll() {
        let mut s = new_state();
        s.scroll_down(5);
        assert!(!s.auto_scroll());
        assert_eq!(s.scroll(), 5);
    }

    #[test]
    fn scroll_up_clamps_to_zero() {
        let mut s = new_state();
        s.scroll_up(3);
        assert_eq!(s.scroll(), 0);
    }

    #[test]
    fn resolve_scroll_pins_to_max_when_auto() {
        let mut s = new_state();
        s.enable_auto_scroll();
        assert_eq!(s.resolve_scroll(100), 100);
    }

    #[test]
    fn resolve_scroll_clamps_manual() {
        let mut s = new_state();
        s.disable_auto_scroll();
        s.set_scroll(50);
        assert_eq!(s.resolve_scroll(10), 10);
    }

    #[test]
    fn resolve_scroll_preserves_manual_within_bounds() {
        let mut s = new_state();
        s.disable_auto_scroll();
        s.set_scroll(7);
        assert_eq!(s.resolve_scroll(100), 7);
    }

    #[test]
    fn histories_are_isolated_per_key() {
        let mut s = new_state();
        s.push_message(ChatMessage::User { text: "from a".into() });
        s.set_active(Tab::B);
        assert_eq!(s.current_messages().len(), 1);
        assert!(matches!(s.current_messages()[0], ChatMessage::Divider));
    }
}
