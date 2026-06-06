use std::sync::Arc;

use serde_json::Value;
use agentik_types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_delete_knowledge",
        "Delete a knowledge entry. Indexes referencing this knowledge are downgraded to empty Group nodes (may trigger empty_leaf diagnostics).",
    )
    .parameter("title", "string", "Title of the knowledge to delete")
    .required("title")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let title = input["title"].as_str().ok_or("missing 'title'")?;

                svc.delete_knowledge(title).await?;

                Ok(ToolResult::success_json(
                    "delete_knowledge",
                    serde_json::json!({ "deleted": title }),
                ))
            })
        })),
        vec![],
    )
}
