//! Agent Status Panel — displays all agents managed by `ProcessManager`.
//!
//! Driven by `agentik_core::ProcessEvent` instead of the old
//! `ParallelProgress` mpsc side-channel. Shows a list of managed agents
//! with their status, streaming text, tool calls, and event logs.
//!
//! This is a standalone panel (focusable via Tab), not embedded in the
//! chat history like the old `ParallelBlock`.

use std::time::{Duration, Instant};

use agentik_core::process::ProcessEvent;
use agentik_types::AgentEvent;
use ratatui::Frame;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use serde_json::Value;

use crate::theme::Theme;

// ---- State types -----------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct AgentPanelState {
    pub agents: Vec<AgentPanelEntry>,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub struct AgentPanelEntry {
    pub agent_id: uuid::Uuid,
    pub title: String,
    pub status: AgentEntryStatus,
    pub events: Vec<AgentPanelEvent>,
    pub expanded: bool,
    pub started_at: Instant,
    pub completed_at: Option<Instant>,
    pub tool_call_count: usize,
    pub streaming_text: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AgentEntryStatus {
    Running,
    Completed { duration: Duration },
    Failed { error: String, duration: Duration },
}

#[derive(Debug, Clone)]
pub enum AgentPanelEvent {
    LlmResponse(String),
    ToolCall { name: String, input: Value },
    ToolResult { ok: bool, content: String },
    Error(String),
}

const MAX_EVENTS_PER_AGENT: usize = 20;
pub const MAX_VISIBLE_AGENTS: usize = 8;
pub const RECENT_COMPLETED_TTL_MS: u64 = 30_000;

// ---- Layout hints for row rendering -----------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct AgentEntryLayout {
    pub index_1based: usize,
    pub total: usize,
    pub is_last: bool,
    pub is_selected: bool,
    pub now: Instant,
}

// ---- State machine --------------------------------------------------------

impl AgentPanelState {
    /// Apply a `ProcessEvent` from `ProcessManager` to the panel state.
    pub fn apply_process_event(&mut self, event: &ProcessEvent) {
        match event {
            ProcessEvent::Agent {
                agent_id,
                event: ui_event,
            } => {
                if let Some(entry) = self.agents.iter_mut().find(|e| e.agent_id == *agent_id) {
                    apply_agent_event(entry, ui_event);
                }
            }
            ProcessEvent::StateChanged { agent_id, .. } => {
                // Register new agents we haven't seen yet.
                let known = self.agents.iter().any(|e| e.agent_id == *agent_id);
                if !known {
                    // Will be given a proper title by the caller
                    // (from `agent_titles` map). Use UUID prefix as
                    // fallback.
                    let title = format!("Agent {}", &agent_id.to_string()[..8]);
                    self.add_agent(*agent_id, title);
                }
            }
            ProcessEvent::ProcessExited { agent_id, status } => {
                if let Some(entry) = self.agents.iter_mut().find(|e| e.agent_id == *agent_id) {
                    let duration = entry.started_at.elapsed();
                    match status {
                        agentik_core::process::ProcessExitStatus::Completed => {
                            entry.status = AgentEntryStatus::Completed { duration };
                        }
                        agentik_core::process::ProcessExitStatus::Error(msg)
                        | agentik_core::process::ProcessExitStatus::Panicked(msg) => {
                            entry.status = AgentEntryStatus::Failed {
                                error: msg.clone(),
                                duration,
                            };
                        }
                        agentik_core::process::ProcessExitStatus::Cancelled
                        | agentik_core::process::ProcessExitStatus::Stopped => {
                            entry.status = AgentEntryStatus::Completed { duration };
                        }
                    }
                    entry.completed_at = Some(Instant::now());
                }
            }
        }
    }

    pub fn add_agent(&mut self, agent_id: uuid::Uuid, title: String) {
        // Don't duplicate.
        if self.agents.iter().any(|e| e.agent_id == agent_id) {
            // Update the title if we now have a better one.
            if let Some(e) = self.agents.iter_mut().find(|e| e.agent_id == agent_id)
                && !title.starts_with("Agent ")
            {
                e.title = title;
            }
            return;
        }
        self.agents.push(AgentPanelEntry {
            agent_id,
            title,
            status: AgentEntryStatus::Running,
            events: Vec::new(),
            expanded: false,
            started_at: Instant::now(),
            completed_at: None,
            tool_call_count: 0,
            streaming_text: None,
        });
    }

