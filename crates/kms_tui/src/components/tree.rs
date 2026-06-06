use ratatui::{
    widgets::{Block, Borders, List, ListItem},
};

use crate::state::Panel;
use crate::theme::Theme;

pub fn render_tree(
    items: &[ListItem<'static>],
    focused: Panel,
    theme: &Theme,
) -> List<'static> {
    let block = Block::default()
        .title(" Tree ")
        .borders(Borders::ALL)
        .border_style(theme.focused_border_style(focused == Panel::Tree));
    List::new(items.to_vec())
        .block(block)
        .highlight_style(theme.highlight_style())
        .scroll_padding(2)
}
