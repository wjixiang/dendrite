use ratatui::text::{Line, Span};
use ratatui::style::Style;
use serde_json::Value;

use crate::parallel_panel::{ParallelPanelState, SubAgentEvent};
use crate::theme::Theme;

const MAX_THINKING_LINES: usize = 10;
const MAX_TOOL_RESULT_LINES: usize = 6;
const MAX_ARRAY_ITEMS: usize = 8;
/// Maximum number of *logical* lines from a user message that the
/// chat panel will display verbatim. Anything beyond is collapsed
/// into a single "… N more lines (truncated)" indicator. The full
/// text is still sent to the agent on submit; this only affects the
/// on-screen chat history. The primary fold mechanism is the
/// `[Pasted ~N lines]` placeholder kept in the message itself;
/// this constant is the fallback for messages the user typed out
/// verbatim (no paste).
const MAX_USER_MESSAGE_LINES: usize = 10;

/// Upper bound for the user-message underline separator. Long enough to
/// visually fill wide panels, short enough to never wrap on narrow
/// ones. The actual wrap is delegated to `Paragraph::scroll` so this
/// value is purely cosmetic.
const USER_SEPARATOR_LIMIT: usize = 80;

#[derive(Debug, Clone)]
pub enum ChatMessage {
    User { text: String },
    Assistant { text: String },
    Thinking { text: String },
    ToolCall { name: String, input: Value },
    ToolResult { ok: bool, content: String },
    /// Placeholder for the parallel-dispatch panel. Inserted right
    /// after `ToolCall: kms_parallel_dispatch`; the renderer expands
    /// it to a multi-line block (header + collapsible sub-agent rows)
    /// by consulting the live `ParallelPanelState` in `App`.
    ///
    /// The `Vec` is empty by construction — the message is purely a
    /// marker; actual data lives in `App.parallel_panel`.
    ParallelBlock,
    Done,
    Error { message: String },
    Divider,
}

impl ChatMessage {
    /// Render this message as a sequence of *logical* lines.
    ///
    /// Note: this function does **not** perform visual word-wrapping.
    /// Word-wrap is the responsibility of `ratatui::Paragraph` (via
    /// `Wrap`), which knows the actual widget width at draw time. Doing
    /// wrap here would risk double-wrapping and, more importantly,
    /// would force the caller to pre-clamp the visible content, which
    /// is what the previous height-clamp bug stemmed from.
    ///
    /// `panel` is consulted only for `ChatMessage::ParallelBlock` and
    /// is `None` for all other variants. When the user is in a
    /// different agent mode (Compose / Knowledge) the panel is always
    /// `None`; only Parallel mode shows it.
    pub fn to_lines(
        &self,
        theme: &Theme,
        panel: Option<&ParallelPanelState>,
    ) -> Vec<Line<'static>> {
        match self {
            ChatMessage::User { text } => render_user_message(text, theme),
            ChatMessage::Assistant { text } => render_assistant_message(text, theme),
            ChatMessage::Thinking { text } => render_thinking(text, theme),
            ChatMessage::ToolCall { name, input } => render_tool_call(name, input, theme),
            ChatMessage::ToolResult { ok, content } => render_tool_result(*ok, content, theme),
            ChatMessage::ParallelBlock => match panel {
                Some(p) => render_parallel_panel(p, theme),
                // If the panel state has been cleared (e.g. mode switch)
                // we render an empty block so the chat layout doesn't
                // shift.
                None => vec![Line::from("")],
            },
            ChatMessage::Done => vec![Line::from(Span::styled(
                format!("{}Agent completed", theme.done_prefix),
                theme.success_style(),
            ))],
            ChatMessage::Error { message } => vec![Line::from(Span::styled(
                format!("{}{}", theme.error_prefix, message),
                theme.error_style(),
            ))],
            ChatMessage::Divider => vec![Line::from("")],
        }
    }
}

