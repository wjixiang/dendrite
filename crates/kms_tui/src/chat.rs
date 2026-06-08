use ratatui::style::Style;
use ratatui::text::{Line, Span};
use serde_json::Value;

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

#[derive(Debug, Clone)]
pub enum ChatMessage {
    User {
        text: String,
    },
    Assistant {
        text: String,
        /// `true` while the LLM is still streaming tokens into this
        /// message. The renderer appends a `█` block-cursor so the user
        /// can see the text is still growing.
        streaming: bool,
    },
    Thinking {
        text: String,
        /// `true` while the model is still emitting thinking tokens.
        streaming: bool,
    },
    ToolCall {
        name: String,
        input: Value,
    },
    ToolResult {
        ok: bool,
        content: String,
        /// Pre-parsed JSON value to avoid re-parsing on every render
        /// frame. Parsed once at insertion time.
        parsed: Option<Value>,
    },
    Done,
    Error {
        message: String,
    },
    Divider,
}

impl ChatMessage {
    /// Render this message as a sequence of *logical* lines.
    pub fn to_lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        match self {
            ChatMessage::User { text } => render_user_message(text, theme),
            ChatMessage::Assistant { text, streaming } => {
                render_assistant_message(text, *streaming, theme)
            }
            ChatMessage::Thinking { text, streaming } => render_thinking(text, *streaming, theme),
            ChatMessage::ToolCall { name, input } => render_tool_call(name, input, theme),
            ChatMessage::ToolResult { ok, content, parsed } => {
                render_tool_result(*ok, content, parsed.as_ref(), theme)
            }
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

    /// Cheap upper-bound estimate of the number of `Line` objects that
    /// `to_lines()` would produce.
    pub fn estimate_lines(&self) -> usize {
        match self {
            ChatMessage::User { text } => {
                let line_count = text.lines().count();
                let truncated = line_count > MAX_USER_MESSAGE_LINES;
                let display = if truncated { MAX_USER_MESSAGE_LINES } else { line_count };
                let mut est = display;
                if truncated { est += 1; }
                est + 1
            }
            ChatMessage::Assistant { text, .. } => text.lines().count().max(1),
            ChatMessage::Thinking { text, .. } => {
                let line_count = text.lines().count();
                let mut est = 1;
                est += line_count.min(MAX_THINKING_LINES);
                if line_count > MAX_THINKING_LINES { est += 1; }
                est.max(2)
            }
            ChatMessage::ToolCall { name: _, input } => {
                1 + input.as_object().map_or(1, |m| m.len())
            }
            ChatMessage::ToolResult { ok: _, content: _, parsed } => {
                let mut est = 1;
                match parsed {
                    Some(Value::Object(map)) => { est += map.len(); }
                    Some(Value::Array(arr)) => { est += arr.len().min(MAX_ARRAY_ITEMS) + 1; }
                    _ => { est += 1; }
                }
                est
            }
            ChatMessage::Done => 1,
            ChatMessage::Error { .. } => 1,
            ChatMessage::Divider => 1,
        }
    }
}

fn render_user_message(text: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let text_lines: Vec<&str> = text.lines().collect();
    let total = text_lines.len();
    let truncated = total > MAX_USER_MESSAGE_LINES;
    let display_count = if truncated { MAX_USER_MESSAGE_LINES } else { total };

    for (i, line_text) in text_lines.iter().take(display_count).enumerate() {
        let prefix = if i == 0 {
            theme.user_prefix.to_string()
        } else {
            "    ".to_string()
        };
        lines.push(Line::from(Span::styled(
            format!("{}{}", prefix, line_text),
            theme.user_style(),
        )));
    }

    if truncated {
        lines.push(Line::from(Span::styled(
            format!("    … {} more lines (truncated)", total - MAX_USER_MESSAGE_LINES),
            Style::default().fg(theme.text_muted),
        )));
    }

    lines
}

fn render_assistant_message(text: &str, streaming: bool, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = text
        .lines()
        .map(|l| {
            Line::from(Span::styled(
                format!("{}{}", theme.assistant_prefix, l),
                theme.assistant_style(),
            ))
        })
        .collect();

    if streaming {
        if let Some(last) = lines.last_mut() {
            last.spans.push(Span::styled("█".to_string(), Style::default().fg(theme.spinner)));
        } else {
            lines.push(Line::from(Span::styled(
                format!("{}█", theme.assistant_prefix),
                Style::default().fg(theme.spinner),
            )));
        }
    }
    lines
}

fn render_thinking(text: &str, streaming: bool, theme: &Theme) -> Vec<Line<'static>> {
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
            format!("   … {} more lines", total - MAX_THINKING_LINES),
            Style::default().fg(theme.text_muted),
        )));
    }
    if streaming {
        if let Some(last) = lines.last_mut() {
            last.spans.push(Span::styled("█".to_string(), Style::default().fg(theme.spinner)));
        }
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

fn render_tool_result(ok: bool, content: &str, parsed: Option<&Value>, theme: &Theme) -> Vec<Line<'static>> {
    let (prefix, color) = if ok {
        (theme.tool_ok_prefix.to_string(), theme.tool_ok)
    } else {
        (theme.tool_err_prefix.to_string(), theme.tool_err)
    };
    let style = Style::default().fg(color);
    let mut lines = vec![Line::from(Span::styled(prefix.clone(), style))];

    match parsed {
        Some(Value::Object(map)) => {
            for (k, v) in map {
                lines.push(Line::from(vec![
                    Span::styled("    ".to_string(), Style::default()),
                    Span::styled(format!("{}: ", k), Style::default().fg(theme.text_muted)),
                    Span::styled(format_value(v), style),
                ]));
            }
        }
        Some(Value::Array(arr)) => {
            for item in arr.iter().take(MAX_ARRAY_ITEMS) {
                let label = if let Some(s) = item.as_str() { truncate_str(s, usize::MAX) } else { format_value(item) };
                lines.push(Line::from(vec![
                    Span::styled("    ".to_string(), Style::default()),
                    Span::styled(format!("  • {}", label), style),
                ]));
            }
            if arr.len() > MAX_ARRAY_ITEMS {
                lines.push(Line::from(Span::styled(
                    format!("    … and {} more", arr.len() - MAX_ARRAY_ITEMS),
                    Style::default().fg(theme.text_muted),
                )));
            }
        }
        Some(Value::String(s)) => {
            for l in s.lines().take(MAX_TOOL_RESULT_LINES) {
                lines.push(Line::from(vec![
                    Span::styled("    ".to_string(), Style::default()),
                    Span::styled(truncate_str(l, usize::MAX), style),
                ]));
            }
        }
        Some(other) => {
            lines.push(Line::from(vec![
                Span::styled("    ".to_string(), Style::default()),
                Span::styled(truncate_str(&other.to_string(), usize::MAX), style),
            ]));
        }
        None => {
            // Fallback: couldn't parse JSON, show raw text lines.
            for l in content.lines().take(MAX_TOOL_RESULT_LINES) {
                lines.push(Line::from(vec![
                    Span::styled("    ".to_string(), Style::default()),
                    Span::styled(truncate_str(l, usize::MAX), style),
                ]));
            }
        }
    }
    lines
}

