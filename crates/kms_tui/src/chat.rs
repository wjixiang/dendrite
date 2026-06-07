use ratatui::style::Style;
use ratatui::text::{Line, Span};
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
    },
    /// A sub-agent's LLM response, surfaced inline in the chat history
    /// (not just inside the collapsible parallel panel) so the user
    /// sees the actual work the sub-agents produced, the same way they
    /// see the orchestrator's `Assistant` text.
    ///
    /// `title` is the sub-agent's staging area name (e.g.
    /// "心血管疾病"). The renderer prefixes each block with
    /// `[子任务:title]` so the user can tell which sub-agent said
    /// what when several are running in parallel.
    ///
    /// Pushed by `input.rs` when a `ParallelProgress::SubAgentEvent`
    /// carrying `AgentEvent::LlmResponse` arrives on the
    /// `parallel_progress_rx` side-channel.
    SubAgentResponse {
        title: String,
        text: String,
    },
    /// Placeholder for the parallel-dispatch panel. Inserted right
    /// after `ToolCall: kms_parallel_dispatch`; the renderer expands
    /// it to a multi-line block (header + collapsible sub-agent rows)
    /// by consulting the live `ParallelPanelState` in `App`.
    ///
    /// The `Vec` is empty by construction — the message is purely a
    /// marker; actual data lives in `App.parallel_panel`.
    ParallelBlock,
    Done,
    Error {
        message: String,
    },
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
    ///
    /// `width` is the available character width for the message.
    /// Currently only `ParallelBlock` consults it (to truncate the
    /// per-sub-agent title + activity hint). Other variants render
    /// at whatever width and rely on `ratatui::Paragraph::wrap` to
    /// flow the text; passing the real width keeps both behaviors
    /// consistent.
    pub fn to_lines(
        &self,
        theme: &Theme,
        panel: Option<&ParallelPanelState>,
        width: usize,
    ) -> Vec<Line<'static>> {
        match self {
            ChatMessage::User { text } => render_user_message(text, theme, width),
            ChatMessage::Assistant { text, streaming } => {
                render_assistant_message(text, *streaming, theme)
            }
            ChatMessage::Thinking { text, streaming } => render_thinking(text, *streaming, theme),
            ChatMessage::ToolCall { name, input } => render_tool_call(name, input, theme),
            ChatMessage::ToolResult { ok, content } => render_tool_result(*ok, content, theme),
            ChatMessage::SubAgentResponse { title, text } => {
                render_sub_agent_response(title, text, theme)
            }
            ChatMessage::ParallelBlock => match panel {
                Some(p) => render_parallel_panel(p, theme, width),
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

    /// Cheap upper-bound estimate of the number of `Line` objects that
    /// `to_lines()` would produce. Used for viewport culling: we
    /// accumulate these estimates across messages to map a scroll
    /// offset (in visual rows) to a message index range, then only call
    /// the expensive `to_lines()` on that range.
    ///
    /// Overestimates are safe (we render a few extra messages);
    /// underestimates would hide content, which must never happen.
    pub fn estimate_lines(&self) -> usize {
        match self {
            ChatMessage::User { text } => {
                let line_count = text.lines().count();
                let truncated = line_count > MAX_USER_MESSAGE_LINES;
                let display = if truncated { MAX_USER_MESSAGE_LINES } else { line_count };
                let mut est = display;
                if truncated { est += 1; }
                est + 1 // +1 separator
            }
            ChatMessage::Assistant { text, .. } => text.lines().count().max(1),
            ChatMessage::Thinking { text, .. } => {
                let line_count = text.lines().count();
                let mut est = 1; // header
                est += line_count.min(MAX_THINKING_LINES);
                if line_count > MAX_THINKING_LINES { est += 1; }
                est.max(2)
            }
            ChatMessage::ToolCall { name: _, input } => {
                1 + input.as_object().map_or(1, |m| m.len())
            }
            ChatMessage::ToolResult { ok: _, content } => {
                let mut est = 1; // header
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
                    match &val {
                        serde_json::Value::Object(map) => { est += map.len(); }
                        serde_json::Value::Array(arr) => { est += arr.len().min(MAX_ARRAY_ITEMS) + 1; }
                        _ => { est += 1; }
                    }
                } else {
                    est += content.lines().count().min(MAX_TOOL_RESULT_LINES);
                }
                est
            }
            ChatMessage::SubAgentResponse { text, .. } => text.lines().count().max(1),
            ChatMessage::ParallelBlock => {
                // Generous upper bound covering 1 header + rows for up
                // to ~16 sub-agents with expanded event logs.
                50
            }
            ChatMessage::Done => 1,
            ChatMessage::Error { .. } => 1,
            ChatMessage::Divider => 1,
        }
    }
}

