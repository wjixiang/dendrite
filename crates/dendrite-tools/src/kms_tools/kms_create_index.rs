use std::sync::Arc;

use serde_json::Value;
use agentik_types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_create_index",
        "Create an index entry under a parent index. Indexes organize entities and knowledge.",
    )
    .parameter("parent_ref", "string", "Title of parent index entry")
    .parameter("title", "string", "Title of this index entry")
    .parameter("target_ref", "string", "Name of knowledge to reference (optional)")
    .parameter("target_type", "string", "'knowledge' if linking to a knowledge entry (optional)")
    .required("parent_ref")
    .required("title")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let parent_ref = input["parent_ref"].as_str().ok_or("missing 'parent_ref'")?;
                let title = input["title"].as_str().ok_or("missing 'title'")?;
                let target_ref = input["target_ref"].as_str();
                let target_type = input["target_type"].as_str().map(|tt| match tt {
                    "knowledge" => kms::TargetType::Knowledge,
                    _ => kms::TargetType::Group,
                });

                svc.create_index_by_ref(parent_ref, Some(title.to_string()), target_ref, target_type)
                    .await?;

                Ok(ToolResult::success_json(
                    "create_index",
                    serde_json::json!({ "title": title }),
                ))
            })
        })),
        vec![],
    )
}
