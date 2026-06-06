use std::sync::Arc;

use serde_json::Value;
use types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_get_entity_knowledge",
        "Get all knowledge entries that reference a given entity.",
    )
    .parameter("entity_name", "string", "Name of the entity to look up")
    .required("entity_name")
    .build();

    tools::ToolRegistration::new(
        definition,
        Box::new(tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let entity_name = input["entity_name"].as_str().ok_or("missing 'entity_name'")?;
                let entity_id = svc.resolve(entity_name).await?;
                let knowledge_list = svc.get_entity_referencing_knowledge(entity_id).await?;

                let results: Vec<Value> = knowledge_list
                    .into_iter()
                    .map(|k| {
                        serde_json::json!({
                            "title": k.title,
                            "knowledge_type": format!("{:?}", k.knowledge_type),
                            "content": k.content,
                        })
                    })
                    .collect();

                Ok(ToolResult::success_json(
                    "get_entity_knowledge",
                    serde_json::json!({
                        "entity": entity_name,
                        "count": results.len(),
                        "knowledges": results,
                    }),
                ))
            })
        })),
        vec![],
    )
}