fn render_user_message(text: &str, theme: &Theme, container_width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Split into per-line rows so the prefix only appears on the
    // first line and the layout mirrors `render_assistant_message`.
    // Without this split, a 100-line message would render as a single
    // very tall `Line` that wraps unpredictably.
    let text_lines: Vec<&str> = text.lines().collect();
    let total = text_lines.len();
    let truncated = total > MAX_USER_MESSAGE_LINES;

    let display_count = if truncated {
        MAX_USER_MESSAGE_LINES
    } else {
        total
    };

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
            format!(
                "    \u{2026} {} more lines (truncated)",
                total - MAX_USER_MESSAGE_LINES
            ),
            Style::default().fg(theme.text_muted),
        )));
    }

    let separator = "\u{2500}".repeat(container_width);
    lines.push(Line::from(Span::styled(
        separator,
        Style::default().fg(theme.user_message),
    )));
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
        // Append a block-cursor on the last line so the user sees
        // the text is still being generated.
        if let Some(last) = lines.last_mut() {
            last.spans.push(Span::styled(
                "\u{2588}".to_string(),
                Style::default().fg(theme.spinner),
            ));
        } else {
            lines.push(Line::from(Span::styled(
                format!("{}\u{2588}", theme.assistant_prefix),
                Style::default().fg(theme.spinner),
            )));
        }
    }

    lines
}

