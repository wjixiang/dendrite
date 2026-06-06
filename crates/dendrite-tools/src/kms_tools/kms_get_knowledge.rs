use std::sync::Arc;

use serde_json::Value;
use types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_get_knowledge",
        "Get the full content of a knowledge entry by its title.",
    )
    .parameter("title", "string", "Title of the knowledge entry to retrieve")
    .required("title")
    .build();

    tools::ToolRegistration::new(
        definition,
        Box::new(tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let title = input["title"].as_str().ok_or("missing 'title'")?;
                let id = svc.resolve_knowledge(title).await?;
                let knowledge = svc.get_knowledge(id).await?;

                let entity_names: Vec<String> = {
                    let svc = svc.clone();
                    futures::future::join_all(knowledge.entities.iter().map(|eid| {
                        let svc = svc.clone();
                        async move {
                            svc.get_entity(*eid)
                                .await
                                .ok()
                                .and_then(|e| e.name.first().map(|n| n.full.clone()))
                        }
                    }))
                    .await
                    .into_iter()
                    .filter_map(|n| n)
                    .collect::<Vec<_>>()
                };

                Ok(ToolResult::success_json(
                    "get_knowledge",
                    serde_json::json!({
                        "title": knowledge.title,
                        "knowledge_type": format!("{:?}", knowledge.knowledge_type),
                        "entities": entity_names,
                        "content": knowledge.content,
                    }),
                ))
            })
        })),
        vec![],
    )
}