pub fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max { return s.to_string(); }
    let end = s.char_indices().take_while(|(i, _)| *i < max).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(max);
    format!("{}…", &s[..end])
}

pub fn format_value(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => truncate_str(s, usize::MAX),
        Value::Array(arr) => {
            if arr.is_empty() { "[]".to_string() } else if arr.len() == 1 { format!("[{}]", format_value(&arr[0])) } else { format!("[{} items]", arr.len()) }
        }
        Value::Object(_) => "{…}".to_string(),
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;

    fn theme() -> Theme { Theme::default_theme() }

    fn message_count(lines: &[Line<'_>]) -> usize {
        let mut total_more = 0usize;
        for line in lines {
            for span in &line.spans {
                if let Some(rest) = span.content.split('…').nth(1) {
                    if let Some(n) = rest.trim_start().trim_start_matches(" and ").trim_start_matches("more lines (truncated)").split_whitespace().next().and_then(|s| s.parse::<usize>().ok()) {
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
        assert_eq!(lines.len(), 3);
        assert!(lines[0].to_string().contains(theme().user_prefix));
        assert!(lines[1].to_string().starts_with("    "));
    }

    #[test]
    fn exactly_threshold_lines_is_not_truncated() {
        let text = (1..=10).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let lines = render_user_message(&text, &theme());
        assert_eq!(lines.len(), 10);
        assert_eq!(message_count(&lines), 0);
    }

    #[test]
    fn over_threshold_user_message_is_folded() {
        let text = (1..=50).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let lines = render_user_message(&text, &theme());
        assert_eq!(lines.len(), 11);
        assert_eq!(message_count(&lines), 40);
    }

    #[test]
    fn assistant_message_is_never_folded() {
        let text = (1..=200).map(|i| format!("reply {i}")).collect::<Vec<_>>().join("\n");
        let lines = render_assistant_message(&text, false, &theme());
        assert_eq!(lines.len(), 200);
    }

    #[test]
    fn empty_user_message_still_renders_nothing() {
        let lines = render_user_message("", &theme());
        assert_eq!(lines.len(), 0);
    }
}

#[cfg(test)]
mod estimate_tests {
    use super::*;

    fn theme() -> Theme { Theme::default_theme() }

    #[test]
    fn estimate_never_undercounts_assistant() {
        let msg = ChatMessage::Assistant { text: "line1\nline2\nline3".into(), streaming: false };
        let actual = msg.to_lines(&theme()).len();
        assert!(msg.estimate_lines() >= actual, "estimate={} < actual={}", msg.estimate_lines(), actual);
    }

    #[test]
    fn estimate_never_undercounts_user() {
        let msg = ChatMessage::User { text: "line1\nline2".into() };
        let actual = msg.to_lines(&theme()).len();
        assert!(msg.estimate_lines() >= actual, "estimate={} < actual={}", msg.estimate_lines(), actual);
    }

    #[test]
    fn estimate_simple_types() {
        assert_eq!(ChatMessage::Divider.estimate_lines(), 1);
        assert_eq!(ChatMessage::Done.estimate_lines(), 1);
        assert_eq!(ChatMessage::Error { message: "oops".into() }.estimate_lines(), 1);
    }
}