    pub fn running_count(&self) -> usize {
        self.agents
            .iter()
            .filter(|e| matches!(e.status, AgentEntryStatus::Running))
            .count()
    }

    pub fn completed_count(&self) -> usize {
        self.agents
            .iter()
            .filter(|e| matches!(e.status, AgentEntryStatus::Completed { .. }))
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.agents
            .iter()
            .filter(|e| matches!(e.status, AgentEntryStatus::Failed { .. }))
            .count()
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.agents.is_empty() {
            return;
        }
        let len = self.agents.len() as isize;
        let cur = self.selected as isize;
        let next = (cur + delta).clamp(0, len - 1);
        self.selected = next as usize;
    }

    pub fn toggle_selected(&mut self) {
        if let Some(entry) = self.agents.get_mut(self.selected) {
            entry.expanded = !entry.expanded;
        }
    }

    pub fn expand_all(&mut self) {
        for e in self.agents.iter_mut() {
            e.expanded = true;
        }
    }

    pub fn collapse_all(&mut self) {
        for e in self.agents.iter_mut() {
            e.expanded = false;
        }
    }

    /// Remove completed/failed agents older than the TTL.
    #[allow(dead_code)]
    pub fn prune_old(&mut self) {
        let now = Instant::now();
        self.agents.retain(|e| {
            !matches!(
                e.completed_at,
                Some(t) if now.duration_since(t).as_millis() > RECENT_COMPLETED_TTL_MS as u128 * 3
            )
        });
        // Keep selected in bounds.
        if !self.agents.is_empty() && self.selected >= self.agents.len() {
            self.selected = self.agents.len() - 1;
        }
    }
}

fn apply_agent_event(entry: &mut AgentPanelEntry, event: &AgentEvent) {
    match event {
        AgentEvent::ToolCall { .. } => {
            entry.tool_call_count += 1;
            if let Some(mapped) = map_agent_event(event) {
                entry.events.push(mapped);
            }
        }
        AgentEvent::TextDelta(token) => {
            entry
                .streaming_text
                .get_or_insert_with(String::new)
                .push_str(token);
        }
        AgentEvent::LlmResponse(text) => {
            entry.streaming_text = None;
            if !text.is_empty() {
                entry
                    .events
                    .push(AgentPanelEvent::LlmResponse(text.clone()));
            }
        }
        _ => {
            if let Some(mapped) = map_agent_event(event) {
                entry.events.push(mapped);
            }
        }
    }
    if entry.events.len() > MAX_EVENTS_PER_AGENT {
        let drop_n = entry.events.len() - MAX_EVENTS_PER_AGENT;
        entry.events.drain(0..drop_n);
    }
}

fn map_agent_event(event: &AgentEvent) -> Option<AgentPanelEvent> {
    match event {
        AgentEvent::LlmResponse(text) => Some(AgentPanelEvent::LlmResponse(text.clone())),
        AgentEvent::ToolCall { name, input } => Some(AgentPanelEvent::ToolCall {
            name: name.clone(),
            input: input.clone(),
        }),
        AgentEvent::ToolResult { ok, content } => Some(AgentPanelEvent::ToolResult {
            ok: *ok,
            content: content.clone(),
        }),
        AgentEvent::Error(msg) => Some(AgentPanelEvent::Error(msg.clone())),
        AgentEvent::Thinking(_) => None,
        AgentEvent::Requesting | AgentEvent::Done => None,
        AgentEvent::TextDelta(_)
        | AgentEvent::ThinkingDelta(_)
        | AgentEvent::UsageUpdate { .. }
        | AgentEvent::StreamStart { .. }
        | AgentEvent::ContentBlockStart { .. }
        | AgentEvent::ContentBlockStop { .. }
        | AgentEvent::StreamDelta { .. } => None,
    }
}

// ---- Per-entry helpers (migrated from parallel_panel.rs) -------------------

