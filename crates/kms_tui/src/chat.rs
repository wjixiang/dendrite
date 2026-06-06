use ratatui::text::{Line, Span};
use ratatui::style::Style;
use serde_json::Value;

use crate::theme::Theme;

const MAX_THINKING_LINES: usize = 10;
const MAX_TOOL_RESULT_LINES: usize = 6;
const MAX_ARRAY_ITEMS: usize = 8;

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
    pub fn to_lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        match self {
            ChatMessage::User { text } => render_user_message(text, theme),
            ChatMessage::Assistant { text } => render_assistant_message(text, theme),
            ChatMessage::Thinking { text } => render_thinking(text, theme),
            ChatMessage::ToolCall { name, input } => render_tool_call(name, input, theme),
            ChatMessage::ToolResult { ok, content } => render_tool_result(*ok, content, theme),
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
    lines.push(Line::from(Span::styled(
        format!("{}{}", theme.user_prefix, text),
        theme.user_style(),
    )));
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
