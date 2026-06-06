use super::Theme;
use ratatui::style::Color;

#[allow(dead_code)]
pub struct Palette;

#[allow(dead_code)]
impl Palette {
    pub fn nord() -> Theme {
        Theme {
            focused_border: Color::Rgb(136, 192, 208),
            unfocused_border: Color::Rgb(76, 86, 106),
            selected_bg: Color::Rgb(76, 86, 106),
            selected_fg: Color::Rgb(236, 239, 244),
            text_primary: Color::Rgb(216, 222, 233),
            text_secondary: Color::Rgb(136, 142, 156),
            text_muted: Color::Rgb(76, 86, 106),
            accent: Color::Rgb(136, 192, 208),
            user_message: Color::Rgb(235, 203, 139),
            assistant_message: Color::Rgb(216, 222, 233),
            thinking: Color::Rgb(180, 142, 173),
            tool_call: Color::Rgb(143, 188, 187),
            tool_ok: Color::Rgb(163, 190, 140),
            tool_err: Color::Rgb(191, 97, 106),
            error: Color::Rgb(191, 97, 106),
            warning: Color::Rgb(235, 203, 139),
            info: Color::Rgb(136, 192, 208),
            success: Color::Rgb(163, 190, 140),
            spinner: Color::Rgb(235, 203, 139),
            input_bg: Color::Rgb(59, 66, 82),
            modal_bg: Color::Rgb(46, 52, 64),
            modal_border: Color::Rgb(136, 192, 208),
            modal_selected_bg: Color::Rgb(235, 203, 139),
            modal_selected_fg: Color::Rgb(46, 52, 64),
            help_key: Color::Rgb(163, 190, 140),
            help_text: Color::Rgb(76, 86, 106),
            tab_active: Color::Rgb(136, 192, 208),
            tab_inactive: Color::Rgb(76, 86, 106),
            ..Theme::default_theme()
        }
    }

    pub fn tokyo_night() -> Theme {
        Theme {
            focused_border: Color::Rgb(125, 207, 255),
            unfocused_border: Color::Rgb(82, 96, 120),
            selected_bg: Color::Rgb(82, 96, 120),
            selected_fg: Color::Rgb(192, 202, 245),
            text_primary: Color::Rgb(169, 177, 214),
            text_secondary: Color::Rgb(131, 139, 167),
            text_muted: Color::Rgb(82, 96, 120),
            accent: Color::Rgb(125, 207, 255),
            user_message: Color::Rgb(224, 175, 104),
            assistant_message: Color::Rgb(169, 177, 214),
            thinking: Color::Rgb(187, 154, 247),
            tool_call: Color::Rgb(112, 195, 220),
            tool_ok: Color::Rgb(158, 206, 106),
            tool_err: Color::Rgb(247, 118, 142),
            error: Color::Rgb(247, 118, 142),
            warning: Color::Rgb(224, 175, 104),
            info: Color::Rgb(125, 207, 255),
            success: Color::Rgb(158, 206, 106),
            spinner: Color::Rgb(224, 175, 104),
            input_bg: Color::Rgb(31, 36, 51),
            modal_bg: Color::Rgb(26, 27, 38),
            modal_border: Color::Rgb(125, 207, 255),
            modal_selected_bg: Color::Rgb(224, 175, 104),
            modal_selected_fg: Color::Rgb(26, 27, 38),
            help_key: Color::Rgb(158, 206, 106),
            help_text: Color::Rgb(82, 96, 120),
            tab_active: Color::Rgb(125, 207, 255),
            tab_inactive: Color::Rgb(82, 96, 120),
            ..Theme::default_theme()
        }
    }

    pub fn catppuccin_mocha() -> Theme {
        Theme {
            focused_border: Color::Rgb(137, 180, 250),
            unfocused_border: Color::Rgb(88, 91, 112),
            selected_bg: Color::Rgb(88, 91, 112),
            selected_fg: Color::Rgb(205, 214, 244),
            text_primary: Color::Rgb(205, 214, 244),
            text_secondary: Color::Rgb(147, 153, 178),
            text_muted: Color::Rgb(88, 91, 112),
            accent: Color::Rgb(137, 180, 250),
            user_message: Color::Rgb(249, 226, 175),
            assistant_message: Color::Rgb(205, 214, 244),
            thinking: Color::Rgb(203, 166, 247),
            tool_call: Color::Rgb(148, 226, 213),
            tool_ok: Color::Rgb(166, 227, 161),
            tool_err: Color::Rgb(243, 139, 168),
            error: Color::Rgb(243, 139, 168),
            warning: Color::Rgb(249, 226, 175),
            info: Color::Rgb(137, 180, 250),
            success: Color::Rgb(166, 227, 161),
            spinner: Color::Rgb(249, 226, 175),
            input_bg: Color::Rgb(49, 50, 68),
            modal_bg: Color::Rgb(30, 30, 46),
            modal_border: Color::Rgb(137, 180, 250),
            modal_selected_bg: Color::Rgb(249, 226, 175),
            modal_selected_fg: Color::Rgb(30, 30, 46),
            help_key: Color::Rgb(166, 227, 161),
            help_text: Color::Rgb(88, 91, 112),
            tab_active: Color::Rgb(137, 180, 250),
            tab_inactive: Color::Rgb(88, 91, 112),
            ..Theme::default_theme()
        }
    }
}
