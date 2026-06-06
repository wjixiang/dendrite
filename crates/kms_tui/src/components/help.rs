use ratatui::{
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::state::{App, Panel};
use crate::theme::Theme;

pub fn render_help_bar(theme: &Theme, app: &App) -> Paragraph<'static> {
    let context_hints = match app.focused {
        Panel::Tree => vec![
            Span::styled(" [j/k] Navigate ", Style::default().fg(theme.help_key)),
            Span::styled(" [Enter] Select ", Style::default().fg(theme.help_key)),
        ],
        Panel::KnowledgeEntity => vec![
            Span::styled(" [j/k] Scroll ", Style::default().fg(theme.help_key)),
            Span::styled(" [t] Tab ", Style::default().fg(theme.help_key)),
        ],
        Panel::Agent => {
            if app.agent_running {
                vec![
                    Span::styled(" ...running... ", Style::default().fg(theme.spinner)),
                ]
            } else if app.agent_input_active {
                vec![
                    Span::styled(" [Enter] Send ", Style::default().fg(theme.help_key)),
                    Span::styled(" [Esc] Cancel ", Style::default().fg(theme.help_key)),
                ]
            } else {
                vec![
                    Span::styled(" [Enter] Type ", Style::default().fg(theme.help_key)),
                    Span::styled(" [a] Agent ", Style::default().fg(theme.help_key)),
                ]
            }
        }
        Panel::Diagnostics => vec![
            Span::styled(" [j/k] Scroll ", Style::default().fg(theme.help_key)),
        ],
    };

    let mut spans = vec![
        Span::styled(format!(" {:?} ", app.focused), Style::default().fg(theme.accent).add_modifier(ratatui::style::Modifier::BOLD)),
        Span::styled(" \u{2502}", Style::default().fg(theme.text_muted)),
    ];
    spans.extend(context_hints);
    spans.push(Span::styled(" \u{2502}", Style::default().fg(theme.text_muted)));
    spans.push(Span::styled(" [Tab] Panel ", Style::default().fg(theme.help_text)));
    spans.push(Span::styled(" [s] Settings ", Style::default().fg(theme.help_text)));
    spans.push(Span::styled(" [q] Quit ", Style::default().fg(theme.help_text)));

    Paragraph::new(vec![Line::from(spans)])
}