impl AgentPanelEntry {
    pub fn elapsed(&self) -> Duration {
        match &self.status {
            AgentEntryStatus::Running => Duration::ZERO,
            AgentEntryStatus::Completed { duration }
            | AgentEntryStatus::Failed { duration, .. } => *duration,
        }
    }

    pub fn is_recently_completed(&self, now: Instant) -> bool {
        matches!(
            self.completed_at,
            Some(t) if now.duration_since(t).as_millis() < RECENT_COMPLETED_TTL_MS as u128
        )
    }

    pub fn activity_hint(&self) -> Option<String> {
        if matches!(self.status, AgentEntryStatus::Failed { .. }) {
            return None;
        }
        if let Some(text) = &self.streaming_text
            && !text.is_empty()
        {
            let char_count = text.chars().count();
            let snippet: String = if char_count > 60 {
                format!(
                    "...{}",
                    text.chars().skip(char_count - 60).collect::<String>()
                )
            } else {
                text.clone()
            };
            return Some(snippet);
        }
        for ev in self.events.iter().rev() {
            match ev {
                AgentPanelEvent::ToolCall { name, input } => {
                    return Some(tool_user_facing_name(name, input));
                }
                AgentPanelEvent::ToolResult { ok: true, .. } => {
                    return Some("done".to_string());
                }
                AgentPanelEvent::ToolResult { ok: false, content } => {
                    let snippet = content
                        .lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(40)
                        .collect::<String>();
                    return Some(if snippet.is_empty() {
                        "tool failed".to_string()
                    } else {
                        format!("tool failed: {}", snippet)
                    });
                }
                AgentPanelEvent::Error(msg) => {
                    let snippet = msg.chars().take(40).collect::<String>();
                    return Some(if snippet.is_empty() {
                        "error".to_string()
                    } else {
                        format!("error: {}", snippet)
                    });
                }
                AgentPanelEvent::LlmResponse(s) if !s.is_empty() => {
                    let snippet = s.chars().take(40).collect::<String>();
                    return Some(if s.chars().count() > 40 {
                        format!("{}…", snippet)
                    } else {
                        snippet
                    });
                }
                AgentPanelEvent::LlmResponse(_) => continue,
            }
        }
        None
    }
}

// ---- Tool name rendering (migrated from parallel_panel.rs) ------------------

pub(crate) fn tool_user_facing_name(name: &str, input: &Value) -> String {
    let first_str = |k: &str| -> Option<String> {
        input
            .as_object()
            .and_then(|o| o.get(k))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };
    let first_id = |k: &str| -> Option<String> {
        first_str(k).map(|s| {
            if s.len() > 8 {
                format!("{}…", &s[..8])
            } else {
                s
            }
        })
    };
    let first_kv = || -> Option<String> {
        let obj = input.as_object()?;
        let (k, v) = obj.iter().next()?;
        Some(format!("{}: {}", k, format_value_short(v)))
    };

    match name {
        "kms_view_local" => first_str("path").map(|p| format!("View {}", p)),
        "kms_create_knowledge" => {
            first_str("title").map(|t| format!("Create knowledge \"{}\"", truncate_inline(&t, 30)))
        }
        "kms_update_knowledge" => {
            first_str("id").map(|id| format!("Update knowledge {}", first_id("id").unwrap_or(id)))
        }
        "kms_rename_knowledge" => {
            first_str("title").map(|t| format!("Rename to \"{}\"", truncate_inline(&t, 30)))
        }
        "kms_delete_knowledge" => first_str("id")
            .map(|_| format!("Delete knowledge {}", first_id("id").unwrap_or_default())),
        "kms_get_knowledge" => first_id("id").map(|id| format!("Get knowledge {}", id)),
        "kms_search_entity" => {
            first_str("query").map(|q| format!("Search '{}'", truncate_inline(&q, 30)))
        }
        "kms_search_subtree" => {
            first_str("query").map(|q| format!("Search subtree '{}'", truncate_inline(&q, 30)))
        }
        "kms_get_entity" => first_id("id").map(|id| format!("Get entity {}", id)),
        "kms_get_entity_knowledge" => {
            first_id("entity_id").map(|id| format!("Get entity knowledge {}", id))
        }
        "kms_list_entities" => first_str("entity_type").map(|t| format!("List {} entities", t)),
        "kms_create_entity" => {
            first_str("name").map(|n| format!("Create entity \"{}\"", truncate_inline(&n, 30)))
        }
        "kms_update_entity" => first_id("id").map(|id| format!("Update entity {}", id)),
        "kms_delete_entity" => first_id("id").map(|id| format!("Delete entity {}", id)),
        "kms_create_index" => {
            first_str("title").map(|t| format!("Create group \"{}\"", truncate_inline(&t, 30)))
        }
        "kms_move_index" => first_id("id").map(|id| format!("Move group {}", id)),
        "kms_delete_index" => first_id("id").map(|id| format!("Delete group {}", id)),
        "kms_navigate" => {
            first_str("target").map(|t| format!("Navigate to {}", truncate_inline(&t, 30)))
        }
        "kms_add_nomenclature" => {
            first_str("term").map(|t| format!("Nomenclature +\"{}\"", truncate_inline(&t, 30)))
        }
        "kms_update_nomenclature" => first_id("id").map(|id| format!("Nomenclature update {}", id)),
        "kms_delete_nomenclature" => first_id("id").map(|id| format!("Nomenclature delete {}", id)),
        "kms_link_orphans" => Some("Link orphans".to_string()),
        "kms_reorganize_children" => {
            first_id("parent_id").map(|id| format!("Reorganize children of {}", id))
        }
        "kms_merge_subtree" => {
            first_str("target").map(|t| format!("Merge subtree → {}", truncate_inline(&t, 30)))
        }
        "kms_parallel_dispatch" => first_str("staging_title")
            .map(|t| format!("Dispatch subtask \"{}\"", truncate_inline(&t, 30))),
        _ => first_kv().map(|kv| format!("{} {}", name, kv)),
    }
    .unwrap_or_else(|| name.to_string())
}

