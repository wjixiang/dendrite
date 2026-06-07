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
            // When a parallel-dispatch panel is open, surface its
            // sub-focus shortcuts (`j/k` to move between sub-agents,
            // `Enter` to expand, `e`/`c` for expand/collapse all).
            // The Tab key toggles the sub-focus between the chat
            // messages and the parallel panel; we keep that as a
            // global hint at the bottom of the bar.
            if app.agent_running {
                vec![
                    Span::styled(" ...running... ", Style::default().fg(theme.spinner)),
                ]
            } else if app.agent_input_active {
                vec![
                    Span::styled(" [Enter] Send ", Style::default().fg(theme.help_key)),
                    Span::styled(" [Esc] Cancel ", Style::default().fg(theme.help_key)),
                ]
            } else if app.parallel_panel.is_some() && app.chat_focus == crate::state::ChatFocus::ParallelPanel {
                vec![
                    Span::styled(" [j/k] Sub-agent ", Style::default().fg(theme.help_key)),
                    Span::styled(" [Enter] Expand ", Style::default().fg(theme.help_key)),
                    Span::styled(" [e/c] All ", Style::default().fg(theme.help_key)),
                ]
            } else {
                // Messages sub-focus (or no parallel panel). The chat
                // history scrolls like a normal scrollable panel —
                // `j/k` moves line by line, `End` jumps to the bottom
                // and re-engages auto-follow. `PgUp`/`PgDn` are
                // handled at the global keymap level (see
                // `handle_page_up` / `handle_page_down`).
                let mut hints = vec![
                    Span::styled(" [Enter] Type ", Style::default().fg(theme.help_key)),
                    Span::styled(" [j/k] Scroll ", Style::default().fg(theme.help_key)),
                    Span::styled(" [End] Follow ", Style::default().fg(theme.help_key)),
                ];
                if app.parallel_panel.is_some() {
                    hints.push(Span::styled(" [Tab] Panel ", Style::default().fg(theme.help_key)));
                } else {
                    hints.push(Span::styled(" [a] Agent ", Style::default().fg(theme.help_key)));
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
        Span::styled(" \u{2502}", Style::default().fg(theme.text_muted)),
    ];
    spans.extend(context_hints);
    spans.push(Span::styled(" \u{2502}", Style::default().fg(theme.text_muted)));
    spans.push(Span::styled(" [Tab] Panel ", Style::default().fg(theme.help_text)));
    spans.push(Span::styled(" [s] Settings ", Style::default().fg(theme.help_text)));
    spans.push(Span::styled(" [q] Quit ", Style::default().fg(theme.help_text)));

    Paragraph::new(vec![Line::from(spans)])
}
