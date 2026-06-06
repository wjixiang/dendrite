use std::sync::Arc;

use serde_json::Value;
use types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_update_knowledge",
        "Update a knowledge entry's content and/or entities. Does NOT change the title.",
    )
    .parameter("title_ref", "string", "Current title of the knowledge to update")
    .parameter("content", "string", "New content — use [[entity name]] to mark entity mentions")
    .parameter("entities", "array", "New array of all entity names mentioned in the content")
    .required("title_ref")
    .build();

    tools::ToolRegistration::new(
        definition,
        Box::new(tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let title_ref = input["title_ref"].as_str().ok_or("missing 'title_ref'")?;
                let content = input["content"].as_str();
                let entities: Option<Vec<&str>> = if input["entities"].is_array() {
                    Some(
                        input["entities"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter_map(|v| v.as_str())
                            .collect(),
                    )
                } else {
                    None
                };

                let knowledge = svc
                    .update_knowledge_by_ref(title_ref, content, entities)
                    .await?;

                Ok(ToolResult::success_json(
                    "update_knowledge",
                    serde_json::json!({ "title": knowledge.title }),
                ))
            })
        })),
        vec![],
    )
}