fn truncate_inline(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

fn format_value_short(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => truncate_inline(s, 30),
        Value::Array(arr) => format!("[{} items]", arr.len()),
        Value::Object(_) => "{…}".to_string(),
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

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let end = s
        .char_indices()
        .take_while(|(i, _)| *i < max)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(max);
    format!("{}…", &s[..end])
}

// ---- Rendering ------------------------------------------------------------

pub fn render_agent_panel(
    f: &mut Frame,
    state: &crate::agent_panel::AgentPanelState,
    theme: &Theme,
    area: ratatui::layout::Rect,
) {
    let block = Block::default()
        .title(" Agents ")
        .borders(Borders::ALL)
        .border_style(theme.focused_border_style(true));
    let inner = block.inner(area);

    let lines = render_panel_lines(state, theme, area.width as usize);
    let paragraph = ratatui::widgets::Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);

    // We don't scroll the agent panel in this initial implementation.
    let _ = inner; // suppress unused warning
}

fn render_panel_lines(state: &AgentPanelState, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    let running = state.running_count();
    let completed = state.completed_count();
    let failed = state.failed_count();
    let total = state.agents.len();

    let header = format!(
        " Agents · {} total ({} ✓ · {} ⠋ · {} ✗)",
        total, completed, running, failed
    );
    lines.push(Line::from(Span::styled(
        header,
        theme.tool_call_bold_style(),
    )));

    if state.agents.is_empty() {
        lines.push(Line::from(Span::styled(
            "    (no agents)".to_string(),
            ratatui::style::Style::default().fg(theme.text_muted),
        )));
        return lines;
    }

    // Prioritize: running + failed first, then recently completed, then fold old.
    let now = Instant::now();
    let mut visible: Vec<usize> = Vec::new();
    let mut foldable: Vec<usize> = Vec::new();
    for (i, e) in state.agents.iter().enumerate() {
        let is_old_done =
            matches!(e.status, AgentEntryStatus::Completed { .. }) && !e.is_recently_completed(now);
        if is_old_done {
            foldable.push(i);
        } else {
            visible.push(i);
        }
    }
    while visible.len() > MAX_VISIBLE_AGENTS {
        if let Some(pos) = visible
            .iter()
            .position(|&i| matches!(state.agents[i].status, AgentEntryStatus::Completed { .. }))
        {
            foldable.push(visible.remove(pos));
        } else {
            break;
        }
    }

    let visible_count = visible.len();
    let total_to_render = visible_count + if !foldable.is_empty() { 1 } else { 0 };

    for (rank, &i) in visible.iter().enumerate() {
        let entry = &state.agents[i];
        let is_last = rank + 1 == total_to_render;
        let is_selected = i == state.selected;
        let layout = AgentEntryLayout {
            index_1based: i + 1,
            total,
            is_last,
            is_selected,
            now,
        };
        lines.push(render_agent_row(entry, &layout, theme, width));

        if entry.expanded {
            for ev in &entry.events {
                for ev_line in render_panel_event(ev, theme) {
                    let mut spans = vec![Span::raw("│   ")];
                    spans.extend(ev_line.spans);
                    lines.push(Line::from(spans));
                }
            }
        } else if matches!(entry.status, AgentEntryStatus::Running) {
            let hint = entry
                .activity_hint()
                .unwrap_or_else(|| "starting…".to_string());
            lines.push(render_peek_line(&hint, theme, width));
        }
    }

    if !foldable.is_empty() {
        let summary = format!(
            "    … +{} completed  (e expand all · c collapse)",
            foldable.len()
        );
        lines.push(Line::from(Span::styled(
            summary,
            ratatui::style::Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::DIM),
        )));
    }

    lines
}

