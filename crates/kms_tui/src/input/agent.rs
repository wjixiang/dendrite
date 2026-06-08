use std::sync::Arc;

use agent_compose::{KmsContext, ParallelComposeContext};
use agent_knowledge::KnowledgeContext;
use agentik_core::context::ContextChanges;
use agentik_types::AgentEvent;
use agentik_types::messages::ContentBlock;

use crate::chat::ChatMessage;
use crate::state::{AgentKind, App};

/// Rebuild all three agents with the given model pool and service reference.
pub async fn rebuild_all_agents(
    app: &mut App,
    pool: &Arc<agentik_sdk::model::model_pool::ModelPool>,
) {
    let svc = app.svc.clone();
    let svc_arc = Arc::new(svc);

    // Compose agent
    let compose_ctx: Arc<dyn agentik_core::context::AgentContext> =
        Arc::new(KmsContext::new(svc_arc.clone()));
    let _ = compose_ctx.write(ContextChanges::default()).await;
    let compose_tools = dendrite_tools::registrations(svc_arc.clone(), compose_ctx.clone());

    // Knowledge agent
    let knowledge_ctx: Arc<dyn agentik_core::context::AgentContext> =
        Arc::new(KnowledgeContext::new(svc_arc.clone()));
    let _ = knowledge_ctx.write(ContextChanges::default()).await;
    let knowledge_tools = dendrite_tools::readonly_registrations(svc_arc.clone());

    // Parallel agent
    let parallel_ctx: Arc<dyn agentik_core::context::AgentContext> =
        Arc::new(ParallelComposeContext::new(svc_arc.clone()));
    let _ = parallel_ctx.write(ContextChanges::default()).await;

    let sub_factory: Arc<
        dyn Fn(
                Arc<kms::KmsService>,
                Arc<agentik_sdk::model::model_pool::ModelPool>,
            ) -> dendrite_tools::SubAgentConfig
            + Send
            + Sync,
    > = Arc::new(|sub_svc, _pool| {
        let ctx = Arc::new(agent_compose::SubTreeComposeContext::new(sub_svc.clone()));
        dendrite_tools::SubAgentConfig {
            context: ctx,
            system_prompt: agent_compose::SUBTREE_COMPOSE_PROMPT,
        }
    });
    let parallel_tools = dendrite_tools::parallel_registrations(
        svc_arc.clone(),
        parallel_ctx.clone(),
        pool.clone(),
        sub_factory,
        app.process_manager.clone(),
        app.agent_titles.clone(),
    );

    let mut last_error = None;

    for (kind, ctx, tools, prompt) in [
        (
            AgentKind::Compose,
            compose_ctx,
            compose_tools,
            agent_compose::KMS_SYSTEM_PROMPT,
        ),
        (
            AgentKind::Knowledge,
            knowledge_ctx,
            knowledge_tools,
            agent_knowledge::KNOWLEDGE_RETRIEVAL_PROMPT,
        ),
        (
            AgentKind::Parallel,
            parallel_ctx,
            parallel_tools,
            agent_compose::PARALLEL_COMPOSE_PROMPT,
        ),
    ] {
        match agentik_core::Agent::builder()
            .with_model_pool(pool.clone())
            .with_context(ctx)
            .with_system_prompt_section(prompt)
            .with_tools(tools)
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
