//! Theme trait for the chat input / status row.
//!
//! Deliberately separate from [`crate::chat::theme::ChatPanelTheme`]:
//! the message-history renderer doesn't need `input_bg`, and the
//! input row doesn't need chat-message colors. Hosts that already
//! have a `Theme` type typically implement both traits on the same
//! bridge struct (see `kms_tui::state::KmsChatThemeBridge`).

use ratatui::style::Color;

/// Colors the chat input renderer reads while laying out the
/// bottom status / prompt row.
///
/// Every method is one the renderer genuinely reaches for. Keep
/// this surface narrow on purpose.
pub trait ChatInputTheme {
    /// Foreground for the idle hint and the empty-pool hint.
    fn text_muted(&self) -> Color;

    /// Foreground for the spinner character and the running-phase
    /// label.
    fn spinner_color(&self) -> Color;

    /// Background for the active input line. Hosts that want the
    /// active input to *stand out* use a contrasting color
    /// (e.g. `DarkGray`); hosts that want it to feel inline use
    /// `Color::Reset`.
    fn input_bg(&self) -> Color;
}

/// Default implementation carrying values that match the original
/// `kms_tui::Theme::default_theme()` color palette.
///
/// Intentionally not re-exported from the crate root — hosts reach
/// it via `agent_panel_tui::chat::input::theme::DefaultChatInputTheme`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultChatInputTheme;

impl ChatInputTheme for DefaultChatInputTheme {
    fn text_muted(&self) -> Color {
        Color::DarkGray
    }
    fn spinner_color(&self) -> Color {
        Color::Yellow
    }
    fn input_bg(&self) -> Color {
        Color::DarkGray
    }
}