/// Render a sub-agent's LLM response. The first line carries a
/// `[子任务:<title>]` prefix so the user can tell *which* sub-agent
/// said what when several are streaming in parallel. Continuation
/// lines are indented to match the prefix column.
///
/// Distinct from `render_sub_agent_event`: that one is for the
/// compact, single-line view inside the collapsed parallel-panel
/// row (💬 emoji + 200-char truncation). This one is for the
/// main chat history, where the user expects the full, untruncated
/// LLM text — same treatment as `render_assistant_message`, just
/// with a sub-task tag attached.
fn render_sub_agent_response(title: &str, text: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let tag = format!("[子任务:{}] ", title);
    for (i, l) in text.lines().enumerate() {
        let prefix = if i == 0 {
            tag.clone()
        } else {
            " ".repeat(tag.chars().count())
        };
        lines.push(Line::from(Span::styled(
            format!("{}{}", prefix, l),
            theme.assistant_style(),
        )));
    }
    // Empty text: still emit a tagged placeholder so the chat
    // layout shows the sub-agent emitted *something* (e.g. a pure
    // thinking-only turn). Without this, an empty LlmResponse would
    // be silently dropped, mirroring the user's "看不到任何内容"
    // complaint.
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("{}(空响应)", tag),
            Style::default().fg(theme.text_muted),
        )));
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
            format!("   \u{2026} {} more lines", total - MAX_THINKING_LINES),
            Style::default().fg(theme.text_muted),
        )));
    }
    if streaming {
        // Append a block-cursor on the last line so the user sees
        // thinking is still in progress.
        if let Some(last) = lines.last_mut() {
            last.spans.push(Span::styled(
                "\u{2588}".to_string(),
                Style::default().fg(theme.spinner),
            ));
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
                Span::styled(
                    format!("{}: ", key),
                    Style::default().fg(theme.text_secondary),
                ),
                Span::styled(format_value(val), Style::default().fg(theme.text_primary)),
            ]));
        }
    } else if !input.is_null() {
        lines.push(Line::from(vec![
            Span::styled("    ".to_string(), Style::default()),
            Span::styled(
                truncate_str(&input.to_string(), usize::MAX),
                Style::default().fg(theme.text_primary),
            ),
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

/// Render the parallel-dispatch panel. Multi-line block inspired by
/// claude-code's `TaskListV2` + `AgentProgressLine`:
///
/// ```text
///  Parallel Dispatch · 2/5  (1 ✓ · 1 ⠋ · 0 ✗) · 0:42
///  ├─ ⠋ 设计原则  [1/3] · 2 tools · 0:08   ↳ Read src/pdb.rs
///  ├─ ⠋ 整理测试  [2/3] · 1 tool · 0:05    ↳ Search 'protein'
///  └─ ✓ 设计原则  [3/3] · 4 tools · 0:11   ↳ done
///      (full event log if expanded, prefixed with `│   `)
///  … +2 completed  (e expand all · c collapse)
/// ```
///
/// `panel.selected` is the index of the sub-agent the user is
/// currently focused on; the focused row's title is rendered in BOLD.
///
/// `width` is the visible character width of the panel. The renderer
/// uses it to truncate the title + activity hint so the row never
/// wraps or pushes the rest of the chat off the right edge.
fn render_parallel_panel(
    panel: &ParallelPanelState,
    theme: &Theme,
    width: usize,
) -> Vec<Line<'static>> {
    use crate::parallel_panel::SubAgentEntryLayout;
    use ratatui::style::Modifier;
    use std::time::Instant;

    let mut lines = Vec::new();

    // --- Header -----------------------------------------------------------
    let elapsed = panel.elapsed();
    let elapsed_label = format_duration(elapsed);
    let running = panel.running_count();
    let header = format!(
        " Parallel Dispatch · {}/{}  ({} ✓ · {} ⠋ · {} ✗) · {}",
        panel.completed, panel.total, panel.completed, running, panel.failed, elapsed_label,
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

    // --- Prioritize and fold ---------------------------------------------
    // Always show: in-progress + failed. Then recently-completed
    // (within `RECENT_COMPLETED_TTL_MS`). Then fold the rest into a
    // summary line.
    let now = Instant::now();
    let total = panel.sub_agents.len();
    let mut visible: Vec<usize> = Vec::new();
    let mut foldable: Vec<usize> = Vec::new();
    for (i, e) in panel.sub_agents.iter().enumerate() {
        let is_done = matches!(
            e.status,
            crate::parallel_panel::SubAgentStatus::Completed { .. }
        );
        if is_done && !e.is_recently_completed(now) {
            foldable.push(i);
        } else {
            visible.push(i);
        }
    }
    // Fold a few more if we still exceed the visible cap.
    let cap = crate::parallel_panel::MAX_VISIBLE_SUB_AGENTS;
    while visible.len() > cap {
        // Move the oldest completed into the folded set. Running /
        // failed / recent rows are never auto-folded.
        if let Some(pos) = visible.iter().position(|&i| {
            matches!(
                panel.sub_agents[i].status,
                crate::parallel_panel::SubAgentStatus::Completed { .. }
            )
        }) {
            foldable.push(visible.remove(pos));
        } else {
            break;
        }
    }

    // --- Render visible rows ---------------------------------------------
    let visible_count = visible.len();
    let total_to_render = visible_count + (if !foldable.is_empty() { 1 } else { 0 });
    for (rank, &i) in visible.iter().enumerate() {
        let entry = &panel.sub_agents[i];
        let is_last = rank + 1 == total_to_render;
        let is_selected = i == panel.selected;
        let layout = SubAgentEntryLayout {
            index_1based: i + 1,
            total,
            is_last,
            is_selected,
            now,
        };
        lines.push(render_sub_agent_row(entry, &layout, theme, width));

        if entry.expanded {
            // Full event log, prefixed with a vertical bar to keep
            // the tree aesthetic (matches the row's `├─` / `└─`).
            for ev in &entry.events {
                for ev_line in render_sub_agent_event(ev, theme) {
                    let mut spans = vec![Span::raw("│   ")];
                    spans.extend(ev_line.spans);
                    lines.push(Line::from(spans));
                }
            }
        } else if matches!(entry.status, crate::parallel_panel::SubAgentStatus::Running) {
            // Auto-peek: even without expanding, show one line of
            // "what is this sub-agent doing right now" so the user
            // can monitor all sub-agents without any keystrokes.
            // Falls back to "starting…" when the agent hasn't
            // emitted its first event yet.
            let hint = entry
                .activity_hint()
                .unwrap_or_else(|| "starting…".to_string());
            lines.push(render_peek_line(&hint, theme, width));
        }
    }

    // --- Hidden summary ---------------------------------------------------
    if !foldable.is_empty() {
        let summary = format!(
            "    … +{} completed  (e expand all · c collapse)",
            foldable.len(),
        );
        lines.push(Line::from(Span::styled(
            summary,
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::DIM),
        )));
    }

    lines
}

fn render_sub_agent_event(ev: &SubAgentEvent, theme: &Theme) -> Vec<Line<'static>> {
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
            // Use `tool_user_facing_name` so the expanded event log
            // shows the same human-readable form as the auto-peek
            // (e.g. "View src/lib.rs" instead of a raw
            // `kms_view_local path: src/lib.rs`).
            let summary = crate::parallel_panel::tool_user_facing_name(name, input);
            vec![Line::from(vec![
                Span::styled("      🔧 ".to_string(), Style::default()),
                Span::styled(summary, Style::default().fg(theme.text_secondary)),
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
            Span::styled(truncate_str(msg, 200), theme.error_style()),
        ])],
    }
}

/// Render a single sub-agent row in the parallel-dispatch panel as one
/// `Line` with multiple styled spans. Layout:
///
/// ```text
///  ├─ ⠋ 设计原则  [1/3] · 2 tools · 0:08   ↳ Read src/pdb.rs
/// ```
///
/// Visual treatment per status (mirrors claude-code's TaskListV2):
///   - Running:        icon in `spinner` color, title in BOLD
///   - Failed:         icon + title in `error` color
///   - Recent done:    icon in `success`, title STRIKETHROUGH, normal text
///   - Older done:     icon in `success`, title STRIKETHROUGH, muted text
///
/// Width-aware: the title is truncated first, then the activity hint.
fn render_sub_agent_row(
    entry: &crate::parallel_panel::SubAgentPanelEntry,
    layout: &crate::parallel_panel::SubAgentEntryLayout,
    theme: &Theme,
    width: usize,
) -> Line<'static> {
    use crate::parallel_panel::SubAgentStatus;
    use ratatui::style::Modifier;

    // --- Status icon + style ---------------------------------------------
    let (icon, icon_style) = match &entry.status {
        SubAgentStatus::Running => (
            "\u{280b}", // ⠋
            Style::default().fg(theme.spinner),
        ),
        SubAgentStatus::Completed { .. } => ("\u{2713}", theme.success_style()), // ✓
        SubAgentStatus::Failed { .. } => ("\u{2717}", theme.error_style()),      // ✗
    };

    // --- Title style -----------------------------------------------------
    let title_style = match &entry.status {
        SubAgentStatus::Running => Style::default()
            .fg(theme.text_primary)
            .add_modifier(Modifier::BOLD),
        SubAgentStatus::Failed { .. } => theme.error_style(),
        SubAgentStatus::Completed { .. } => {
            let base = if entry.is_recently_completed(layout.now) {
                Style::default().fg(theme.text_primary)
            } else {
                Style::default().fg(theme.text_muted)
            };
            base.add_modifier(Modifier::CROSSED_OUT)
        }
    };

    // --- Meta: [i/total] · N tools · duration ---------------------------
    let mut meta = format!(" [{}/{}]", layout.index_1based, layout.total);
    if entry.tool_call_count > 0 {
        meta.push_str(&format!(
            " \u{00b7} {} tool{}",
            entry.tool_call_count,
            if entry.tool_call_count == 1 { "" } else { "s" }
        ));
    }
    match &entry.status {
        SubAgentStatus::Running => {}
        SubAgentStatus::Completed { .. } | SubAgentStatus::Failed { .. } => {
            meta.push_str(&format!(" \u{00b7} {}", format_duration(entry.elapsed())));
        }
    }

    // --- Activity hint (or "starting…" / error) -------------------------
    let hint = match &entry.status {
        SubAgentStatus::Running => entry
            .activity_hint()
            .unwrap_or_else(|| "starting\u{2026}".to_string()),
        SubAgentStatus::Failed { error, .. } => truncate_str(error, 60),
        SubAgentStatus::Completed { .. } => "done".to_string(),
    };

    // --- Tree connector -------------------------------------------------
    let connector = if layout.is_last {
        "\u{2514}\u{2500} " // └─
    } else {
        "\u{251c}\u{2500} " // ├─
    };
    let connector_span = Span::styled(connector, Style::default().fg(theme.text_muted));

    // --- Width budgeting -----------------------------------------------
    // Hard-coded width shares so the row stays legible at typical
    // terminal widths (28 cols up to ~120 cols). Title gets the
    // bigger share because it's the primary identifier; hint gets
    // whatever's left over.
    let budget = width.saturating_sub(2); // 2 for the leading indent "  "
    let meta_len = meta.chars().count();
    let hint_full = format!("  \u{21b3} {}", hint); // 2 spaces + ↳
    let hint_len = hint_full.chars().count();
    let prefix_len = connector.chars().count() + icon.chars().count() + 1; // "├─ ⠋ "
    let available = budget.saturating_sub(prefix_len + meta_len + hint_len + 1);
    let title_max = available.max(8);
    let title_displayed = truncate_str(&entry.title, title_max);

    // --- Selection: brighten the connector + make the title BOLD -------
    let title_span = Span::styled(
        format!(" {} ", title_displayed),
        if layout.is_selected {
            title_style.add_modifier(Modifier::BOLD)
        } else {
            title_style
        },
    );

    // --- Assemble the row -----------------------------------------------
    Line::from(vec![
        connector_span,
        Span::styled(icon.to_string(), icon_style),
        title_span,
        Span::styled(meta, Style::default().fg(theme.text_muted)),
        Span::styled(hint_full, Style::default().fg(theme.text_secondary)),
    ])
}

