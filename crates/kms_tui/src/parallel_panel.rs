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
                    });
                }
            }
            P::SubAgentEvent { title, event } => {
                if let Some(entry) = self.sub_agents.iter_mut().find(|e| e.title == *title) {
                    let mapped = map_agent_event(event);
                    entry.events.push(mapped);
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
) -> SubAgentEvent {
    use agentik_types::AgentUiEvent as E;
    match event {
        E::LlmResponse(text) => SubAgentEvent::LlmResponse(text.clone()),
        E::ToolCall { name, input } => SubAgentEvent::ToolCall {
            name: name.clone(),
            input: input.clone(),
        },
        E::ToolResult { ok, content } => SubAgentEvent::ToolResult {
            ok: *ok,
            content: content.clone(),
        },
        E::Error(msg) => SubAgentEvent::Error(msg.clone()),
        // `Thinking` is intentionally dropped — it floods the chat
        // without adding diagnostic value at this layer.
        E::Thinking(_) => SubAgentEvent::LlmResponse(String::new()),
        // `Requesting` and `Done` are orchestrator-level signals and
        // should not appear inside a sub-agent's event log; map them
        // to a no-op text event so the type still has a variant.
        E::Requesting | E::Done => SubAgentEvent::LlmResponse(String::new()),
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
}
