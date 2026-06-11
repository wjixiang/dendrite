//! KMS + corpus tool implementations, one module per tool.
//!
//! Each submodule defines a single [`agentik_core::tools::ToolRegistration`]
//! via its `registration(svc)` (or `registration(svc, corpus)`) function.
//! This module aggregates them into the flat list consumed by the agent
//! runtime.

use std::sync::Arc;

use agentik_core::context::{AgentContext, ContextChanges};
use agentik_core::tools::{ToolFunction, ToolRegistration};
use agentik_sdk::model::model_pool::ModelPool;
use serde_json::Value;

/// Tools that start with the `kms_` prefix but are read-only.
///
/// These do not mutate persistent state, so they must not trigger
/// post-mutation context refresh.
const READONLY_KMS_TOOLS: &[&str] = &[
    "kms_search_entity",
    "kms_get_entity_knowledge",
];

/// Tools that start with the `corpus_` prefix but are read-only.
const READONLY_CORPUS_TOOLS: &[&str] = &[
    "corpus_list",
    "corpus_get_chunk",
    "corpus_get_window",
    "corpus_search",
    "corpus_get_metadata",
];

/// Returns `true` when a tool is a mutation tool (i.e. starts with
/// `kms_` or `corpus_` and is *not* in the corresponding readonly list).
pub fn is_mutation_tool(tool_name: &str) -> bool {
    if tool_name.starts_with("kms_") {
        return !READONLY_KMS_TOOLS.contains(&tool_name);
    }
    if tool_name.starts_with("corpus_") {
        return !READONLY_CORPUS_TOOLS.contains(&tool_name);
    }
    false
}

// ---------------------------------------------------------------------------
// Mutation-refresh wrapper
// ---------------------------------------------------------------------------

/// Wraps a [`ToolFunction`] so that after execution it triggers a context
/// refresh via [`AgentContext::write`].  Used to keep the agent's
/// location and diagnostics in sync after mutation tools modify the KMS.
struct MutationRefreshTool {
    inner: Box<dyn ToolFunction>,
    ctx: Arc<dyn AgentContext>,
}

#[async_trait::async_trait]
impl ToolFunction for MutationRefreshTool {
    async fn execute(
        &self,
        input: Value,
    ) -> Result<agentik_core::tools::ToolResult, Box<dyn std::error::Error + Send + Sync>> {
        let result = self.inner.execute(input).await?;
        // Trigger context refresh.  Fire-and-forget errors — a context
        // refresh failure must not break the tool execution result.
        let _ = self.ctx.write(ContextChanges::default()).await;
        Ok(result)
    }

    fn timeout_seconds(&self) -> u64 {
        self.inner.timeout_seconds()
    }

    fn definition(&self) -> agentik_sdk::types::Tool {
        self.inner.definition()
    }
}

/// Minimal no-op tool used as a temporary placeholder during
/// `std::mem::replace` when swapping out the inner implementation.
struct NoopTool;

#[async_trait::async_trait]
impl ToolFunction for NoopTool {
    async fn execute(
        &self,
        _input: Value,
    ) -> Result<agentik_core::tools::ToolResult, Box<dyn std::error::Error + Send + Sync>> {
        unreachable!("NoopTool should never be executed")
    }
}

