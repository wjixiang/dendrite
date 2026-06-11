use std::sync::Arc;

use serde_json::Value;
use agentik::types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik::core::tools::ToolRegistration {
    let definition = ToolBuilder::new("kms_get_entity", "Get an entity by its nomenclature name.")
        .parameter("name", "string", "Nomenclature full name of the entity")
        .required("name")
        .build();

    agentik::core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik::core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let name = input["name"].as_str().ok_or("missing 'name'")?;
                let entity_id = svc.resolve(name).await?;
                let entity = svc.get_entity(entity_id).await?;

                let names: Vec<Value> = entity
                    .name
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "lang": format!("{:?}", n.lang),
                            "full": n.full,
                            "abbr": n.abbr
                        })
                    })
                    .collect();

                Ok(ToolResult::success_json(
                    "get_entity",
                    serde_json::json!({
                        "names": names,
                        "definition": entity.definition
                    }),
                ))
            })
        })),
        vec![],
    )
}
