//! KMS tool implementations, one module per tool.
//!
//! Each submodule defines a single [`agentik_core::tools::ToolRegistration`] via its
//! `registration(svc)` function. This module aggregates them into the
//! flat list consumed by the agent runtime.

use std::sync::Arc;

use agentik_core::tools::ToolRegistration;
use agentik_sdk::model::model_pool::ModelPool;

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
mod kms_navigate;
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

pub fn registrations(svc: Arc<kms::KmsService>) -> Vec<ToolRegistration> {
    vec![
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
        kms_create_knowledge::registration(svc.clone()),
        kms_get_knowledge::registration(svc.clone()),
        kms_create_index::registration(svc.clone()),
        kms_navigate::registration(svc.clone()),
        kms_reorganize_children::registration(svc.clone()),
        kms_move_index::registration(svc.clone()),
        kms_link_orphans::registration(svc.clone()),
        kms_update_knowledge::registration(svc.clone()),
        kms_rename_knowledge::registration(svc.clone()),
        kms_delete_knowledge::registration(svc.clone()),
        kms_delete_index::registration(svc.clone()),
        kms_merge_subtree::registration(svc),
    ]
}

pub fn readonly_registrations(svc: Arc<kms::KmsService>) -> Vec<ToolRegistration> {
    vec![
        kms_search_entity::registration(svc.clone()),
        kms_navigate::registration(svc.clone()),
        kms_get_entity::registration(svc.clone()),
        kms_get_entity_knowledge::registration(svc.clone()),
        kms_get_knowledge::registration(svc.clone()),
        // Stateless local-view tools (preferred for read-only agents).
        kms_view_local::registration(svc.clone()),
        kms_subtree_knowledge::registration(svc.clone()),
        kms_search_subtree::registration(svc),
    ]
}

/// Build the tool set for the parallel compose agent.
///
/// Includes every regular KMS tool plus:
/// - `kms_parallel_dispatch` (the fan-out orchestrator)
/// - `kms_merge_subtree` (for staged merges the agent wants to do by hand)
///
/// `sub_context_factory` is invoked once per spawned sub-agent to
/// construct its `AgentContext` from a dedicated `KmsService` whose
/// pointer is pinned to the sub-agent's staging area. This indirection
/// lets the caller (typically the `agent-compose` crate) decide
/// exactly what prompt/tools the sub-agents get.
///
/// `process_manager` is the TUI-owned singleton that manages sub-agent
/// lifecycles. `agent_titles` is a shared map that maps spawned agent
/// UUIDs to human-readable titles for the TUI's agent status panel.
pub fn parallel_registrations(
    svc: Arc<kms::KmsService>,
    pool: Arc<ModelPool>,
    sub_context_factory: Arc<
        dyn Fn(Arc<kms::KmsService>) -> Arc<dyn agentik_core::context::AgentContext> + Send + Sync,
    >,
    process_manager: Arc<agentik_core::process::ProcessManager>,
    agent_titles: Arc<std::sync::RwLock<std::collections::HashMap<uuid::Uuid, String>>>,
) -> Vec<ToolRegistration> {
    let mut tools = registrations(svc.clone());
    tools.push(kms_parallel_dispatch::registration(
        svc,
        pool,
        sub_context_factory,
        process_manager,
        agent_titles,
    ));
    tools
}

