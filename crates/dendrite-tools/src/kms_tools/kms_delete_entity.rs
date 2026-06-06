use std::sync::Arc;

use serde_json::Value;
use agentik_types::tools::{ToolBuilder, ToolResult};
use uuid::Uuid;

pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_delete_entity",
        "Delete an entity and all its nomenclatures by UUID. Use kms_list_entities to find the ID of orphan or duplicate entities.",
    )
    .parameter("id", "string", "UUID of the entity to delete")
    .required("id")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let id_str = input["id"].as_str().ok_or("missing 'id'")?;
                let id = Uuid::parse_str(id_str).map_err(|_| "invalid 'id' UUID")?;
                svc.delete_entity(id).await?;
                Ok(ToolResult::success_json(
                    "delete_entity",
                    serde_json::json!({ "deleted": id_str }),
                ))
            })
        })),
        vec![],
    )
}
