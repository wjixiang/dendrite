use std::sync::Arc;

use serde_json::Value;
use agentik_types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_move_index",
        "Move an index node (and its entire subtree) to a new parent. Use this to restructure the tree without creating duplicates.",
    )
    .parameter("index_title", "string", "Title of the index to move")
    .parameter("new_parent_title", "string", "Title of the new parent index to move under")
    .required("index_title")
    .required("new_parent_title")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let index_title = input["index_title"]
                    .as_str()
                    .ok_or("missing 'index_title'")?;
                let new_parent_title = input["new_parent_title"]
                    .as_str()
                    .ok_or("missing 'new_parent_title'")?;

                let result = svc.move_index(index_title, new_parent_title).await?;

                Ok(ToolResult::success("move_index", &result))
            })
        })),
        vec![],
    )
}
