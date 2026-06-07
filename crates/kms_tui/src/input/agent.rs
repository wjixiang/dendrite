use std::sync::Arc;

use agent_compose::KmsContext;
use agent_knowledge::KnowledgeContext;
use agentik_types::AgentUiEvent;
use agentik_types::messages::ContentBlock;

use crate::chat::ChatMessage;
use crate::state::{AgentKind, App};

/// Rebuild all three agents with the given model pool and service reference.
pub async fn rebuild_all_agents(app: &mut App, pool: &Arc<agentik_sdk::model::model_pool::ModelPool>) {
    let svc = app.svc.clone();
    let svc_arc = Arc::new(svc);

    let compose_ctx: Arc<dyn agentik_core::context::AgentContext> =
        Arc::new(KmsContext::new(svc_arc.clone()));
    let knowledge_ctx: Arc<dyn agentik_core::context::AgentContext> =
        Arc::new(KnowledgeContext::new(svc_arc.clone()));
    let parallel_ctx: Arc<dyn agentik_core::context::AgentContext> = Arc::new(
        agent_compose::ParallelComposeContext::new(
            svc_arc,
            pool.clone(),
            app.parallel_progress_tx.clone(),
        ),
    );

    let mut last_error = None;

    for (kind, ctx) in [
        (AgentKind::Compose, compose_ctx),
        (AgentKind::Knowledge, knowledge_ctx),
        (AgentKind::Parallel, parallel_ctx),
    ] {
        match agentik_core::Agent::builder()
            .with_model_pool(pool.clone())
            .with_context(ctx)
            .build()
            .await
        {
            Ok(agent) => {
                app.agents
                    .insert(kind, Arc::new(tokio::sync::Mutex::new(agent)));
            }
            Err(e) => {
                last_error = Some(format!("{} agent: {}", kind.label(), e));
            }
        }
    }

    if let Some(err) = last_error {
        app.toast.error(format!("Agent rebuild failed: {}", err));
    }
}

pub fn spawn_agent_task(app: &mut App, user_input: String) {
    if app.agent_running {
        return;
    }

    // Snapshot the original input (with any paste placeholders still
    // embedded) and the side-channel that maps placeholders to full
    // text. We then drop the side-channel — the agent has what it
    // needs, and the chat history deliberately keeps the compact form.
    let compact_for_history = user_input.clone();
    let mut pastes = std::mem::take(&mut app.agent_pastes);

    // Assemble the full text for the agent by expanding each
    // placeholder once. Placeholders the user edited away are kept
    // in `pastes` in case the user re-inserts them, but normally
    // they'll be discarded at the end of this scope.
    let mut expanded = user_input;
    for (placeholder, full) in &pastes {
        if expanded.contains(placeholder) {
            expanded = expanded.replacen(placeholder, full, 1);
        }
    }
    pastes.retain(|(placeholder, _)| expanded.contains(placeholder));

    let kind = app.agent_kind;
    // Push the COMPACT form (with `[Pasted ~N lines]` placeholders
    // still embedded) to the chat history. This is what mirrors
    // opencode's design: the chat panel shows the placeholder, not
    // the full pasted text. The agent still receives the full text
    // because we expand before injection below.
    app.agent_messages_mut().push(ChatMessage::User {
        text: compact_for_history,
    });
    app.agent_messages_mut().push(ChatMessage::Divider);
    app.agent_running = true;
    // Re-engage auto-scroll on every new submission so the new
    // stream is visible from the first byte. The user can still
    // override with j/k/PgUp/PgDown/End.
    app.agent_auto_scroll = true;
    app.agent_scroll = 0;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    app.agent_event_rx = Some(rx);

    // The agents map is populated in two places: `App::new` (via
    // `main.rs`, only when a pool can be built) and
    // `rebuild_all_agents` (re-run on settings confirm). Both can
    // legitimately leave the map empty or partially populated — for
    // example a custom provider whose first request to the upstream
    // /models endpoint fails, or a transient network blip mid-rebuild.
    // When that happens the user can still navigate to the Agent
    // panel, type a long message, and press Enter; we must not panic
    // on the lookup. Surface a clear error, reset the running flag so
    // the TUI stays interactive, and drop the half-pushed chat entry.
    let agent_arc = match app.agents.get(&kind).cloned() {
        Some(arc) => arc,
        None => {
            // Pop the User + Divider entries we just pushed so the
            // history doesn't accumulate orphaned rows for submissions
            // that never reached the agent.
            if let Some(history) = app.agent_messages_map.get_mut(&kind) {
                history.pop(); // Divider
                history.pop(); // User
            }
            app.agent_running = false;
            app.agent_event_rx = None;
            app.toast.error(format!(
                "{} agent is not available \u{2014} check the model pool in Settings",
                kind.label()
            ));
            return;
        }
    };

    tokio::spawn(async move {
        let mut agent = agent_arc.lock().await;
        agent.event_tx = Some(tx.clone());

        if let Err(e) = agent.inject_message(vec![ContentBlock::Text { text: expanded }]) {
            let _ = tx.send(AgentUiEvent::Error(format!("Inject error: {}", e)));
            let _ = tx.send(AgentUiEvent::Done);
            agent.event_tx = None;
            return;
        }

        if let Err(e) = agent.start().await {
            let _ = tx.send(AgentUiEvent::Error(format!("Agent failed: {}", e)));
        }

        let _ = tx.send(AgentUiEvent::Done);
        agent.event_tx = None;
    });
}
