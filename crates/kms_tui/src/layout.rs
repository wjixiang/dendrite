use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::state::Panel;

const MIN_WIDTH_FOR_3_COL: u16 = 100;
const MIN_WIDTH_FOR_SIDEBAR: u16 = 60;

#[derive(Debug, Clone, Copy)]
pub enum LayoutMode {
    Three,
    Two,
    Single,
}

impl LayoutMode {
    pub fn from_width(width: u16) -> Self {
        if width >= MIN_WIDTH_FOR_3_COL {
            LayoutMode::Three
        } else if width >= MIN_WIDTH_FOR_SIDEBAR {
            LayoutMode::Two
        } else {
            LayoutMode::Single
        }
    }

    pub fn panel_order(&self) -> &'static [Panel] {
        match self {
            LayoutMode::Three => &[
                Panel::Tree,
                Panel::KnowledgeEntity,
                Panel::Agent,
                Panel::Diagnostics,
            ],
            LayoutMode::Two => &[
                Panel::Tree,
                Panel::Agent,
                Panel::KnowledgeEntity,
                Panel::Diagnostics,
            ],
            LayoutMode::Single => &[
                Panel::Agent,
                Panel::Tree,
                Panel::KnowledgeEntity,
                Panel::Diagnostics,
            ],
        }
    }
}

pub struct AppLayout {
    #[allow(dead_code)]
    pub mode: LayoutMode,
    pub tree_area: Rect,
    pub ke_area: Rect,
    pub agent_area: Rect,
    pub diag_area: Rect,
    pub help_area: Rect,
}

pub fn compute(area: Rect) -> AppLayout {
    let mode = LayoutMode::from_width(area.width);

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(95), Constraint::Min(1)])
        .split(area);

    let main = vertical[0];
    let help = vertical[1];

    match mode {
        LayoutMode::Three => {
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(20),
                    Constraint::Percentage(45),
                    Constraint::Percentage(35),
                ])
                .split(main);

            let middle = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(85), Constraint::Percentage(15)])
                .split(columns[1]);

            AppLayout {
                mode,
                tree_area: columns[0],
                ke_area: middle[0],
                diag_area: middle[1],
                agent_area: columns[2],
                help_area: help,
            }
        }
        LayoutMode::Two => {
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
                .split(main);

            let right = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
                .split(columns[1]);

            let ke_diag = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(columns[0]);

            AppLayout {
                mode,
                tree_area: ke_diag[0],
                ke_area: ke_diag[1],
                agent_area: right[0],
                diag_area: right[1],
                help_area: help,
            }
        }
        LayoutMode::Single => {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(60),
                    Constraint::Percentage(15),
                    Constraint::Percentage(15),
                    Constraint::Percentage(10),
                ])
                .split(main);

            AppLayout {
                mode,
                tree_area: rows[1],
                ke_area: rows[2],
                agent_area: rows[0],
                diag_area: rows[3],
                help_area: help,
            }
        }
    }
}
