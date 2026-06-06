mod palette;

use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct Theme {
    pub focused_border: Color,
    pub unfocused_border: Color,
    pub selected_bg: Color,
    #[allow(dead_code)]
    pub selected_fg: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub user_message: Color,
    pub assistant_message: Color,
    pub thinking: Color,
    pub tool_call: Color,
    pub tool_ok: Color,
    pub tool_err: Color,
    pub error: Color,
    pub warning: Color,
    pub info: Color,
    pub success: Color,
    pub spinner: Color,
    pub input_bg: Color,
    pub modal_bg: Color,
    pub modal_border: Color,
    pub modal_selected_bg: Color,
    pub modal_selected_fg: Color,
    pub help_key: Color,
    pub help_text: Color,
    pub tab_active: Color,
    pub tab_inactive: Color,
    pub tree_group_icon: &'static str,
    pub tree_item_icon: &'static str,
    pub user_prefix: &'static str,
    pub assistant_prefix: &'static str,
    pub thinking_prefix: &'static str,
    pub tool_prefix: &'static str,
    pub tool_ok_prefix: &'static str,
    pub tool_err_prefix: &'static str,
    pub done_prefix: &'static str,
    pub error_prefix: &'static str,
}

impl Theme {
    pub fn default_theme() -> Self {
        Self {
            focused_border: Color::Cyan,
            unfocused_border: Color::DarkGray,
            selected_bg: Color::DarkGray,
            selected_fg: Color::White,
            text_primary: Color::White,
            text_secondary: Color::Gray,
            text_muted: Color::DarkGray,
            accent: Color::Cyan,
            user_message: Color::Yellow,
            assistant_message: Color::White,
            thinking: Color::Magenta,
            tool_call: Color::Cyan,
            tool_ok: Color::Green,
            tool_err: Color::Red,
            error: Color::Red,
            warning: Color::Yellow,
            info: Color::Cyan,
            success: Color::Green,
            spinner: Color::Yellow,
            input_bg: Color::DarkGray,
            modal_bg: Color::DarkGray,
            modal_border: Color::Cyan,
            modal_selected_bg: Color::Yellow,
            modal_selected_fg: Color::Black,
            help_key: Color::Green,
            help_text: Color::DarkGray,
            tab_active: Color::Cyan,
            tab_inactive: Color::DarkGray,
            tree_group_icon: "\u{25b8} ",
            tree_item_icon: "\u{25cf} ",
            user_prefix: "\u{25b6} ",
            assistant_prefix: "",
            thinking_prefix: "\u{1f4ad} ",
            tool_prefix: "\u{1f527} ",
            tool_ok_prefix: "  \u{2713}",
            tool_err_prefix: "  \u{2717}",
            done_prefix: "\u{2705} ",
            error_prefix: "\u{274c} ",
        }
    }

    pub fn focused_border_style(&self, is_focused: bool) -> Style {
        let color = if is_focused {
            self.focused_border
        } else {
            self.unfocused_border
        };
        Style::default().fg(color)
    }

    #[allow(dead_code)]
    pub fn tool_ok_style(&self) -> Style {
        Style::default().fg(self.tool_ok)
    }

    #[allow(dead_code)]
    pub fn tool_err_style(&self) -> Style {
        Style::default().fg(self.tool_err)
    }

    pub fn highlight_style(&self) -> Style {
        Style::default()
            .bg(self.selected_bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn user_style(&self) -> Style {
        Style::default().fg(self.user_message)
    }

    pub fn assistant_style(&self) -> Style {
        Style::default().fg(self.assistant_message)
    }

    pub fn thinking_style(&self) -> Style {
        Style::default().fg(self.thinking)
    }

    pub fn thinking_bold_style(&self) -> Style {
        Style::default()
            .fg(self.thinking)
            .add_modifier(Modifier::BOLD)
    }

    pub fn tool_call_style(&self) -> Style {
        Style::default().fg(self.tool_call)
    }

    pub fn tool_call_bold_style(&self) -> Style {
        Style::default()
            .fg(self.tool_call)
            .add_modifier(Modifier::BOLD)
    }

    pub fn error_style(&self) -> Style {
        Style::default().fg(self.error)
    }

    pub fn success_style(&self) -> Style {
        Style::default().fg(self.success)
    }

    pub fn modal_highlight_style(&self) -> Style {
        Style::default()
            .fg(self.modal_selected_fg)
            .bg(self.modal_selected_bg)
            .add_modifier(Modifier::BOLD)
    }
}