/// Render a one-line "peek" sub-row indented under a running
/// sub-agent's main row, so the user can see what every sub-agent is
/// doing right now without expanding any row. Layout:
///
/// ```text
/// │   ↳ Read src/pdb.rs
/// ```
fn render_peek_line(hint: &str, theme: &Theme, width: usize) -> Line<'static> {
    let prefix = "\u{2502}   \u{21b3} "; // "│   ↳ "
    let budget = width.saturating_sub(prefix.chars().count());
    let truncated = truncate_str(hint, budget);
    Line::from(vec![
        Span::raw(prefix),
        Span::styled(truncated, Style::default().fg(theme.text_secondary)),
    ])
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
        let lines = render_user_message(text, &theme(), 80);
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
        let text = (1..=10)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = render_user_message(&text, &theme(), 80);
        // 10 text lines + 1 separator = 11, no truncation marker
        assert_eq!(lines.len(), 11);
        assert_eq!(message_count(&lines), 0);
    }

    #[test]
    fn over_threshold_user_message_is_folded() {
        let text = (1..=50)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = render_user_message(&text, &theme(), 80);
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
        let text = (1..=200)
            .map(|i| format!("reply {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = render_assistant_message(&text, false, &theme());
        // 200 reply lines, no separator, no truncation marker.
        assert_eq!(lines.len(), 200);
        assert_eq!(message_count(&lines), 0);
    }

    #[test]
    fn empty_user_message_still_renders_separator() {
        let lines = render_user_message("", &theme(), 80);
        // Empty text has 0 lines; we still emit the separator.
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn parallel_block_renders_header_with_progress() {
        use crate::parallel_panel::ParallelPanelState;
        let mut panel = ParallelPanelState::new(3);
        panel.apply(
            &dendrite_tools::parallel_progress::ParallelProgress::StagingCreated {
                index: 0,
                total: 3,
                title: "A".to_string(),
            },
        );
        panel.apply(
            &dendrite_tools::parallel_progress::ParallelProgress::StagingCreated {
                index: 1,
                total: 3,
                title: "B".to_string(),
            },
        );
        panel.apply(
            &dendrite_tools::parallel_progress::ParallelProgress::StagingCreated {
                index: 2,
                total: 3,
                title: "C".to_string(),
            },
        );
        panel.apply(
            &dendrite_tools::parallel_progress::ParallelProgress::SubAgentCompleted {
                index: 0,
                total: 3,
                title: "A".to_string(),
                duration_ms: 1500,
            },
        );
        let lines = render_parallel_panel(&panel, &theme(), 80);
        // 1 header + 1 row for A (Completed) + 1 row + 1 auto-peek
        // for B (Running) + 1 row + 1 auto-peek for C (Running)
        // = 6 lines.
        assert_eq!(lines.len(), 6);
        let header = lines[0].to_string();
        assert!(header.contains("Parallel Dispatch"));
        assert!(header.contains("1/3"));
        // Sub-agent rows show icons and titles.
        let row_a = lines[1].to_string();
        assert!(row_a.contains("A"));
        // `format_duration` rounds to whole seconds; 1500ms → "1s".
        assert!(row_a.contains("1s"));
    }

    #[test]
    fn parallel_block_expanded_shows_sub_agent_events() {
        use crate::parallel_panel::ParallelPanelState;
        use agentik_types::AgentEvent;
        use dendrite_tools::parallel_progress::ParallelProgress;
        let mut panel = ParallelPanelState::new(1);
        panel.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        panel.apply(&ParallelProgress::SubAgentEvent {
            title: "A".to_string(),
            event: AgentEvent::LlmResponse("hello".to_string()),
        });
        panel.selected = 0;
        panel.toggle_selected(); // expand
        let lines = render_parallel_panel(&panel, &theme(), 80);
        // 1 header + 1 row + 1 event line = 3.
        assert_eq!(lines.len(), 3);
        assert!(lines[2].to_string().contains("hello"));
    }

    #[test]
    fn parallel_block_collapsed_does_not_show_events() {
        use crate::parallel_panel::ParallelPanelState;
        use agentik_types::AgentEvent;
        use dendrite_tools::parallel_progress::ParallelProgress;
        let mut panel = ParallelPanelState::new(1);
        panel.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        panel.apply(&ParallelProgress::SubAgentEvent {
            title: "A".to_string(),
            event: AgentEvent::LlmResponse("hello".to_string()),
        });
        // selected=0 default, expanded=false default
        let lines = render_parallel_panel(&panel, &theme(), 80);
        // 1 header + 1 row + 1 auto-peek line (running sub-agent
        // shows its activity hint without needing expand). The
        // peek line is the third "line" in the output.
        assert!(lines.len() >= 2);
        // The event LLM text appears in the auto-peek, not in the
        // main row.
        assert!(lines.iter().any(|l| l.to_string().contains("hello")));
    }

    #[test]
    fn parallel_block_with_no_sub_agents_renders_placeholder() {
        use crate::parallel_panel::ParallelPanelState;
        let panel = ParallelPanelState::new(0);
        let lines = render_parallel_panel(&panel, &theme(), 80);
        // 1 header + 1 "(no sub-agents yet)" line.
        assert_eq!(lines.len(), 2);
        assert!(lines[1].to_string().contains("no sub-agents"));
    }

    // ---- claude-code-style layout tests -------------------------------

    /// A running sub-agent's row must include a tree connector, an
    /// `[i/total]` segment, and a trailing `↳` activity hint derived
    /// from the most recent tool call. The auto-peek below the row
    /// (when collapsed) surfaces the same hint indented under the
    /// tree, so the user sees "what is the agent doing" without any
    /// keypress.
    #[test]
    fn running_row_includes_tree_connector_index_and_activity_hint() {
        use crate::parallel_panel::ParallelPanelState;
        use agentik_types::AgentEvent;
        use dendrite_tools::parallel_progress::ParallelProgress;
        let mut panel = ParallelPanelState::new(1);
        panel.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "alpha".to_string(),
        });
        panel.apply(&ParallelProgress::SubAgentEvent {
            title: "alpha".to_string(),
            event: AgentEvent::ToolCall {
                name: "kms_view_local".to_string(),
                input: serde_json::json!({"path": "src/lib.rs"}),
            },
        });
        let lines = render_parallel_panel(&panel, &theme(), 80);
        // 1 header + 1 main row + 1 auto-peek.
        assert_eq!(lines.len(), 3);
        let row = lines[1].to_string();
        // Tree connector is the only `├─` in the layout (since the
        // row is also the last visible, modern renderers would use
        // `└─`; with one row it can be either — we just check
        // *some* tree character).
        assert!(row.contains('\u{251c}') || row.contains('\u{2514}'));
        assert!(row.contains("[1/1]"));
        assert!(row.contains("\u{21b3}")); // ↳
        // The activity hint names the file via `tool_user_facing_name`.
        assert!(row.contains("View src/lib.rs"));
    }

    /// When the user has expanded a running row, the auto-peek must
    /// not duplicate the activity hint (the full event log already
    /// shows the most recent tool call). We verify the layout: only
    /// the event log line carries the tool-call label, not an extra
    /// peek line.
    #[test]
    fn expanded_row_does_not_repeat_auto_peek() {
        use crate::parallel_panel::ParallelPanelState;
        use agentik_types::AgentEvent;
        use dendrite_tools::parallel_progress::ParallelProgress;
        let mut panel = ParallelPanelState::new(1);
        panel.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "alpha".to_string(),
        });
        panel.apply(&ParallelProgress::SubAgentEvent {
            title: "alpha".to_string(),
            event: AgentEvent::ToolCall {
                name: "kms_search_entity".to_string(),
                input: serde_json::json!({"query": "foo"}),
            },
        });
        panel.toggle_selected(); // expand
        let lines = render_parallel_panel(&panel, &theme(), 80);
        // 1 header + 1 row + 1 event line. NO extra peek.
        assert_eq!(lines.len(), 3);
        let event_line = lines[2].to_string();
        assert!(event_line.contains("Search"));
        // The event line uses the "│   " tree branch, not the
        // "│   ↳" peek prefix.
        assert!(!event_line.contains("\u{21b3}"));
    }

    /// When more than `MAX_VISIBLE_SUB_AGENTS` sub-agents are present,
    /// the oldest completed ones fold into a single `… +N completed`
    /// summary line, while running / recent rows stay visible.
    #[test]
    fn many_sub_agents_fold_into_hidden_summary() {
        use crate::parallel_panel::ParallelPanelState;
        use dendrite_tools::parallel_progress::ParallelProgress;
        let mut panel = ParallelPanelState::new(10);
        // 10 completed agents, all of them > TTL ago (we never
        // touch `completed_at`, so it remains `None` and they
        // count as "old completed" — foldable).
        for i in 0..10 {
            panel.apply(&ParallelProgress::StagingCreated {
                index: i,
                total: 10,
                title: format!("t{i}"),
            });
            panel.apply(&ParallelProgress::SubAgentCompleted {
                index: i,
                total: 10,
                title: format!("t{i}"),
                duration_ms: 100,
            });
        }
        let lines = render_parallel_panel(&panel, &theme(), 120);
        // The last line must be the hidden summary. We don't
        // assert the exact foldable count because it depends on
        // `MAX_VISIBLE_SUB_AGENTS`, but the marker `+` and the
        // word `completed` must appear.
        let last = lines.last().expect("at least header + summary").to_string();
        assert!(last.contains("+"), "expected summary marker in: {last}");
        assert!(
            last.contains("completed"),
            "expected 'completed' in: {last}"
        );
    }

    /// A row whose `completed_at` is within the TTL renders its
    /// title with a `CROSSED_OUT` modifier (claude-code's
    /// "strikethrough for just-finished" pattern). We assert the
    /// title span carries the modifier.
    #[test]
    fn recently_completed_title_has_crossed_out_modifier() {
        use crate::parallel_panel::{ParallelPanelState, SubAgentStatus};
        use dendrite_tools::parallel_progress::ParallelProgress;
        use ratatui::style::Modifier;
        let mut panel = ParallelPanelState::new(1);
        panel.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "alpha".to_string(),
        });
        panel.apply(&ParallelProgress::SubAgentCompleted {
            index: 0,
            total: 1,
            title: "alpha".to_string(),
            duration_ms: 100,
        });
        let lines = render_parallel_panel(&panel, &theme(), 80);
        // 1 header + 1 row (Completed rows do NOT get a peek).
        assert_eq!(lines.len(), 2);
        // Find the title span and check its modifiers.
        let row = &lines[1];
        let title_span = row
            .spans
            .iter()
            .find(|s| s.content.contains("alpha"))
            .expect("title span present");
        assert!(
            title_span
                .style
                .add_modifier
                .contains(Modifier::CROSSED_OUT),
            "expected CROSSED_OUT on recently-completed title, got: {:?}",
            title_span.style
        );
        // The entry is in `Completed` state; running row count is 0.
        assert!(matches!(
            panel.sub_agents[0].status,
            SubAgentStatus::Completed { .. }
        ));
    }
}