fn render_user_message(text: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Split into per-line rows so the prefix only appears on the
    // first line and the layout mirrors `render_assistant_message`.
    // Without this split, a 100-line message would render as a single
    // very tall `Line` that wraps unpredictably.
    let text_lines: Vec<&str> = text.lines().collect();
    let total = text_lines.len();
    let truncated = total > MAX_USER_MESSAGE_LINES;

    let display_count = if truncated { MAX_USER_MESSAGE_LINES } else { total };

    for (i, line_text) in text_lines.iter().take(display_count).enumerate() {
        let prefix = if i == 0 { theme.user_prefix.to_string() } else { "    ".to_string() };
        lines.push(Line::from(Span::styled(
            format!("{}{}", prefix, line_text),
            theme.user_style(),
        )));
    }

    if truncated {
        lines.push(Line::from(Span::styled(
            format!(
                "    \u{2026} {} more lines (truncated)",
                total - MAX_USER_MESSAGE_LINES
            ),
            Style::default().fg(theme.text_muted),
        )));
    }

    let separator = "\u{2500}".repeat(USER_SEPARATOR_LIMIT);
    lines.push(Line::from(Span::styled(
        separator,
        Style::default().fg(theme.user_message),
    )));
    lines
}

fn render_assistant_message(text: &str, theme: &Theme) -> Vec<Line<'static>> {
    text.lines()
        .map(|l| {
            Line::from(Span::styled(
                format!("{}{}", theme.assistant_prefix, l),
                theme.assistant_style(),
            ))
        })
        .collect()
}

fn render_thinking(text: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        format!("{}Thinking:", theme.thinking_prefix),
        theme.thinking_bold_style(),
    ))];
    for l in text.lines().take(MAX_THINKING_LINES) {
        lines.push(Line::from(Span::styled(
            format!("   {}", l),
            theme.thinking_style(),
        )));
    }
    let total = text.lines().count();
    if total > MAX_THINKING_LINES {
        lines.push(Line::from(Span::styled(
            format!("   \u{2026} {} more lines", total - MAX_THINKING_LINES),
            Style::default().fg(theme.text_muted),
        )));
    }
    lines
}

fn render_tool_call(name: &str, input: &Value, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled(theme.tool_prefix.to_string(), theme.tool_call_style()),
        Span::styled(name.to_string(), theme.tool_call_bold_style()),
    ])];
    if let Some(obj) = input.as_object() {
        for (key, val) in obj {
            lines.push(Line::from(vec![
                Span::styled("    ".to_string(), Style::default()),
                Span::styled(format!("{}: ", key), Style::default().fg(theme.text_secondary)),
                Span::styled(format_value(val), Style::default().fg(theme.text_primary)),
            ]));
        }
    } else if !input.is_null() {
        lines.push(Line::from(vec![
            Span::styled("    ".to_string(), Style::default()),
            Span::styled(truncate_str(&input.to_string(), usize::MAX), Style::default().fg(theme.text_primary)),
        ]));
    }
    lines
}

fn render_tool_result(ok: bool, content: &str, theme: &Theme) -> Vec<Line<'static>> {
    let (prefix, color) = if ok {
        (theme.tool_ok_prefix.to_string(), theme.tool_ok)
    } else {
        (theme.tool_err_prefix.to_string(), theme.tool_err)
    };
    let style = Style::default().fg(color);
    let mut lines = vec![Line::from(Span::styled(prefix.clone(), style))];

    if let Ok(val) = serde_json::from_str::<Value>(content) {
        match &val {
            Value::Object(map) => {
                for (k, v) in map {
                    lines.push(Line::from(vec![
                        Span::styled("    ".to_string(), Style::default()),
                        Span::styled(format!("{}: ", k), Style::default().fg(theme.text_muted)),
                        Span::styled(format_value(v), style),
                    ]));
                }
            }
            Value::Array(arr) => {
                for item in arr.iter().take(MAX_ARRAY_ITEMS) {
                    let label = if let Some(s) = item.as_str() {
                        truncate_str(s, usize::MAX)
                    } else {
                        format_value(item)
                    };
                    lines.push(Line::from(vec![
                        Span::styled("    ".to_string(), Style::default()),
                        Span::styled(format!("  \u{2022} {}", label), style),
                    ]));
                }
                if arr.len() > MAX_ARRAY_ITEMS {
                    lines.push(Line::from(Span::styled(
                        format!("    \u{2026} and {} more", arr.len() - MAX_ARRAY_ITEMS),
                        Style::default().fg(theme.text_muted),
                    )));
                }
            }
            Value::String(s) => {
                for l in s.lines().take(MAX_TOOL_RESULT_LINES) {
                    lines.push(Line::from(vec![
                        Span::styled("    ".to_string(), Style::default()),
                        Span::styled(truncate_str(l, usize::MAX), style),
                    ]));
                }
            }
            other => {
                lines.push(Line::from(vec![
                    Span::styled("    ".to_string(), Style::default()),
                    Span::styled(truncate_str(&other.to_string(), usize::MAX), style),
                ]));
            }
        }
    } else {
        for l in content.lines().take(MAX_TOOL_RESULT_LINES) {
            lines.push(Line::from(vec![
                Span::styled("    ".to_string(), Style::default()),
                Span::styled(truncate_str(l, usize::MAX), style),
            ]));
        }
    }
    lines
}

