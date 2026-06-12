use std::sync::Arc;

use serde_json::Value;
use agentik_sdk::types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_delete_index",
        "Delete an EMPTY Group-type index node by its title. The call REFUSES \
         to run in three cases — each protects against silent data loss: \
         (1) the index has any children (move or delete them first via \
         `kms_move_children` / `kms_move_index` / `kms_delete_index` / \
         `kms_delete_knowledge`); \
         (2) the index is a knowledge mount (`target_type=knowledge`) — \
         use `kms_delete_knowledge` to remove the knowledge itself, or \
         `kms_detach_knowledge` to temporarily unmount it; \
         (3) the index is the root.",
    )
    .parameter("title", "string", "Title of the (empty, group-type) index to delete")
    .required("title")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let title = input["title"].as_str().ok_or("missing 'title'")?;

                svc.delete_index(title).await?;

                Ok(ToolResult::success_json(
                    "delete_index",
                    serde_json::json!({ "deleted": title }),
                ))
            })
        })),
        vec![],
    )
}