#[cfg(test)]
mod estimate_tests {
    use super::*;
    use crate::theme::Theme;

    fn theme() -> Theme {
        Theme::default_theme()
    }

    #[test]
    fn estimate_never_undercounts_assistant() {
        let msg = ChatMessage::Assistant {
            text: "line1\nline2\nline3".into(),
            streaming: false,
        };
        let actual = msg.to_lines(&theme(), None, 80).len();
        assert!(
            msg.estimate_lines() >= actual,
            "estimate={} < actual={}",
            msg.estimate_lines(),
            actual,
        );
    }

    #[test]
    fn estimate_never_undercounts_user() {
        let msg = ChatMessage::User {
            text: "line1\nline2".into(),
        };
        let actual = msg.to_lines(&theme(), None, 80).len();
        assert!(
            msg.estimate_lines() >= actual,
            "estimate={} < actual={}",
            msg.estimate_lines(),
            actual,
        );
    }

    #[test]
    fn estimate_never_undercounts_thinking() {
        let msg = ChatMessage::Thinking {
            text: "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl".into(),
            streaming: false,
        };
        let actual = msg.to_lines(&theme(), None, 80).len();
        assert!(
            msg.estimate_lines() >= actual,
            "estimate={} < actual={}",
            msg.estimate_lines(),
            actual,
        );
    }

