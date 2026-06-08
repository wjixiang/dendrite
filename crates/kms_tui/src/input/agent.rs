use std::sync::Arc;

use agent_compose::KmsContext;
use agent_knowledge::KnowledgeContext;
use agentik_types::AgentEvent;
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
            app.process_manager.clone(),
            app.agent_titles.clone(),
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

    let compact_for_history = user_input.clone();
    let mut pastes = std::mem::take(&mut app.agent_pastes);

    let mut expanded = user_input;
    for (placeholder, full) in &pastes {
        if expanded.contains(placeholder) {
            expanded = expanded.replacen(placeholder, full, 1);
        }
    }
    pastes.retain(|(placeholder, _)| expanded.contains(placeholder));
    drop(pastes);

    let kind = app.agent_kind;
    app.agent_messages_mut().push(ChatMessage::User {
        text: compact_for_history,
    });
    app.agent_messages_mut().push(ChatMessage::Divider);
    app.agent_running = true;
    app.agent_auto_scroll = true;
    app.agent_scroll = 0;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    app.agent_event_rx = Some(rx);

    let agent_arc = match app.agents.get(&kind).cloned() {
        Some(arc) => arc,
        None => {
            if let Some(history) = app.agent_messages_map.get_mut(&kind) {
                history.pop(); // Divider
                history.pop(); // User
            }
            app.agent_running = false;
            app.agent_event_rx = None;
            app.toast.error(format!(
                "{} agent is not available — check the model pool in Settings",
                kind.label()
            ));
            return;
        }
    };

    tokio::spawn(async move {
        let mut agent = agent_arc.lock().await;
        agent.event_tx = Some(tx.clone());

        if let Err(e) = agent.inject_message(vec![ContentBlock::Text { text: expanded }]) {
            let _ = tx.send(AgentEvent::Error(format!("Inject error: {}", e)));
            let _ = tx.send(AgentEvent::Done);
            agent.event_tx = None;
            return;
        }

        if let Err(e) = agent.start().await {
            let _ = tx.send(AgentEvent::Error(format!("Agent failed: {}", e)));
        }

        let _ = tx.send(AgentEvent::Done);
        agent.event_tx = None;
    });
}
