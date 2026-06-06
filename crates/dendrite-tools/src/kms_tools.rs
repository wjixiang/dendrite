//! KMS tool implementations, one module per tool.
//!
//! Each submodule defines a single [`agentik_core::tools::ToolRegistration`] via its
//! `registration(svc)` function. This module aggregates them into the
//! flat list consumed by the agent runtime.

use std::sync::Arc;

use agentik_core::tools::ToolRegistration;

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
mod kms_move_index;
mod kms_navigate;
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
        kms_delete_index::registration(svc),
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