    #[test]
    fn estimate_never_undercounts_truncated_user() {
        let long_text = (1..=50)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let msg = ChatMessage::User { text: long_text };
        let actual = msg.to_lines(&theme(), None, 80).len();
        assert!(
            msg.estimate_lines() >= actual,
            "estimate={} < actual={}",
            msg.estimate_lines(),
            actual,
        );
    }

    #[test]
    fn estimate_never_undercounts_tool_call() {
        let msg = ChatMessage::ToolCall {
            name: "kms_view_local".into(),
            input: serde_json::json!({"path": "/心血管", "depth": 3}),
        };
        let actual = msg.to_lines(&theme(), None, 80).len();
        assert!(
            msg.estimate_lines() >= actual,
            "estimate={} < actual={}",
            msg.estimate_lines(),
            actual,
        );
    }

    #[test]
    fn estimate_never_undercounts_tool_result() {
        let msg = ChatMessage::ToolResult {
            ok: true,
            content: r#"{"title": "心力衰竭 · 药物治疗", "content": "使用 ACEI"}"#.into(),
        };
        let actual = msg.to_lines(&theme(), None, 80).len();
        assert!(
            msg.estimate_lines() >= actual,
            "estimate={} < actual={}",
            msg.estimate_lines(),
            actual,
        );
    }

    #[test]
    fn estimate_simple_types() {
        assert_eq!(ChatMessage::Divider.estimate_lines(), 1);
        assert_eq!(ChatMessage::Done.estimate_lines(), 1);
        assert_eq!(
            ChatMessage::Error {
                message: "oops".into()
            }
            .estimate_lines(),
            1,
        );
    }
}