fn render_agent_row(
    entry: &AgentPanelEntry,
    layout: &AgentEntryLayout,
    theme: &Theme,
    width: usize,
) -> Line<'static> {
    let (icon, icon_style) = match &entry.status {
        AgentEntryStatus::Running => ("⠋", ratatui::style::Style::default().fg(theme.spinner)),
        AgentEntryStatus::Completed { .. } => ("✓", theme.success_style()),
        AgentEntryStatus::Failed { .. } => ("✗", theme.error_style()),
    };

    let title_style = match &entry.status {
        AgentEntryStatus::Running => ratatui::style::Style::default()
            .fg(theme.text_primary)
            .add_modifier(Modifier::BOLD),
        AgentEntryStatus::Failed { .. } => theme.error_style(),
        AgentEntryStatus::Completed { .. } => {
            let base = if entry.is_recently_completed(layout.now) {
                ratatui::style::Style::default().fg(theme.text_primary)
            } else {
                ratatui::style::Style::default().fg(theme.text_muted)
            };
            base.add_modifier(Modifier::CROSSED_OUT)
        }
    };

    let mut meta = format!(" [{}/{}]", layout.index_1based, layout.total);
    if entry.tool_call_count > 0 {
        meta.push_str(&format!(
            " · {} tool{}",
            entry.tool_call_count,
            if entry.tool_call_count == 1 { "" } else { "s" }
        ));
    }
    match &entry.status {
        AgentEntryStatus::Running => {}
        AgentEntryStatus::Completed { .. } | AgentEntryStatus::Failed { .. } => {
            meta.push_str(&format!(" · {}", format_duration(entry.elapsed())));
        }
    }

    let hint = match &entry.status {
        AgentEntryStatus::Running => entry
            .activity_hint()
            .unwrap_or_else(|| "starting…".to_string()),
        AgentEntryStatus::Failed { error, .. } => truncate_str(error, 60),
        AgentEntryStatus::Completed { .. } => "done".to_string(),
    };

    let connector = if layout.is_last { "└─ " } else { "├─ " };
    let connector_span = Span::styled(
        connector,
        ratatui::style::Style::default().fg(theme.text_muted),
    );

    let budget = width.saturating_sub(2);
    let meta_len = meta.chars().count();
    let hint_full = format!("  ↳ {}", hint);
    let hint_len = hint_full.chars().count();
    let prefix_len = connector.chars().count() + icon.chars().count() + 1;
    let available = budget.saturating_sub(prefix_len + meta_len + hint_len + 1);
    let title_max = available.max(8);
    let title_displayed = truncate_str(&entry.title, title_max);

    let title_span = Span::styled(
        format!(" {} ", title_displayed),
        if layout.is_selected {
            title_style.add_modifier(Modifier::BOLD)
        } else {
            title_style
        },
    );

    Line::from(vec![
        connector_span,
        Span::styled(icon.to_string(), icon_style),
        title_span,
        Span::styled(meta, ratatui::style::Style::default().fg(theme.text_muted)),
        Span::styled(
            hint_full,
            ratatui::style::Style::default().fg(theme.text_secondary),
        ),
    ])
}

