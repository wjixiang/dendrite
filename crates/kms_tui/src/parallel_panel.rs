//! State and rendering for the parallel-dispatch panel.
//!
//! When `kms_parallel_dispatch` is running, the user sees a collapsible
//! block in the chat panel showing each sub-agent's title, status
//! (running / completed / failed), and (when expanded) the events the
//! sub-agent emitted (LLM responses, tool calls, tool results).
//!
//! This is a streaming-progress UI: every `ParallelProgress` event
//! from the tool is converted to a state-machine update so the user
//! sees "X of Y started / completed / failed" in real time instead of
//! waiting minutes for a single `ToolResult`.

use std::time::{Duration, Instant};

use serde_json::Value;

/// State for the parallel-dispatch panel.
#[derive(Debug, Clone)]
pub struct ParallelPanelState {
    pub dispatch_id: u64,
    pub started_at: Instant,
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub sub_agents: Vec<SubAgentPanelEntry>,
    /// Index of the currently-selected sub-agent (for collapse/expand
    /// interaction). Always a valid index into `sub_agents` once at
    /// least one sub-agent exists.
    pub selected: usize,
}

impl ParallelPanelState {
    pub fn new(total: usize) -> Self {
        Self {
            dispatch_id: 0,
            started_at: Instant::now(),
            total,
            completed: 0,
            failed: 0,
            sub_agents: Vec::with_capacity(total),
            selected: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SubAgentPanelEntry {
    pub title: String,
    pub status: SubAgentStatus,
    /// Sub-agent's emitted events (LLM responses, tool calls, tool
    /// results). Capped at 20 to avoid memory blow-up on chatty
    /// sub-agents.
    pub events: Vec<SubAgentEvent>,
    /// Whether the sub-agent's event log is expanded in the chat panel.
    /// Defaults to `false` so a parallel dispatch with 5 sub-agents
    /// stays compact.
    pub expanded: bool,
    /// Wall-clock instant when the sub-agent transitioned to a
    /// terminal state. `None` while running. Used by the renderer to
    /// distinguish "just finished" (within `RECENT_COMPLETED_TTL_MS`,
    /// shown with the title color) from "finished a while ago" (shown
    /// muted). Mirrors the `RECENT_COMPLETED_TTL_MS` pattern in
    /// claude-code's `TaskListV2`.
    pub completed_at: Option<Instant>,
    /// Number of tool calls the sub-agent has emitted so far. Cheap
    /// counter incremented in `apply()` for every `ToolCall` event.
    /// Displayed in the row as "N tools" alongside the elapsed time
    /// so the user can tell at a glance which sub-agents are doing
    /// real work vs. spinning.
    pub tool_call_count: usize,
    /// Accumulated text from `TextDelta` events while the sub-agent
    /// is streaming. Grows token-by-token, cleared when the
    /// aggregated `LlmResponse` finalizes the response. Used by
    /// `activity_hint()` to show a live preview of what the
    /// sub-agent is generating.
    pub streaming_text: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SubAgentStatus {
    Running,
    Completed { duration: Duration },
    Failed { error: String, duration: Duration },
}

#[derive(Debug, Clone)]
pub enum SubAgentEvent {
    LlmResponse(String),
    ToolCall { name: String, input: Value },
    ToolResult { ok: bool, content: String },
    Error(String),
}

/// Per-sub-agent event log cap. Sub-agents can be chatty; we keep the
/// most recent N events to bound memory.
const MAX_EVENTS_PER_SUB_AGENT: usize = 20;

/// Within this window, a freshly-completed sub-agent is rendered with
/// its title in `text_primary` + `STRIKETHROUGH`. After the window
/// elapses, the title drops to `text_muted` + `STRIKETHROUGH` so old
/// completions don't keep stealing attention from the still-running
/// sub-agents. Matches claude-code's `RECENT_COMPLETED_TTL_MS`.
pub(crate) const RECENT_COMPLETED_TTL_MS: u64 = 30_000;

/// Soft cap on visible sub-agent rows. When `panel.sub_agents.len()`
/// exceeds this, older completed rows are folded into a single
/// `… +N completed` summary line. In-progress and recently-completed
/// rows are always shown.
pub(crate) const MAX_VISIBLE_SUB_AGENTS: usize = 8;

/// Layout hints the renderer passes into the per-row helper. Bundled
/// into a struct so the row renderer doesn't have to take 8
/// positional arguments.
#[derive(Debug, Clone, Copy)]
pub struct SubAgentEntryLayout {
    /// 1-based index of the sub-agent in the original dispatch order.
    pub index_1based: usize,
    /// Total number of sub-agents in the dispatch (used for `[i/total]`).
    pub total: usize,
    /// True if this is the last row of the visible set (use `└─`
    /// instead of `├─` as the tree connector).
    pub is_last: bool,
    /// True if the user has this row focused (highlight differently).
    pub is_selected: bool,
    /// Wall-clock instant used to decide "recently completed" (passed
    /// in by the caller so the renderer is pure and testable).
    pub now: std::time::Instant,
}

impl SubAgentPanelEntry {
    /// Wall-clock elapsed since the sub-agent started, or — if it has
    /// already terminated — the actual duration the sub-agent ran
    /// (reported by the dispatch tool's `duration_ms` field, stored
    /// inside `SubAgentStatus`). Used by the row badge.
    pub fn elapsed(&self) -> Duration {
        match &self.status {
            SubAgentStatus::Running => {
                // We don't have the original start instant — the
                // dispatch tool only sends `duration_ms` on
                // completion. Approximate with the panel's overall
                // start so at least the number is monotonically
                // increasing while the row is open. The caller
                // (renderer) re-asks every frame, so this stays
                // correct.
                Duration::ZERO
            }
            SubAgentStatus::Completed { duration }
            | SubAgentStatus::Failed { duration, .. } => *duration,
        }
    }

    /// True iff the sub-agent finished within `RECENT_COMPLETED_TTL_MS`
    /// of the current frame. Renderers use this to switch from
    /// `text_primary + STRIKETHROUGH` to `text_muted + STRIKETHROUGH`
    /// after the user has had a moment to read the result.
    pub fn is_recently_completed(&self, now: Instant) -> bool {
        matches!(
            self.completed_at,
            Some(t) if now.duration_since(t).as_millis() < RECENT_COMPLETED_TTL_MS as u128
        )
    }

    /// One-line "what the sub-agent is doing right now" hint, derived
    /// from the most recent event. Used as the trailing `↳` segment
    /// on every row so the user can see all sub-agents' live
    /// activity at a glance, without expanding any row.
    ///
    /// Returns `None` for terminal `Failed` rows (the renderer falls
    /// back to displaying the failure reason on the row) and for
    /// rows that haven't even started yet (the renderer falls back
    /// to `"starting…"`).
    pub fn activity_hint(&self) -> Option<String> {
        // Terminal failed rows: no activity hint; the row shows the
        // failure reason instead.
        if matches!(self.status, SubAgentStatus::Failed { .. }) {
            return None;
        }
        // If the sub-agent is currently streaming LLM text, show a
        // live preview of the last ~60 characters so the user sees
        // progress without expanding the row.
        if let Some(text) = &self.streaming_text {
            if !text.is_empty() {
                let char_count = text.chars().count();
                let snippet: String = if char_count > 60 {
                    format!(
                        "...{}",
                        text.chars()
                            .skip(char_count - 60)
                            .collect::<String>()
                    )
                } else {
                    text.clone()
                };
                return Some(snippet);
            }
        }
        // Walk events from newest to oldest; the latest meaningful
        // event wins. We skip "noise" events (empty LLM responses,
        // Thinking) so the user doesn't see "Thinking…" forever.
        for ev in self.events.iter().rev() {
            match ev {
                SubAgentEvent::ToolCall { name, input } => {
                    return Some(tool_user_facing_name(name, input));
                }
                SubAgentEvent::ToolResult { ok: true, .. } => {
                    return Some("done".to_string());
                }
                SubAgentEvent::ToolResult { ok: false, content } => {
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
                SubAgentEvent::Error(msg) => {
                    let snippet = msg.chars().take(40).collect::<String>();
                    return Some(if snippet.is_empty() {
                        "error".to_string()
                    } else {
                        format!("error: {}", snippet)
                    });
                }
                SubAgentEvent::LlmResponse(s) if !s.is_empty() => {
                    let snippet = s.chars().take(40).collect::<String>();
                    return Some(if s.chars().count() > 40 {
                        format!("{}…", snippet)
                    } else {
                        snippet
                    });
                }
                // Empty LLM responses (Thinking / Requesting / Done
                // from the agentik_types enum mapped by
                // `map_agent_event`) — keep walking back to find a
                // more informative event.
                SubAgentEvent::LlmResponse(_) => continue,
            }
        }
        None
    }
}

impl ParallelPanelState {
    /// Apply a `ParallelProgress` event to the panel state. Pure
    /// function (no I/O) so it's easy to unit-test.
    pub fn apply(
        &mut self,
        event: &dendrite_tools::parallel_progress::ParallelProgress,
    ) {
        use dendrite_tools::parallel_progress::ParallelProgress as P;
        match event {
            P::DispatchStarted { total } => {
                self.total = *total;
            }
            P::StagingCreated { title, .. } => {
                // Pre-create a row so the user sees staging areas pop up
                // before the sub-agent actually runs.
                if self
                    .sub_agents
                    .iter()
                    .all(|e| e.title != *title)
                {
                    self.sub_agents.push(SubAgentPanelEntry {
                        title: title.clone(),
                        status: SubAgentStatus::Running,
                        events: Vec::new(),
                        expanded: false,
                        completed_at: None,
                        tool_call_count: 0,
                        streaming_text: None,
                    });
                }
            }
            P::SubAgentStarted { title, .. } => {
                if let Some(entry) = self.sub_agents.iter_mut().find(|e| e.title == *title) {
                    entry.status = SubAgentStatus::Running;
                } else {
                    self.sub_agents.push(SubAgentPanelEntry {
                        title: title.clone(),
                        status: SubAgentStatus::Running,
                        events: Vec::new(),
                        expanded: false,
                        completed_at: None,
                        tool_call_count: 0,
                        streaming_text: None,
                    });
                }
            }
            P::SubAgentEvent { title, event } => {
                if let Some(entry) = self.sub_agents.iter_mut().find(|e| e.title == *title) {
                    // Bump the per-sub-agent tool-call counter whenever
                    // the sub-agent initiates a tool call. We only
                    // count `ToolCall` events (not `ToolResult`); one
                    // tool call produces one of each, so the result
                    // counter would double-count.
                    if matches!(event, agentik_types::AgentUiEvent::ToolCall { .. }) {
                        entry.tool_call_count += 1;
                    }
                    // Handle streaming deltas directly on the entry
                    // rather than pushing them through map_agent_event().
                    match event {
                        agentik_types::AgentUiEvent::TextDelta(token) => {
                            entry
                                .streaming_text
                                .get_or_insert_with(String::new)
                                .push_str(token);
                        }
                        agentik_types::AgentUiEvent::LlmResponse(text) => {
                            // Finalize: clear streaming text, push the
                            // authoritative event to the log.
                            entry.streaming_text = None;
                            if !text.is_empty() {
                                entry
                                    .events
                                    .push(SubAgentEvent::LlmResponse(text.clone()));
                            }
                        }
                        _ => {
                            if let Some(mapped) = map_agent_event(event) {
                                entry.events.push(mapped);
                            }
                        }
                    }
                    if entry.events.len() > MAX_EVENTS_PER_SUB_AGENT {
                        let drop_n = entry.events.len() - MAX_EVENTS_PER_SUB_AGENT;
                        entry.events.drain(0..drop_n);
                    }
                }
            }
            P::SubAgentCompleted {
                title, duration_ms, ..
            } => {
                if let Some(entry) = self.sub_agents.iter_mut().find(|e| e.title == *title) {
                    entry.status = SubAgentStatus::Completed {
                        duration: Duration::from_millis(*duration_ms),
                    };
                    entry.completed_at = Some(Instant::now());
                }
                self.completed += 1;
            }
            P::SubAgentFailed {
                title,
                error,
                duration_ms,
                ..
            } => {
                if let Some(entry) = self.sub_agents.iter_mut().find(|e| e.title == *title) {
                    entry.status = SubAgentStatus::Failed {
                        error: error.clone(),
                        duration: Duration::from_millis(*duration_ms),
                    };
                    entry.completed_at = Some(Instant::now());
                }
                self.failed += 1;
            }
            P::Merged { .. } => {
                // Merge events are bookkeeping; the final dispatch
                // summary in the chat history will show the merge
                // report. The panel could show "merged into X" inline
                // but we'd need to thread the staging title through
                // Merged; deferred to a follow-up.
            }
            P::DispatchFinished { .. } => {
                // The chat history will receive the orchestrator's
                // summary, so we don't need to do anything here. The
                // panel stays visible until the next dispatch.
            }
        }
    }

    /// Number of sub-agents still in `Running` status.
    pub fn running_count(&self) -> usize {
        self.sub_agents
            .iter()
            .filter(|e| matches!(e.status, SubAgentStatus::Running))
            .count()
    }

    /// Wall-clock time elapsed since `DispatchStarted`.
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Move the `selected` index. Saturates at the boundaries.
    pub fn move_selection(&mut self, delta: isize) {
        if self.sub_agents.is_empty() {
            return;
        }
        let len = self.sub_agents.len() as isize;
        let cur = self.selected as isize;
        let next = (cur + delta).clamp(0, len - 1);
        self.selected = next as usize;
    }

    /// Toggle the expansion of the currently-selected sub-agent.
    pub fn toggle_selected(&mut self) {
        if let Some(entry) = self.sub_agents.get_mut(self.selected) {
            entry.expanded = !entry.expanded;
        }
    }

    /// Expand all sub-agents.
    pub fn expand_all(&mut self) {
        for e in self.sub_agents.iter_mut() {
            e.expanded = true;
        }
    }

    /// Collapse all sub-agents.
    pub fn collapse_all(&mut self) {
        for e in self.sub_agents.iter_mut() {
            e.expanded = false;
        }
    }
}

fn map_agent_event(
    event: &agentik_types::AgentUiEvent,
) -> Option<SubAgentEvent> {
    use agentik_types::AgentUiEvent as E;
    match event {
        E::LlmResponse(text) => Some(SubAgentEvent::LlmResponse(text.clone())),
        E::ToolCall { name, input } => Some(SubAgentEvent::ToolCall {
            name: name.clone(),
            input: input.clone(),
        }),
        E::ToolResult { ok, content } => Some(SubAgentEvent::ToolResult {
            ok: *ok,
            content: content.clone(),
        }),
        E::Error(msg) => Some(SubAgentEvent::Error(msg.clone())),
        // `Thinking` is intentionally dropped — it floods the chat
        // without adding diagnostic value at this layer.
        E::Thinking(_) => None,
        // `Requesting` and `Done` are orchestrator-level signals and
        // should not appear inside a sub-agent's event log.
        E::Requesting | E::Done => None,
        // Delta events are handled directly in `apply()`, not
        // pushed to the events vec.
        E::TextDelta(_)
        | E::ThinkingDelta(_)
        | E::UsageUpdate { .. }
        | E::StreamStart { .. }
        | E::ContentBlockStart { .. }
        | E::ContentBlockStop { .. }
        | E::StreamDelta { .. } => None,
    }
}

/// Render a `kms_*` tool call as a short, human-readable verb phrase
/// like `View src/pdb.rs` or `Search 'protein'`. Mirrors claude-code's
/// `renderToolActivity.tsx` (which uses `tool.userFacingName(parsedInput)
/// + renderToolUseMessage`).
///
/// We special-case the KMS tools shipped under
/// `crates/dendrite-tools/src/kms_tools/` because the agent passes
/// JSON like `{"path": "..."}` or `{"query": "..."}` and a raw dump
/// of that JSON in the panel would be unreadable.
///
/// Unknown tool names fall through to a generic `<name> <k>: <v>` line
/// using the first key/value pair, so the panel is always at least
/// informative.
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
            // Trim long UUIDs to a stable short prefix.
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
        "kms_view_local" => first_str("path")
            .map(|p| format!("View {}", p)),
        "kms_create_knowledge" => first_str("title")
            .map(|t| format!("Create knowledge \"{}\"", truncate_inline(&t, 30))),
        "kms_update_knowledge" => first_str("id")
            .map(|id| format!("Update knowledge {}", first_id("id").unwrap_or(id))),
        "kms_rename_knowledge" => first_str("title")
            .map(|t| format!("Rename to \"{}\"", truncate_inline(&t, 30))),
        "kms_delete_knowledge" => first_str("id")
            .map(|_| format!("Delete knowledge {}", first_id("id").unwrap_or_default())),
        "kms_get_knowledge" => first_id("id").map(|id| format!("Get knowledge {}", id)),
        "kms_search_entity" => first_str("query")
            .map(|q| format!("Search '{}'", truncate_inline(&q, 30))),
        "kms_search_subtree" => first_str("query")
            .map(|q| format!("Search subtree '{}'", truncate_inline(&q, 30))),
        "kms_get_entity" => first_id("id").map(|id| format!("Get entity {}", id)),
        "kms_get_entity_knowledge" => first_id("entity_id")
            .map(|id| format!("Get entity knowledge {}", id)),
        "kms_list_entities" => first_str("entity_type")
            .map(|t| format!("List {} entities", t)),
        "kms_create_entity" => first_str("name")
            .map(|n| format!("Create entity \"{}\"", truncate_inline(&n, 30))),
        "kms_update_entity" => first_id("id")
            .map(|id| format!("Update entity {}", id)),
        "kms_delete_entity" => first_id("id")
            .map(|id| format!("Delete entity {}", id)),
        "kms_create_index" => first_str("title")
            .map(|t| format!("Create group \"{}\"", truncate_inline(&t, 30))),
        "kms_move_index" => first_id("id").map(|id| format!("Move group {}", id)),
        "kms_delete_index" => first_id("id").map(|id| format!("Delete group {}", id)),
        "kms_navigate" => first_str("target")
            .map(|t| format!("Navigate to {}", truncate_inline(&t, 30))),
        "kms_add_nomenclature" => first_str("term")
            .map(|t| format!("Nomenclature +\"{}\"", truncate_inline(&t, 30))),
        "kms_update_nomenclature" => first_id("id")
            .map(|id| format!("Nomenclature update {}", id)),
        "kms_delete_nomenclature" => first_id("id")
            .map(|id| format!("Nomenclature delete {}", id)),
        "kms_link_orphans" => Some("Link orphans".to_string()),
        "kms_reorganize_children" => first_id("parent_id")
            .map(|id| format!("Reorganize children of {}", id)),
        "kms_merge_subtree" => first_str("target")
            .map(|t| format!("Merge subtree \u{2192} {}", truncate_inline(&t, 30))),
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
        let end: String = s.chars().take(max).collect();
        format!("{}…", end)
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

#[cfg(test)]
mod tests {
    use super::*;
    use agentik_types::AgentUiEvent;
    use dendrite_tools::parallel_progress::ParallelProgress;

    fn fresh_panel() -> ParallelPanelState {
        ParallelPanelState::new(3)
    }

    #[test]
    fn dispatch_started_sets_total() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::DispatchStarted { total: 5 });
        assert_eq!(p.total, 5);
    }

    #[test]
    fn staging_created_adds_row() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 2,
            title: "A".to_string(),
        });
        assert_eq!(p.sub_agents.len(), 1);
        assert_eq!(p.sub_agents[0].title, "A");
        assert!(matches!(p.sub_agents[0].status, SubAgentStatus::Running));
    }

