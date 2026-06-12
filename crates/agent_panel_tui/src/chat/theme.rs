//! Theme trait for the chat panel.
//!
//! Hosts that already have a `Theme` type implement [`ChatPanelTheme`]
//! on it (or on a thin bridge struct) so the renderer can read
//! colors, styles, and message prefixes without re-implementing the
//! color mapping. [`DefaultChatPanelTheme`] carries a reasonable
//! default for hosts that don't need to customize.

use ratatui::style::{Color, Modifier, Style};

/// Colors, styles, and message prefixes the chat panel renderer
/// reads while laying out a conversation history.
///
/// The trait is deliberately narrow: every method is one the
/// renderer genuinely reaches for. Keep this surface narrow on
/// purpose.
pub trait ChatPanelTheme {
    // ---- Colors ----

    /// Default foreground for primary message text.
    fn text_primary(&self) -> Color;

    /// Foreground for secondary text (key labels, "kms_*" tool name labels).
    fn text_secondary(&self) -> Color;

    /// Foreground for muted text (fold footers, "more lines" indicators).
    fn text_muted(&self) -> Color;

    /// Color of the streaming `█` block cursor.
    fn spinner_color(&self) -> Color;

    /// Color for successful tool results.
    fn tool_ok(&self) -> Color;

    /// Color for failed tool results and error lines.
    fn tool_err(&self) -> Color;

    // ---- Composite styles ----

    /// Style for user messages (`▶ ` prefix + body).
    fn user_style(&self) -> Style;

    /// Style for assistant messages.
    fn assistant_style(&self) -> Style;

    /// Style for thinking content lines (the indented body).
    fn thinking_style(&self) -> Style;

    /// Style for the "Thinking:" header line.
    fn thinking_bold_style(&self) -> Style;

    /// Style for the tool call prefix icon.
    fn tool_call_style(&self) -> Style;

    /// Style for the bold tool name.
    fn tool_call_bold_style(&self) -> Style;

    /// Style for the success / done line.
    fn success_style(&self) -> Style;

    // ---- Scrollbar ----

    /// Style for the scrollbar thumb (the "█" the user drags).
    /// Default: `Style::default().fg(text_muted())`.
    fn scrollbar_thumb_style(&self) -> Style {
        Style::default().fg(self.text_muted())
    }

    /// Style for the scrollbar track (the empty cells the thumb
    /// slides through). Default: `Style::default()`.
    fn scrollbar_track_style(&self) -> Style {
        Style::default()
    }

    // ---- String prefixes ----

    fn user_prefix(&self) -> &'static str;
    fn assistant_prefix(&self) -> &'static str;
    fn thinking_prefix(&self) -> &'static str;
    fn tool_prefix(&self) -> &'static str;
    fn tool_ok_prefix(&self) -> &'static str;
    fn tool_err_prefix(&self) -> &'static str;
    fn done_prefix(&self) -> &'static str;
    fn error_prefix(&self) -> &'static str;
}

/// Default implementation carrying values that match the original
/// `kms_tui::Theme::default_theme()` color palette.
///
/// Intentionally not re-exported from the crate root — hosts reach
/// it via `agent_panel_tui::chat::theme::DefaultChatPanelTheme` to
/// keep the public surface narrow.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultChatPanelTheme;

impl ChatPanelTheme for DefaultChatPanelTheme {
    fn text_primary(&self) -> Color {
        Color::White
    }
    fn text_secondary(&self) -> Color {
        Color::Gray
    }
    fn text_muted(&self) -> Color {
        Color::DarkGray
    }
    fn spinner_color(&self) -> Color {
        Color::Yellow
    }
    fn tool_ok(&self) -> Color {
        Color::Green
    }
    fn tool_err(&self) -> Color {
        Color::Red
    }

    fn user_style(&self) -> Style {
        Style::default().fg(Color::Yellow)
    }
    fn assistant_style(&self) -> Style {
        Style::default().fg(Color::White)
    }
    fn thinking_style(&self) -> Style {
        Style::default().fg(Color::Magenta)
    }
    fn thinking_bold_style(&self) -> Style {
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD)
    }
    fn tool_call_style(&self) -> Style {
        Style::default().fg(Color::Cyan)
    }
    fn tool_call_bold_style(&self) -> Style {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }
    fn success_style(&self) -> Style {
        Style::default().fg(Color::Green)
    }

    fn user_prefix(&self) -> &'static str {
        "\u{25b6} "
    }
    fn assistant_prefix(&self) -> &'static str {
        ""
    }
    fn thinking_prefix(&self) -> &'static str {
        "\u{1f4ad} "
    }
    fn tool_prefix(&self) -> &'static str {
        "\u{1f527} "
    }
    fn tool_ok_prefix(&self) -> &'static str {
        "  \u{2713}"
    }
    fn tool_err_prefix(&self) -> &'static str {
        "  \u{2717}"
    }
    fn done_prefix(&self) -> &'static str {
        "\u{2705} "
    }
    fn error_prefix(&self) -> &'static str {
        "\u{274c} "
    }
}
