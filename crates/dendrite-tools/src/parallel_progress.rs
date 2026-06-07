//! `ParallelProgress` — the side-channel event type that lets
//! `kms_parallel_dispatch` stream sub-agent lifecycle and activity
//! updates to the TUI while a parallel dispatch is in progress.
//!
//! This enum is intentionally decoupled from `agentik_types::AgentUiEvent`
//! (which is reserved for the *orchestrator* agent's events). Sub-agent
//! activity is wrapped as `SubAgentEvent` so the TUI can group events
//! by `title` when rendering the parallel panel.
//!
//! The design follows the opencode TUI's principle of *streaming
//! progress*: the user sees "X of Y started / completed / failed" in
//! real time rather than waiting minutes for a single `ToolResult`.

use agentik_types::AgentUiEvent;

/// Channel type used to forward parallel-dispatch progress to the TUI.
pub type ParallelProgressTx = tokio::sync::mpsc::UnboundedSender<ParallelProgress>;

#[derive(Debug, Clone)]
pub enum ParallelProgress {
    /// The dispatch tool was entered. `total` is the number of sub-tasks.
    DispatchStarted { total: usize },

    /// A staging Group node was created under root for the sub-task.
    StagingCreated {
        index: usize,
        total: usize,
        title: String,
    },

    /// A sub-agent has been spawned and started running.
    SubAgentStarted {
        index: usize,
        total: usize,
        title: String,
    },

    /// A sub-agent emitted an event from its own `event_tx` channel.
    /// `title` is preserved so the TUI can route it to the right
    /// collapsible row in the parallel panel.
    SubAgentEvent { title: String, event: AgentUiEvent },

    /// A sub-agent finished successfully.
    SubAgentCompleted {
        index: usize,
        total: usize,
        title: String,
        duration_ms: u64,
    },

    /// A sub-agent failed (build error, start error, or join error).
    SubAgentFailed {
        index: usize,
        total: usize,
        title: String,
        error: String,
        duration_ms: u64,
    },

    /// A staging area was merged into its target parent in the main tree.
    Merged {
        index: usize,
        total: usize,
        target: String,
        moved: usize,
    },

    /// The whole dispatch tool returned. `elapsed_ms` is wall-clock time
    /// from `DispatchStarted` to this event.
    DispatchFinished {
        total: usize,
        succeeded: usize,
        failed: usize,
        elapsed_ms: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_variants_are_send_and_sync() {
        // Compile-time check: mpsc::UnboundedSender requires Send (and the
        // value type T requires Send + 'static). This test forces the
        // compiler to verify the type satisfies the channel contract.
        fn assert_send_sync<T: Send + Sync + 'static>() {}
        assert_send_sync::<ParallelProgress>();
        assert_send_sync::<ParallelProgressTx>();
    }

    #[test]
    fn variants_are_cloneable() {
        let started = ParallelProgress::DispatchStarted { total: 3 };
        let _copy = started.clone();

        let event = ParallelProgress::SubAgentEvent {
            title: "x".to_string(),
            event: AgentUiEvent::LlmResponse("hi".to_string()),
        };
        let _copy = event.clone();
    }
}
