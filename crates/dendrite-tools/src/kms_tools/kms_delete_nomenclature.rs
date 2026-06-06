use std::sync::Arc;

use serde_json::Value;
use types::tools::{ToolBuilder, ToolResult};
use uuid::Uuid;

pub fn registration(svc: Arc<kms::KmsService>) -> tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_delete_nomenclature",
        "Delete a nomenclature from an entity. The entity must retain at least one nomenclature.",
    )
    .parameter("entity_id", "string", "UUID of the entity")
    .parameter("nomenclature_id", "string", "UUID of the nomenclature to delete")
    .required("entity_id")
    .required("nomenclature_id")
    .build();

    tools::ToolRegistration::new(
        definition,
        Box::new(tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let entity_id = Uuid::parse_str(input["entity_id"].as_str().ok_or("missing 'entity_id'")?)
                    .map_err(|_| "invalid 'entity_id' UUID")?;
                let nom_id = Uuid::parse_str(
                    input["nomenclature_id"]
                        .as_str()
                        .ok_or("missing 'nomenclature_id'")?,
                )
                .map_err(|_| "invalid 'nomenclature_id' UUID")?;
                let entity = svc.delete_nomenclature(entity_id, nom_id).await?;
                let names_json: Vec<Value> = entity
                    .name
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "id": n.id.to_string(),
                            "lang": format!("{:?}", n.lang),
                            "full": n.full,
                            "abbr": n.abbr
                        })
                    })
                    .collect();
                Ok(ToolResult::success_json(
                    "delete_nomenclature",
                    serde_json::json!({
                        "entity_id": entity_id.to_string(),
                        "names": names_json
                    }),
                ))
            })
        })),
        vec![],
    )
}
