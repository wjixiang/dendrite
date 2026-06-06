use std::sync::Arc;

use serde_json::Value;
use agentik_types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_delete_index",
        "Delete an index node by its title. Cannot delete the root index. Children of the deleted node are reparented to the deleted node's parent.",
    )
    .parameter("title", "string", "Title of the index to delete")
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