/// Render the parallel-dispatch panel. Multi-line block:
///
/// ```text
/// ── Parallel Dispatch [2/5 ✓ ✗ ⠋ 1 running] · 1m 23s ─────
///   ▶ 设计原则 [1/3] ✓ 12s                            [▾]
///       (sub-agent events if expanded)
///   ▶ 实现细节 [2/3] ✗ 8s                             [▸]
///   ⠋ 测试用例 [3/3] running... 14s                    [▸]
/// ```
///
/// `panel.selected` is the index of the sub-agent the user is
/// currently focused on; the focused row's toggle indicator is
/// highlighted.
fn render_parallel_panel(
    panel: &ParallelPanelState,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let elapsed = panel.elapsed();
    let elapsed_label = format_duration(elapsed);
    let running = panel.running_count();
    let header = format!(
        " Parallel Dispatch [{}/{} ✓ {} ✗ {} ⠋ {} running] · {} ",
        panel.completed,
        panel.total,
        panel.completed,
        panel.failed,
        running,
        elapsed_label,
    );
    lines.push(Line::from(Span::styled(
        header,
        theme.tool_call_bold_style(),
    )));

    if panel.sub_agents.is_empty() {
        lines.push(Line::from(Span::styled(
            "    (no sub-agents yet)".to_string(),
            Style::default().fg(theme.text_muted),
        )));
        return lines;
    }

    for (idx, entry) in panel.sub_agents.iter().enumerate() {
        let is_selected = idx == panel.selected;
        let (icon, status_str, status_style) = match &entry.status {
            crate::parallel_panel::SubAgentStatus::Running => (
                "⠋",
                "running...".to_string(),
                Style::default().fg(theme.spinner),
            ),
            crate::parallel_panel::SubAgentStatus::Completed { duration } => (
                "✓",
                format!("{}s", duration.as_secs_f64()),
                theme.success_style(),
            ),
            crate::parallel_panel::SubAgentStatus::Failed { error: _, duration } => (
                "✗",
                format!("{}s", duration.as_secs_f64()),
                theme.error_style(),
            ),
        };
        let toggle = if entry.expanded { "[▾]" } else { "[▸]" };
        let row_style = if is_selected {
            theme.tool_call_bold_style()
        } else {
            Style::default()
        };
        // Color the status segment (running/completed/failed) with the
        // status_style; keep the rest in the row's base style so the
        // selection highlight still wins on the title.
        lines.push(Line::from(vec![
            Span::styled(format!("  {} {} [{}] {} ", icon, entry.title, idx + 1, panel.total), row_style),
            Span::styled(status_str, status_style),
            Span::styled(format!("    {}", toggle), row_style),
        ]));

        if entry.expanded {
            for ev in &entry.events {
                let indented = render_sub_agent_event(ev, theme);
                lines.extend(indented);
            }
        }
    }

    lines
}

