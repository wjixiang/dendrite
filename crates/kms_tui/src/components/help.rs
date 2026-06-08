use ratatui::{
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::state::{App, ChatFocus, Panel};
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
            // The Agent panel has two sub-sections. The hints differ
            // based on `chat_focus` so the user knows which set of
            // keys is live.
            if app.agent_running {
                vec![
                    Span::styled(" ...running... ", Style::default().fg(theme.spinner)),
                ]
            } else if app.agent_input_active {
                vec![
                    Span::styled(" [Enter] Send ", Style::default().fg(theme.help_key)),
                    Span::styled(" [Esc] Cancel ", Style::default().fg(theme.help_key)),
                ]
            } else if app.chat_focus == ChatFocus::AgentsPanel
                && !app.agent_panel.agents.is_empty()
            {
                // Sub-agent list is the active sub-focus.
                vec![
                    Span::styled(" [j/k] Select ", Style::default().fg(theme.help_key)),
                    Span::styled(" [Enter] Expand ", Style::default().fg(theme.help_key)),
                    Span::styled(" [e/c] All ", Style::default().fg(theme.help_key)),
                    Span::styled(" [S-Tab] Back ", Style::default().fg(theme.help_key)),
                ]
            } else {
                // Chat history is the active sub-focus (default).
                let mut hints = vec![
                    Span::styled(" [Enter] Type ", Style::default().fg(theme.help_key)),
                    Span::styled(" [j/k] Scroll ", Style::default().fg(theme.help_key)),
                    Span::styled(" [End] Follow ", Style::default().fg(theme.help_key)),
                    Span::styled(" [a] Agent ", Style::default().fg(theme.help_key)),
                ];
                if !app.agent_panel.agents.is_empty() {
                    hints.push(Span::styled(
                        " [Tab] Agents ",
                        Style::default().fg(theme.help_key),
                    ));
                }
                hints
            }
        }
        Panel::Diagnostics => vec![
            Span::styled(" [j/k] Scroll ", Style::default().fg(theme.help_key)),
        ],
    };

    let mut spans = vec![
        Span::styled(format!(" {:?} ", app.focused), Style::default().fg(theme.accent).add_modifier(ratatui::style::Modifier::BOLD)),
        Span::styled(" ─ ", Style::default().fg(theme.text_muted)),
    ];
    spans.extend(context_hints);
    spans.push(Span::styled(" ─ ", Style::default().fg(theme.text_muted)));
    spans.push(Span::styled(" [Tab] Panel ", Style::default().fg(theme.help_text)));
    spans.push(Span::styled(" [s] Settings ", Style::default().fg(theme.help_text)));
    spans.push(Span::styled(" [q] Quit ", Style::default().fg(theme.help_text)));

    Paragraph::new(vec![Line::from(spans)])
}
