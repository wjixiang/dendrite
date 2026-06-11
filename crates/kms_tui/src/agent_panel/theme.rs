//! `AgentPanelTheme` trait + the blanket impl that lets the host
//! keep passing `&Theme` to the renderer unchanged.
//!
//! When the host wants to fully decouple (B/C upgrade), it provides
//! a struct of its own, `impl AgentPanelTheme for MyTheme`, and
//! switches the renderer's `theme: &Theme` parameter to
//! `theme: &dyn AgentPanelTheme`. The blanket impl stays around for
//! tests and any code path that still has a concrete `&Theme` handy.

use ratatui::style::{Color, Style};

use super::AgentPanelTheme;

impl AgentPanelTheme for crate::theme::Theme {
    fn text_primary(&self) -> Color {
        self.text_primary
    }
    fn text_secondary(&self) -> Color {
        self.text_secondary
    }
    fn text_muted(&self) -> Color {
        self.text_muted
    }
    fn spinner(&self) -> Color {
        self.spinner
    }
    fn tool_ok(&self) -> Color {
        self.tool_ok
    }
    fn tool_err(&self) -> Color {
        self.tool_err
    }
    fn error_style(&self) -> Style {
        crate::theme::Theme::error_style(self)
    }
    fn success_style(&self) -> Style {
        crate::theme::Theme::success_style(self)
    }
    fn tool_call_bold_style(&self) -> Style {
        crate::theme::Theme::tool_call_bold_style(self)
    }
}
