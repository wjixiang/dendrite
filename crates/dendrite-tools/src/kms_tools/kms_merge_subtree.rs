use std::sync::Arc;

use serde_json::Value;
use agentik::types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik::core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_merge_subtree",
        "Merge a staging sub-tree into a target parent in the main tree. \
         All direct children of the staging node are moved (reparented) \
         under the target parent, then the now-empty staging node is deleted. \
         Use this to fold a parallel-build staging area back into the main tree.",
    )
    .parameter(
        "sub_root_title",
        "string",
        "Title of the staging sub-root node (must be a Group)",
    )
    .parameter(
        "target_parent_title",
        "string",
        "Title of the target parent in the main tree",
    )
    .required("sub_root_title")
    .required("target_parent_title")
    .build();

    agentik::core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik::core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let sub_root_title = input["sub_root_title"]
                    .as_str()
                    .ok_or("missing 'sub_root_title'")?;
                let target_parent_title = input["target_parent_title"]
                    .as_str()
                    .ok_or("missing 'target_parent_title'")?;

                let sub_root_id = svc
                    .resolve_index(sub_root_title)
                    .await
                    .map_err(|e| format!("sub_root: {e}"))?;
                let target_parent_id = svc
                    .resolve_index(target_parent_title)
                    .await
                    .map_err(|e| format!("target_parent: {e}"))?;

                let moved = svc
                    .merge_subtree(sub_root_id, target_parent_id)
                    .await?;

                Ok(ToolResult::success_json(
                    "merge_subtree",
                    serde_json::json!({
                        "sub_root": sub_root_title,
                        "target_parent": target_parent_title,
                        "moved_children": moved,
                    }),
                ))
            })
        })),
        vec![],
    )
}
