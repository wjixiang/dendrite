use std::sync::Arc;

use serde_json::Value;
use types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_rename_knowledge",
        "Rename a knowledge entry. All indexes referencing this knowledge are updated to the new title.",
    )
    .parameter("current_title", "string", "Current title of the knowledge to rename")
    .parameter("new_title", "string", "New title for the knowledge entry")
    .required("current_title")
    .required("new_title")
    .build();

    tools::ToolRegistration::new(
        definition,
        Box::new(tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let current_title = input["current_title"]
                    .as_str()
                    .ok_or("missing 'current_title'")?;
                let new_title = input["new_title"].as_str().ok_or("missing 'new_title'")?;

                let knowledge = svc.rename_knowledge(current_title, new_title).await?;

                Ok(ToolResult::success_json(
                    "rename_knowledge",
                    serde_json::json!({ "old_title": current_title, "new_title": knowledge.title }),
                ))
            })
        })),
        vec![],
    )
}
