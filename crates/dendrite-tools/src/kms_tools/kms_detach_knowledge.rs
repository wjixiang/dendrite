use std::sync::Arc;

use serde_json::Value;
use agentik_sdk::types::tools::{ToolBuilder, ToolResult};

/// Temporarily unmount a Knowledge from the index tree.
///
/// This tool removes only the Index node that surfaces the Knowledge in
/// the tree. The Knowledge row itself stays in the `knowledges` table,
/// but no Index points at it anymore — it is now an **orphan**.
///
/// Prefer `kms_delete_knowledge` (deletes the knowledge entirely and
/// downgrades all its mounts) or `kms_move_index` (moves a mount in
/// one step) whenever those fit. Use this tool only when neither does.
pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_detach_knowledge",
        "Temporarily UNMOUNT a Knowledge from the index tree by deleting \
         ONLY its knowledge-typed Index node. The Knowledge row itself \
         is preserved in the database as an ORPHAN (no Index surfaces it). \
         \n\n\
         ⚠️ DANGER — orphan Knowledge entries are invisible in the tree \
         view and easy to lose track of. You MUST re-link the orphan \
         before ending your turn, via `kms_link_orphans` (or \
         `kms_create_index` with target_type=knowledge). If you only \
         want to move a knowledge mount to a different parent, prefer \
         `kms_move_index` — it does the unmount+remount atomically and \
         never produces an orphan. \n\n\
         REFUSES when: the title resolves to a Group (use \
         `kms_delete_index` instead), the title is not found, or the \
         resolved index has children. Refuses to detach the root index.",
    )
    .parameter(
        "title",
        "string",
        "Title of the knowledge-typed Index node to detach (the mount, not the knowledge row).",
    )
    .required("title")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let title = input["title"].as_str().ok_or("missing 'title'")?;

                let knowledge_id = svc.detach_knowledge_index(title).await?;

                Ok(ToolResult::success_json(
                    "detach_knowledge",
                    serde_json::json!({
                        "detached_title": title,
                        "orphan_knowledge_id": knowledge_id.to_string(),
                        "warning": "Knowledge is now ORPHAN. Re-link via \
                                    kms_link_orphans before ending your turn.",
                    }),
                ))
            })
        })),
        vec![],
    )
}