fn render_sub_agent_event(
    ev: &SubAgentEvent,
    theme: &Theme,
) -> Vec<Line<'static>> {
    match ev {
        SubAgentEvent::LlmResponse(s) => {
            if s.is_empty() {
                Vec::new()
            } else {
                vec![Line::from(vec![
                    Span::styled("      💬 ".to_string(), Style::default()),
                    Span::styled(
                        truncate_str(s, 200),
                        Style::default().fg(theme.text_primary),
                    ),
                ])]
            }
        }
        SubAgentEvent::ToolCall { name, input } => {
            let summary = if let Some(obj) = input.as_object() {
                let first = obj.iter().next();
                match first {
                    Some((k, v)) => format!("{}: {}", k, format_value(v)),
                    None => "{}".to_string(),
                }
            } else {
                truncate_str(&input.to_string(), 80)
            };
            vec![Line::from(vec![
                Span::styled("      🔧 ".to_string(), Style::default()),
                Span::styled(
                    format!("{} ", name),
                    Style::default().fg(theme.tool_call),
                ),
                Span::styled(
                    summary,
                    Style::default().fg(theme.text_secondary),
                ),
            ])]
        }
        SubAgentEvent::ToolResult { ok, content } => {
            let (icon, color) = if *ok {
                ("✓", theme.tool_ok)
            } else {
                ("✗", theme.tool_err)
            };
            let summary = if let Ok(val) = serde_json::from_str::<Value>(content) {
                if let Some(s) = val.as_str() {
                    s.to_string()
                } else {
                    format_value(&val)
                }
            } else {
                truncate_str(content, 80)
            };
            vec![Line::from(vec![
                Span::styled(format!("      {} ", icon), Style::default().fg(color)),
                Span::styled(summary, Style::default().fg(theme.text_muted)),
            ])]
        }
        SubAgentEvent::Error(msg) => vec![Line::from(vec![
            Span::styled("      ✗ ".to_string(), theme.error_style()),
            Span::styled(
                truncate_str(msg, 200),
                theme.error_style(),
            ),
        ])],
    }
}

fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}

pub fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let end = s
        .char_indices()
        .take_while(|(i, _)| *i < max)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(max);
    format!("{}\u{2026}", &s[..end])
}