fn render_panel_event(ev: &AgentPanelEvent, theme: &Theme) -> Vec<Line<'static>> {
    match ev {
        AgentPanelEvent::LlmResponse(s) if !s.is_empty() => {
            vec![Line::from(vec![
                Span::styled("      💬 ".to_string(), ratatui::style::Style::default()),
                Span::styled(
                    truncate_str(s, 200),
                    ratatui::style::Style::default().fg(theme.text_primary),
                ),
            ])]
        }
        AgentPanelEvent::LlmResponse(_) => Vec::new(),
        AgentPanelEvent::ToolCall { name, input } => {
            let summary = tool_user_facing_name(name, input);
            vec![Line::from(vec![
                Span::styled("      🔧 ".to_string(), ratatui::style::Style::default()),
                Span::styled(
                    summary,
                    ratatui::style::Style::default().fg(theme.text_secondary),
                ),
            ])]
        }
        AgentPanelEvent::ToolResult { ok, content } => {
            let (icon, color) = if *ok {
                ("✓", theme.tool_ok)
            } else {
                ("✗", theme.tool_err)
            };
            let summary = if let Ok(val) = serde_json::from_str::<Value>(content) {
                if let Some(s) = val.as_str() {
                    s.to_string()
                } else {
                    format!("{}", val)
                }
            } else {
                truncate_str(content, 80)
            };
            vec![Line::from(vec![
                Span::styled(
                    format!("      {} ", icon),
                    ratatui::style::Style::default().fg(color),
                ),
                Span::styled(
                    summary,
                    ratatui::style::Style::default().fg(theme.text_muted),
                ),
            ])]
        }
        AgentPanelEvent::Error(msg) => vec![Line::from(vec![
            Span::styled("      ✗ ".to_string(), theme.error_style()),
            Span::styled(truncate_str(msg, 200), theme.error_style()),
        ])],
    }
}

fn render_peek_line(hint: &str, theme: &Theme, width: usize) -> Line<'static> {
    let prefix = "│   ↳ ";
    let budget = width.saturating_sub(prefix.chars().count());
    let truncated = truncate_str(hint, budget);
    Line::from(vec![
        Span::raw(prefix),
        Span::styled(
            truncated,
            ratatui::style::Style::default().fg(theme.text_secondary),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_panel() -> AgentPanelState {
        AgentPanelState::default()
    }

    #[test]
    fn add_agent_creates_entry() {
        let mut p = fresh_panel();
        let id = uuid::Uuid::nil();
        p.add_agent(id, "test".to_string());
        assert_eq!(p.agents.len(), 1);
        assert_eq!(p.agents[0].title, "test");
    }

    #[test]
    fn add_agent_deduplicates() {
        let mut p = fresh_panel();
        let id = uuid::Uuid::nil();
        p.add_agent(id, "test".to_string());
        p.add_agent(id, "test2".to_string());
        assert_eq!(p.agents.len(), 1);
        // Title should update to the better one.
        assert_eq!(p.agents[0].title, "test2");
    }

    #[test]
    fn move_selection_clamps() {
        let mut p = fresh_panel();
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        p.add_agent(id1, "A".into());
        p.add_agent(id2, "B".into());
        p.move_selection(5);
        assert_eq!(p.selected, 1);
        p.move_selection(-10);
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn toggle_selected_flips() {
        let mut p = fresh_panel();
        p.add_agent(uuid::Uuid::nil(), "A".into());
        assert!(!p.agents[0].expanded);
        p.toggle_selected();
        assert!(p.agents[0].expanded);
        p.toggle_selected();
        assert!(!p.agents[0].expanded);
    }

    #[test]
    fn tool_user_facing_name_view_local() {
        let s = tool_user_facing_name("kms_view_local", &serde_json::json!({"path": "src/lib.rs"}));
        assert_eq!(s, "View src/lib.rs");
    }

    #[test]
    fn tool_user_facing_name_unknown_falls_back() {
        let s = tool_user_facing_name("kms_unknown", &serde_json::json!({"foo": "bar"}));
        assert!(s.starts_with("kms_unknown"));
    }
}
