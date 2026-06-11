use std::sync::Arc;

use agent_compose::{KmsContext, ParallelComposeContext};
use agent_knowledge::KnowledgeContext;
use agentik::types::AgentEvent;
use agentik::types::messages::ContentBlock;

use crate::chat::ChatMessage;
use crate::state::{AgentKind, App};

use super::paste;

/// Rebuild all three agents with the given model pool and service reference.
pub async fn rebuild_all_agents(
    app: &mut App,
    pool: &Arc<agentik::sdk::model::model_pool::ModelPool>,
) {
    let svc = app.svc.clone();
    let svc_arc = Arc::new(svc);
    let corpus_arc = app.corpus.clone();

    // Compose agent — stateless, initialized once with root local_view.
    // Keep the concrete type so we can call `initialize()` before
    // erasing to the trait object.
    let compose_ctx_arc = Arc::new(KmsContext::new(svc_arc.clone(), corpus_arc.clone()));
    let _ = compose_ctx_arc.initialize().await;
    let compose_ctx: Arc<dyn agentik::core::context::AgentContext> = compose_ctx_arc;
    let compose_tools = dendrite_tools::registrations(
        svc_arc.clone(),
        corpus_arc.clone(),
        compose_ctx.clone(),
    );

    // Knowledge agent
    let knowledge_ctx_arc = Arc::new(KnowledgeContext::new(svc_arc.clone()));
    let _ = knowledge_ctx_arc.initialize().await;
    let knowledge_ctx: Arc<dyn agentik::core::context::AgentContext> = knowledge_ctx_arc;
    let knowledge_tools =
        dendrite_tools::readonly_registrations(svc_arc.clone(), corpus_arc.clone());

    // Parallel agent — stateless, initialized once with root local_view.
    let parallel_ctx_arc = Arc::new(ParallelComposeContext::new(svc_arc.clone()));
    let _ = parallel_ctx_arc.initialize().await;
    let parallel_ctx: Arc<dyn agentik::core::context::AgentContext> = parallel_ctx_arc;

    let sub_factory: Arc<
        dyn Fn(
                Arc<kms::KmsService>,
                Arc<agentik::sdk::model::model_pool::ModelPool>,
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
        match agentik::core::Agent::builder()
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

    // 2. Build replacement list from paste entries and file entries.
    let mut replacements: Vec<(String, String)> = Vec::new();
    for entry in &app.agent_pastes {
        match &entry.content {
            Some(content) if paste::should_ingest_as_document(content) => {
                // Long content — keep for ingestion, placeholder will be
                // replaced with doc ref after ingest.
                replacements.push((entry.placeholder.clone(), content.clone()));
            }
            None => {
                // Already ingested — use placeholder as-is (it already
                // says [doc:uuid, uploaded]).
                replacements.push((entry.placeholder.clone(), entry.placeholder.clone()));
            }
            _ => {
                // Short paste (content = Some, but below threshold).
                replacements.push((entry.placeholder.clone(), entry.placeholder.clone()));
            }
        }
    }
    // File entries that are long enough to ingest.
    for (placeholder, content) in &file_entries {
        if paste::should_ingest_as_document(content) {
            replacements.push((placeholder.clone(), content.clone()));
        } else {
            replacements.push((placeholder.clone(), placeholder.clone()));
        }
    }

    // 3. Build the text for chat history — always use compact
    //    placeholders, never full text.
    let mut display_text = user_input.clone();
    for (placeholder, replacement) in &replacements {
        if display_text.contains(placeholder) {
            display_text = display_text.replacen(placeholder, replacement, 1);
        }
    }

    // 4. Build the text for the LLM — for long content that was
    //    ingested, we DON'T expand; instead we'll do the ingestion
    //    in a blocking step first, then replace in the model text.
    let mut model_text = expanded;
    let mut ingest_queue: Vec<String> = Vec::new();
    for (placeholder, content) in &replacements {
        if !model_text.contains(placeholder) {
            continue;
        }
        let is_long = paste::should_ingest_as_document(content);
        if is_long {
            // Remove the full content and replace with a temporary
            // marker. We'll do the actual DB ingest next, then
            // substitute the marker with a proper doc placeholder.
            let marker = format!("\x00INGEST:{placeholder}\x00");
            ingest_queue.push(content.clone());
            model_text = model_text.replacen(placeholder, &marker, 1);
        }
        // Short content stays in model_text as-is (it IS the
        // placeholder, which is compact).
    }

    // 5. Perform ingestion synchronously (it's fast: just DB writes).
    //    The agent doesn't start until we're done.
    let corpus = app.corpus.clone();
    let corpus_inner = corpus.clone();
    let ingest_items = ingest_queue.clone();
    tokio::task::block_in_place(|| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            for content in &ingest_items {
                let title = generate_paste_title(content);
                let _ = corpus_inner.ingest_document(&title, None, content).await;
            }
        });
    });

    // 6. Replace markers with proper doc placeholders in model_text.
    //    We ingest long content; if successful the KmsService has it.
    //    But the simpler approach: just put a lightweight note.
    //    The agent already has available_documents injected at init,
    //    so we just need a short pointer.
    for content in &ingest_queue {
        let marker = format!("\x00INGEST:");
        let end_marker = format!("\x00");
        // Find and replace markers. We stored the content, so we
        // can match on a substring.
        while let Some(pos) = model_text.find(&marker) {
            let rest = &model_text[pos + marker.len()..];
            if let Some(end) = rest.find(&end_marker) {
                let title = generate_paste_title(content);
                model_text = format!(
                    "{}[长文本已上传「{}」为文档，使用 corpus_search / corpus_get_window 按需读取]{}",
                    &model_text[..pos],
                    title,
                    &rest[end + end_marker.len()..],
                );
            } else {
                break;
            }
        }
    }

    // Also expand any remaining short paste placeholders (these are
    // already compact, just ensure they're in model_text).
    // (They were already there — no action needed.)

    let kind = app.agent_kind;
    app.agent_messages_mut().push(ChatMessage::User {
        text: display_text,
    });
    app.agent_messages_mut().push(ChatMessage::Divider);
    app.agent_running = true;
    app.agent_requesting = false;
    app.agent_streaming = false;
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