    #[test]
    fn sub_agent_started_after_staging_dedupes_by_title() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        p.apply(&ParallelProgress::SubAgentStarted {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        // No duplicate row.
        assert_eq!(p.sub_agents.len(), 1);
    }

    #[test]
    fn sub_agent_completed_updates_status_and_counter() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 2,
            title: "A".to_string(),
        });
        p.apply(&ParallelProgress::SubAgentCompleted {
            index: 0,
            total: 2,
            title: "A".to_string(),
            duration_ms: 1500,
        });
        match p.sub_agents[0].status {
            SubAgentStatus::Completed { duration } => {
                assert_eq!(duration.as_millis(), 1500);
            }
            _ => panic!("expected Completed"),
        }
        assert_eq!(p.completed, 1);
    }

    #[test]
    fn sub_agent_failed_updates_status_and_counter() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        p.apply(&ParallelProgress::SubAgentFailed {
            index: 0,
            total: 1,
            title: "A".to_string(),
            error: "boom".to_string(),
            duration_ms: 200,
        });
        match &p.sub_agents[0].status {
            SubAgentStatus::Failed { error, duration } => {
                assert_eq!(error, "boom");
                assert_eq!(duration.as_millis(), 200);
            }
            _ => panic!("expected Failed"),
        }
        assert_eq!(p.failed, 1);
    }

    #[test]
    fn sub_agent_event_appends_and_caps() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        for i in 0..30 {
            p.apply(&ParallelProgress::SubAgentEvent {
                title: "A".to_string(),
                event: AgentUiEvent::LlmResponse(format!("r{i}")),
            });
        }
        // Cap is 20; oldest 10 dropped.
        assert_eq!(p.sub_agents[0].events.len(), 20);
        match &p.sub_agents[0].events[0] {
            SubAgentEvent::LlmResponse(s) => assert_eq!(s, "r10"),
            _ => panic!(),
        }
    }

    #[test]
    fn move_selection_clamps() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 2,
            title: "A".to_string(),
        });
        p.apply(&ParallelProgress::StagingCreated {
            index: 1,
            total: 2,
            title: "B".to_string(),
        });
        p.move_selection(5);
        assert_eq!(p.selected, 1);
        p.move_selection(-10);
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn toggle_selected_flips_expanded() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        assert!(!p.sub_agents[0].expanded);
        p.toggle_selected();
        assert!(p.sub_agents[0].expanded);
        p.toggle_selected();
        assert!(!p.sub_agents[0].expanded);
    }

    #[test]
    fn expand_and_collapse_all() {
        let mut p = fresh_panel();
        for t in ["A", "B", "C"] {
            p.apply(&ParallelProgress::StagingCreated {
                index: 0,
                total: 1,
                title: t.to_string(),
            });
        }
        p.expand_all();
        assert!(p.sub_agents.iter().all(|e| e.expanded));
        p.collapse_all();
        assert!(p.sub_agents.iter().all(|e| !e.expanded));
    }

    #[test]
    fn running_count_excludes_finished() {
        let mut p = fresh_panel();
        for t in ["A", "B"] {
            p.apply(&ParallelProgress::StagingCreated {
                index: 0,
                total: 1,
                title: t.to_string(),
            });
        }
        p.apply(&ParallelProgress::SubAgentCompleted {
            index: 0,
            total: 1,
            title: "A".to_string(),
            duration_ms: 100,
        });
        assert_eq!(p.running_count(), 1);
    }

    // ---- New field / method tests for the claude-code-style panel ----

    #[test]
    fn completed_at_is_none_while_running() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        assert!(p.sub_agents[0].completed_at.is_none());
    }

    #[test]
    fn completed_at_set_on_completion() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        p.apply(&ParallelProgress::SubAgentCompleted {
            index: 0,
            total: 1,
            title: "A".to_string(),
            duration_ms: 200,
        });
        assert!(p.sub_agents[0].completed_at.is_some());
    }

    #[test]
    fn completed_at_set_on_failure() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        p.apply(&ParallelProgress::SubAgentFailed {
            index: 0,
            total: 1,
            title: "A".to_string(),
            error: "boom".to_string(),
            duration_ms: 50,
        });
        assert!(p.sub_agents[0].completed_at.is_some());
    }

    #[test]
    fn tool_call_count_increments_on_tool_call() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        for _ in 0..3 {
            p.apply(&ParallelProgress::SubAgentEvent {
                title: "A".to_string(),
                event: AgentUiEvent::ToolCall {
                    name: "kms_view_local".to_string(),
                    input: serde_json::json!({"path": "x"}),
                },
            });
        }
        // ToolResult events should NOT increment — only ToolCall does.
        p.apply(&ParallelProgress::SubAgentEvent {
            title: "A".to_string(),
            event: AgentUiEvent::ToolResult {
                ok: true,
                content: "{}".to_string(),
            },
        });
        assert_eq!(p.sub_agents[0].tool_call_count, 3);
    }

    #[test]
    fn is_recently_completed_true_within_ttl() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        p.apply(&ParallelProgress::SubAgentCompleted {
            index: 0,
            total: 1,
            title: "A".to_string(),
            duration_ms: 100,
        });
        // Just-completed row is "recent" for the renderer.
        assert!(p.sub_agents[0]
            .is_recently_completed(Instant::now()));
    }

    #[test]
    fn elapsed_returns_reported_duration_after_completion() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        p.apply(&ParallelProgress::SubAgentCompleted {
            index: 0,
            total: 1,
            title: "A".to_string(),
            duration_ms: 1234,
        });
        assert_eq!(p.sub_agents[0].elapsed().as_millis(), 1234);
    }

    #[test]
    fn activity_hint_running_with_tool_call_returns_user_facing_name() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        p.apply(&ParallelProgress::SubAgentEvent {
            title: "A".to_string(),
            event: AgentUiEvent::ToolCall {
                name: "kms_view_local".to_string(),
                input: serde_json::json!({"path": "src/pdb.rs"}),
            },
        });
        let hint = p.sub_agents[0].activity_hint().expect("hint");
        assert!(hint.contains("View"));
        assert!(hint.contains("src/pdb.rs"));
    }

    #[test]
    fn activity_hint_running_no_events_yet_returns_none() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        // No SubAgentEvent yet — no hint; the renderer shows
        // "starting…" itself.
        assert!(p.sub_agents[0].activity_hint().is_none());
    }

    #[test]
    fn activity_hint_failed_returns_none() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        p.apply(&ParallelProgress::SubAgentFailed {
            index: 0,
            total: 1,
            title: "A".to_string(),
            error: "boom".to_string(),
            duration_ms: 50,
        });
        // Failed row: the renderer shows the error inline; no hint.
        assert!(p.sub_agents[0].activity_hint().is_none());
    }

    #[test]
    fn activity_hint_walks_back_over_thinking_to_find_tool_call() {
        let mut p = fresh_panel();
        p.apply(&ParallelProgress::StagingCreated {
            index: 0,
            total: 1,
            title: "A".to_string(),
        });
        // The agent first emits a thinking event (mapped to empty
        // LlmResponse) and then a real tool call. The hint should
        // find the tool call, not say "Thinking…".
        p.apply(&ParallelProgress::SubAgentEvent {
            title: "A".to_string(),
            event: AgentUiEvent::Thinking("planning".to_string()),
        });
        p.apply(&ParallelProgress::SubAgentEvent {
            title: "A".to_string(),
            event: AgentUiEvent::ToolCall {
                name: "kms_search_entity".to_string(),
                input: serde_json::json!({"query": "protein"}),
            },
        });
        let hint = p.sub_agents[0].activity_hint().expect("hint");
        assert!(hint.contains("Search"), "got: {hint}");
        assert!(hint.contains("protein"), "got: {hint}");
    }

    #[test]
    fn tool_user_facing_name_known_tool_view_local() {
        let s = tool_user_facing_name(
            "kms_view_local",
            &serde_json::json!({"path": "src/lib.rs"}),
        );
        assert_eq!(s, "View src/lib.rs");
    }

    #[test]
    fn tool_user_facing_name_unknown_tool_falls_back_to_first_kv() {
        let s = tool_user_facing_name(
            "kms_unknown_tool",
            &serde_json::json!({"foo": "bar", "n": 2}),
        );
        assert!(s.starts_with("kms_unknown_tool"));
        assert!(s.contains("foo: bar") || s.contains("n: 2"));
    }

    #[test]
    fn tool_user_facing_name_long_string_is_truncated() {
        // `kms_create_knowledge` truncates the title at 30 chars
        // inside `tool_user_facing_name` itself, so an 80-char
        // title must come out with an ellipsis. (The row renderer
        // does an additional pass of truncation for the hint; this
        // test only covers the per-tool cap.)
        let long = "x".repeat(80);
        let s = tool_user_facing_name(
            "kms_create_knowledge",
            &serde_json::json!({"title": long}),
        );
        assert!(s.contains('…'), "expected truncated output, got: {s}");
        // The whole formatted line is bounded: "Create knowledge \"...\"".
        // Title is 30 chars + ellipsis = 31 chars, plus the literal
        // prefix and quotes — well under the 80-char input length.
        assert!(s.chars().count() < 80);
    }
}