pub fn format_value(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => truncate_str(s, usize::MAX),
        Value::Array(arr) => {
            if arr.is_empty() {
                "[]".to_string()
            } else if arr.len() == 1 {
                format!("[{}]", format_value(&arr[0]))
            } else {
                format!("[{} items]", arr.len())
            }
        }
        Value::Object(_) => "{\u{2026}}".to_string(),
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::theme::Theme;

    fn theme() -> Theme {
        Theme::default_theme()
    }

    fn message_count(lines: &[Line<'_>]) -> usize {
        // Count "more lines" mentions in any span.
        let mut total_more = 0usize;
        for line in lines {
            for span in &line.spans {
                if let Some(rest) = span.content.split('\u{2026}').nth(1) {
                    if let Some(n) = rest
                        .trim_start()
                        .trim_start_matches(" and ")
                        .trim_start_matches("more lines (truncated)")
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        total_more += n;
                    }
                }
            }
        }
        total_more
    }

    #[test]
    fn short_user_message_renders_in_full() {
        let text = "line a\nline b\nline c";
        let lines = render_user_message(text, &theme());
        // 3 text lines + 1 separator = 4
        assert_eq!(lines.len(), 4);
        // No "more lines" indicator on short messages.
        assert_eq!(message_count(&lines), 0);
        // First line should carry the user prefix.
        assert!(lines[0].to_string().contains(theme().user_prefix));
        // Continuation lines should NOT carry the prefix (replaced by indent).
        assert!(lines[1].to_string().starts_with("    "));
    }

    #[test]
    fn exactly_threshold_lines_is_not_truncated() {
        let text = (1..=10).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let lines = render_user_message(&text, &theme());
        // 10 text lines + 1 separator = 11, no truncation marker
        assert_eq!(lines.len(), 11);
        assert_eq!(message_count(&lines), 0);
    }

    #[test]
    fn over_threshold_user_message_is_folded() {
        let text = (1..=50).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let lines = render_user_message(&text, &theme());
        // 10 visible text lines + 1 "more lines" indicator + 1 separator = 12
        assert_eq!(lines.len(), 12);
        // The truncation indicator reports exactly 40 hidden lines.
        assert_eq!(message_count(&lines), 40);
        // The "… 40 more lines (truncated)" marker must be present.
        let marker = lines
            .iter()
            .map(|l| l.to_string())
            .find(|s| s.contains("more lines (truncated)"))
            .expect("truncation marker missing");
        assert!(marker.contains("40"));
    }

    #[test]
    fn assistant_message_is_never_folded() {
        // Even a 200-line assistant reply must render in full per the
        // design rule: agent's pure-text responses are always complete.
        let text = (1..=200).map(|i| format!("reply {i}")).collect::<Vec<_>>().join("\n");
        let lines = render_assistant_message(&text, &theme());
        // 200 reply lines, no separator, no truncation marker.
        assert_eq!(lines.len(), 200);
        assert_eq!(message_count(&lines), 0);
    }

    #[test]
    fn empty_user_message_still_renders_separator() {
        let lines = render_user_message("", &theme());
        // Empty text has 0 lines; we still emit the separator.
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn parallel_block_renders_header_with_progress() {
        use crate::parallel_panel::ParallelPanelState;
        let mut panel = ParallelPanelState::new(3);
        panel.apply(&dendrite_tools::parallel_progress::ParallelProgress::StagingCreated {
            index: 0,
            total: 3,
            title: "A".to_string(),
        });
        panel.apply(&dendrite_tools::parallel_progress::ParallelProgress::StagingCreated {
            index: 1,
            total: 3,
            title: "B".to_string(),
        });
        panel.apply(&dendrite_tools::parallel_progress::ParallelProgress::StagingCreated {
            index: 2,
            total: 3,
            title: "C".to_string(),
        });
        panel.apply(&dendrite_tools::parallel_progress::ParallelProgress::SubAgentCompleted {
            index: 0,
            total: 3,
            title: "A".to_string(),
            duration_ms: 1500,
        });
        let lines = render_parallel_panel(&panel, &theme());
        // 1 header + 3 sub-agent rows.
        assert_eq!(lines.len(), 4);
        let header = lines[0].to_string();
        assert!(header.contains("Parallel Dispatch"));
        assert!(header.contains("1/3"));
        // Sub-agent rows show icons and titles.
        let row_a = lines[1].to_string();
        assert!(row_a.contains("A"));
        assert!(row_a.contains("1.5s"));
    }

    #[test]
    fn parallel_block_expanded_shows_sub_agent_events() {
        use crate::parallel_panel::ParallelPanelState;
        use agentik_types::AgentUiEvent;
        use dendrite_tools::parallel_progress::ParallelProgress;
        let mut panel = ParallelPanelState::new(1);
        panel.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        panel.apply(&ParallelProgress::SubAgentEvent {
            title: "A".to_string(),
            event: AgentUiEvent::LlmResponse("hello".to_string()),
        });
        panel.selected = 0;
        panel.toggle_selected(); // expand
        let lines = render_parallel_panel(&panel, &theme());
        // 1 header + 1 row + 1 event line = 3.
        assert_eq!(lines.len(), 3);
        assert!(lines[2].to_string().contains("hello"));
    }

    #[test]
    fn parallel_block_collapsed_does_not_show_events() {
        use crate::parallel_panel::ParallelPanelState;
        use agentik_types::AgentUiEvent;
        use dendrite_tools::parallel_progress::ParallelProgress;
        let mut panel = ParallelPanelState::new(1);
        panel.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        panel.apply(&ParallelProgress::SubAgentEvent {
            title: "A".to_string(),
            event: AgentUiEvent::LlmResponse("hello".to_string()),
        });
        // selected=0 default, expanded=false default
        let lines = render_parallel_panel(&panel, &theme());
        // 1 header + 1 row only.
        assert_eq!(lines.len(), 2);
        // The collapsed line must NOT contain the event text.
        assert!(!lines.iter().any(|l| l.to_string().contains("hello")));
    }

    #[test]
    fn parallel_block_with_no_sub_agents_renders_placeholder() {
        use crate::parallel_panel::ParallelPanelState;
        let panel = ParallelPanelState::new(0);
        let lines = render_parallel_panel(&panel, &theme());
        // 1 header + 1 "(no sub-agents yet)" line.
        assert_eq!(lines.len(), 2);
        assert!(lines[1].to_string().contains("no sub-agents"));
    }
}