/// Wrap mutation tools so they trigger a context refresh after execution.
fn wrap_mutation_tools(
    tools: Vec<ToolRegistration>,
    ctx: Arc<dyn AgentContext>,
) -> Vec<ToolRegistration> {
    tools
        .into_iter()
        .map(|mut reg| {
            if is_mutation_tool(&reg.definition.name) {
                let ctx = ctx.clone();
                let inner = std::mem::replace(&mut reg.implementation, Box::new(NoopTool));
                reg.implementation = Box::new(MutationRefreshTool { inner, ctx });
            }
            reg
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-module declarations
// ---------------------------------------------------------------------------

mod corpus_delete;
mod corpus_get_chunk;
mod corpus_get_metadata;
mod corpus_get_window;
mod corpus_ingest;
mod corpus_list;
mod corpus_search;
mod kms_add_nomenclature;
mod kms_create_entity;
mod kms_create_index;
mod kms_create_knowledge;
mod kms_delete_entity;
mod kms_delete_index;
mod kms_delete_knowledge;
mod kms_delete_nomenclature;
mod kms_get_entity;
mod kms_get_entity_knowledge;
mod kms_get_knowledge;
mod kms_link_orphans;
mod kms_list_entities;
mod kms_merge_subtree;
mod kms_move_index;
mod kms_parallel_dispatch;
mod kms_rename_knowledge;
mod kms_reorganize_children;
mod kms_search_entity;
mod kms_search_subtree;
mod kms_subtree_knowledge;
mod kms_update_entity;
mod kms_update_knowledge;
mod kms_update_nomenclature;
mod kms_view_local;

// ---------------------------------------------------------------------------
// Aggregate registration functions
// ---------------------------------------------------------------------------

/// Raw tool list (no mutation wrapping). Used internally and as the
/// basis for [`registrations`] and [`parallel_registrations`].
fn raw_registrations(
    svc: Arc<kms::KmsService>,
    corpus: Arc<corpus::CorpusService>,
) -> Vec<ToolRegistration> {
    let mut tools: Vec<ToolRegistration> = vec![
        kms_create_entity::registration(svc.clone()),
        kms_update_entity::registration(svc.clone()),
        kms_list_entities::registration(svc.clone()),
        kms_get_entity::registration(svc.clone()),
        kms_search_entity::registration(svc.clone()),
        kms_delete_entity::registration(svc.clone()),
        kms_add_nomenclature::registration(svc.clone()),
        kms_update_nomenclature::registration(svc.clone()),
        kms_delete_nomenclature::registration(svc.clone()),
        kms_get_entity_knowledge::registration(svc.clone()),
        kms_create_knowledge::registration(svc.clone(), corpus.clone()),
        kms_get_knowledge::registration(svc.clone()),
        kms_create_index::registration(svc.clone()),
        kms_reorganize_children::registration(svc.clone()),
        kms_move_index::registration(svc.clone()),
        kms_link_orphans::registration(svc.clone()),
        kms_update_knowledge::registration(svc.clone()),
        kms_rename_knowledge::registration(svc.clone()),
        kms_delete_knowledge::registration(svc.clone()),
        kms_delete_index::registration(svc.clone()),
        kms_merge_subtree::registration(svc.clone()),
    ];
    // Corpus tools.
    tools.push(corpus_list::registration(corpus.clone()));
    tools.push(corpus_get_chunk::registration(corpus.clone()));
    tools.push(corpus_get_window::registration(corpus.clone()));
    tools.push(corpus_search::registration(corpus.clone()));
    tools.push(corpus_get_metadata::registration(corpus.clone()));
    tools.push(corpus_ingest::registration(corpus.clone()));
    tools.push(corpus_delete::registration(corpus));
    tools
}

/// Full write tool set with mutation-refresh wrapping.
///
/// Every mutation tool will trigger a `ctx.write()` after execution so
/// the agent loop picks up updated location and diagnostics.
pub fn registrations(
    svc: Arc<kms::KmsService>,
    corpus: Arc<corpus::CorpusService>,
    ctx: Arc<dyn AgentContext>,
) -> Vec<ToolRegistration> {
    wrap_mutation_tools(raw_registrations(svc, corpus), ctx)
}

/// Read-only tool set — no wrapping needed.
pub fn readonly_registrations(
    svc: Arc<kms::KmsService>,
    corpus: Arc<corpus::CorpusService>,
) -> Vec<ToolRegistration> {
    let mut tools: Vec<ToolRegistration> = vec![
        kms_search_entity::registration(svc.clone()),
        kms_get_entity::registration(svc.clone()),
        kms_get_entity_knowledge::registration(svc.clone()),
        kms_get_knowledge::registration(svc.clone()),
        kms_view_local::registration(svc.clone()),
        kms_subtree_knowledge::registration(svc.clone()),
        kms_search_subtree::registration(svc),
    ];
    tools.push(corpus_list::registration(corpus.clone()));
    tools.push(corpus_get_chunk::registration(corpus.clone()));
    tools.push(corpus_get_window::registration(corpus.clone()));
    tools.push(corpus_search::registration(corpus.clone()));
    tools.push(corpus_get_metadata::registration(corpus));
    tools
}

/// Configuration bundle produced by the sub-agent factory for parallel dispatch.
pub struct SubAgentConfig {
    pub context: Arc<dyn AgentContext>,
    pub system_prompt: &'static str,
    /// Optional async initializer that the dispatch awaits before
    /// spawning the sub-agent. The factory uses this to seed the
    /// sub-agent's `local_view` (and any other one-shot setup) without
    /// blocking the dispatch's async runtime.
    pub init: Option<std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>>,
}

/// Build the tool set for the parallel compose agent.
///
/// Includes every regular KMS tool (with mutation wrapping) plus:
/// - `kms_parallel_dispatch` (the fan-out orchestrator)
/// - `kms_merge_subtree` (already included in raw_registrations)
///
/// `sub_context_factory` is invoked once per spawned sub-agent to
/// produce a [`SubAgentConfig`] containing the sub-agent's context and
/// system prompt.
pub fn parallel_registrations(
    svc: Arc<kms::KmsService>,
    corpus: Arc<corpus::CorpusService>,
    ctx: Arc<dyn AgentContext>,
    pool: Arc<ModelPool>,
    sub_context_factory: Arc<
        dyn Fn(Arc<kms::KmsService>, Arc<ModelPool>, String) -> SubAgentConfig + Send + Sync,
    >,
    process_manager: Arc<agentik_core::process::ProcessManager>,
    agent_titles: Arc<std::sync::RwLock<std::collections::HashMap<uuid::Uuid, String>>>,
) -> Vec<ToolRegistration> {
    let mut tools = raw_registrations(svc.clone(), corpus.clone());
    tools.push(kms_parallel_dispatch::registration(
        svc,
        corpus,
        pool,
        sub_context_factory,
        process_manager,
        agent_titles,
    ));
    wrap_mutation_tools(tools, ctx)
}
