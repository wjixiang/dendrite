use std::sync::Arc;

use agent_compose::{KmsContext, ParallelComposeContext};
use agent_knowledge::KnowledgeContext;
use agentik_sdk::types::AgentEvent;
use agentik_sdk::types::messages::ContentBlock;

use agent_panel_tui::ChatMessage;
use crate::state::{AgentKind, App};

/// Rebuild all three agents with the given model pool and service reference.
pub async fn rebuild_all_agents(
    app: &mut App,
    pool: &Arc<agentik_sdk::model::model_pool::ModelPool>,
) {
    let svc = app.svc.clone();
    let svc_arc = Arc::new(svc);
    let corpus_arc = app.corpus.clone();

    // Compose agent — stateless, initialized once with root local_view.
    // Keep the concrete type so we can call `initialize()` before
    // erasing to the trait object.
    let compose_ctx_arc = Arc::new(KmsContext::new(svc_arc.clone(), corpus_arc.clone()));
    let _ = compose_ctx_arc.initialize().await;
    let compose_ctx: Arc<dyn agentik_core::context::AgentContext> = compose_ctx_arc;
    let compose_tools = dendrite_tools::registrations(
        svc_arc.clone(),
        corpus_arc.clone(),
        compose_ctx.clone(),
    );

    // Knowledge agent
    let knowledge_ctx_arc = Arc::new(KnowledgeContext::new(svc_arc.clone()));
    let _ = knowledge_ctx_arc.initialize().await;
    let knowledge_ctx: Arc<dyn agentik_core::context::AgentContext> = knowledge_ctx_arc;
    let knowledge_tools =
        dendrite_tools::readonly_registrations(svc_arc.clone(), corpus_arc.clone());

    // Parallel agent — stateless, initialized once with root local_view.
    let parallel_ctx_arc = Arc::new(ParallelComposeContext::new(svc_arc.clone()));
    let _ = parallel_ctx_arc.initialize().await;
    let parallel_ctx: Arc<dyn agentik_core::context::AgentContext> = parallel_ctx_arc;

    let sub_factory: Arc<
        dyn Fn(
                Arc<kms::KmsService>,
                Arc<agentik_sdk::model::model_pool::ModelPool>,
                String,
            ) -> dendrite_tools::SubAgentConfig
            + Send
            + Sync,
    > = Arc::new(|sub_svc, _pool, staging_path| {
        let ctx = Arc::new(agent_compose::SubTreeComposeContext::new(sub_svc.clone()));
        // Seed the sub-agent's snapshot with a one-shot `local_view`
        // of the staging subtree. The dispatch awaits this future
        // before spawning the sub-agent.
        let ctx_for_init = ctx.clone();
        let path_for_init = staging_path.clone();
        let init: std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), String>> + Send>,
        > = Box::pin(async move { ctx_for_init.initialize(&path_for_init).await });
        dendrite_tools::SubAgentConfig {
            context: ctx,
            system_prompt: agent_compose::SUBTREE_COMPOSE_PROMPT,
            init: Some(init),
        }
    });
    let parallel_tools = dendrite_tools::parallel_registrations(
        svc_arc.clone(),
        corpus_arc.clone(),
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

pub fn spawn_agent_task(app: &mut App, user_input: String, pastes: Vec<agent_panel_tui::PasteEntry>) {
    if app.agent_running {
        return;
    }

    // 1. Expand @file references.
    let mut expanded = user_input.clone();
    let mut file_entries: Vec<(String, String)> = Vec::new();
    {
        let tokens: Vec<&str> = expanded.split_whitespace().collect();
        let mut rebuilt = String::new();
        for (i, token) in tokens.iter().enumerate() {
            if let Some(path) = token.strip_prefix('@') {
                if !path.is_empty() {
                    let resolved = resolve_file_path(path);
                    match std::fs::read_to_string(&resolved) {
                        Ok(content) => {
                            let placeholder = format!("[file:{i}]");
                            file_entries.push((placeholder.clone(), content));
                            if !rebuilt.is_empty() {
                                rebuilt.push(' ');
                            }
                            rebuilt.push_str(&placeholder);
                            continue;
                        }
                        Err(_) => {
                            // File not found — keep the token as-is.
                        }
                    }
                }
            }
            if !rebuilt.is_empty() {
                rebuilt.push(' ');
            }
            rebuilt.push_str(token);
        }
        expanded = rebuilt;
    }

    // 2. Use the paste entries captured by the Enter key handler
    //    before `take_input_text()` cleared them.
    //    Short pastes are inserted verbatim into the input display
    //    and have no entry here; long pastes have a
    //    `[Pasted ~N lines]` placeholder in `expanded` and the
    //    full content in this list.

    // 3. Build the ingest queue: every long paste, plus every
    //    long @file. The placeholder is what we'll search for
    //    in the model text; the content is what we upload.
    let mut ingest_queue: Vec<(String, String)> = Vec::new();
    for entry in &pastes {
        if agent_panel_tui::PASTE_SUMMARY_LINE_THRESHOLD
            <= entry.content.lines().count().max(1)
            || entry.content.chars().count() > agent_panel_tui::PASTE_SUMMARY_LEN_THRESHOLD
        {
            ingest_queue.push((entry.placeholder.clone(), entry.content.clone()));
        }
    }
    for (placeholder, content) in &file_entries {
        if agent_panel_tui::PASTE_SUMMARY_LINE_THRESHOLD
            <= content.lines().count().max(1)
            || content.chars().count() > agent_panel_tui::PASTE_SUMMARY_LEN_THRESHOLD
        {
            ingest_queue.push((placeholder.clone(), content.clone()));
        }
    }

    // 4. `display_text` is what the chat history shows: the
    //    input as the user composed it, with placeholders
    //    intact. Both long pastes and long @file references
    //    appear as their placeholder strings; the full content
    //    lives only in `ingest_queue` (and, for pastes, in the
    //    chat panel until `take_full_input_text` is called).
    let display_text = expanded.clone();

    // 5. Build the model text. Start from the same `expanded`
    //    string (which already has @file placeholders), then
    //    replace each ingest placeholder with a doc pointer.
    //    Short pastes are already verbatim in `expanded`, so
    //    no replacement is needed for them.
    let mut model_text = expanded;

    // 6. Perform ingestion asynchronously so the UI stays
    //    responsive.  The agent doesn't start until ingestion
    //    finishes.
    let corpus_inner = app.corpus.clone();
    let ingest_items = ingest_queue.clone();

    // 7. Replace each ingest placeholder with a doc pointer
    //    telling the agent how to retrieve the full content
    //    via corpus_search / corpus_get_window.
    for (placeholder, content) in &ingest_queue {
        if !model_text.contains(placeholder) {
            continue;
        }
        let title = generate_paste_title(content);
        let pointer = format!(
            "[长文本已上传「{}」为文档，使用 corpus_search / corpus_get_window 按需读取]",
            title,
        );
        model_text = model_text.replacen(placeholder, &pointer, 1);
    }

    let kind = app.agent_kind;
    app.agent_messages_mut().push(ChatMessage::User {
        text: display_text,
    });
    app.agent_messages_mut().push(ChatMessage::Divider);
    app.agent_running = true;
    app.agent_requesting = false;
    app.agent_streaming = false;
    app.chat_panel.enable_auto_scroll();

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    app.agent_event_rx = Some(rx);

    let agent_arc = match app.agents.get(&kind).cloned() {
        Some(arc) => arc,
        None => {
            let history = app.agent_messages_mut();
            history.pop(); // Divider
            history.pop(); // User
            app.agent_running = false;
            app.agent_requesting = false;
            app.agent_streaming = false;
            app.agent_event_rx = None;
            app.toast.error(format!(
                "{} agent is not available — check the model pool in Settings",
                kind.label()
            ));
            return;
        }
    };

    tokio::spawn(async move {
        // Ingest long pastes / @file content into the corpus
        // before starting the agent.
        for (_placeholder, content) in &ingest_items {
            let title = generate_paste_title(content);
            let _ = corpus_inner.ingest_document(&title, None, content).await;
        }

        let mut agent = agent_arc.lock().await;
        agent.event_tx = Some(tx.clone());

        if let Err(e) = agent.inject_message(vec![ContentBlock::Text { text: model_text }]) {
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

/// Generate a title for a pasted document based on the first few
/// non-blank characters of content.
fn generate_paste_title(content: &str) -> String {
    let preview: String = content
        .chars()
        .filter(|c| !c.is_whitespace())
        .take(30)
        .collect();
    let now = format_iso_now();
    if preview.len() >= 20 {
        format!("Pasted {now} — {preview}…")
    } else {
        format!("Pasted {now} — {preview}")
    }
}

/// ISO-8601 timestamp (same algorithm as kms::service::iso_now).
fn format_iso_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = d.as_secs();
    let mut days = (total_secs / 86400) as i64;
    let secs_of_day = (total_secs % 86400) as u32;
    let hh = secs_of_day / 3600;
    let mm = (secs_of_day % 3600) / 60;
    let ss = secs_of_day % 60;
    days += 719468;
    let era = if days >= 0 {
        days / 146097
    } else {
        (days - 146096) / 146097
    };
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 + doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let d = doy - (153 * (doe - (365 * yoe + yoe / 4 - yoe / 100)) + 2) / 5 + 1;
    let m = if d < 10 { d + 3 } else { d };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Resolve a user-supplied path (prefixed with `@`) to an absolute
/// file path.
fn resolve_file_path(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!(
                "{}/{}",
                home.to_string_lossy(),
                &path[2..]
            );
        }
    }
    path.to_string()
}
