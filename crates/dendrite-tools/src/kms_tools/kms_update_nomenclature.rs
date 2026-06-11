use std::sync::Arc;

use serde_json::Value;
use agentik::types::tools::{ToolBuilder, ToolResult};
use uuid::Uuid;

pub fn registration(svc: Arc<kms::KmsService>) -> agentik::core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_update_nomenclature",
        "Update an existing nomenclature's lang, full name, or abbreviation.",
    )
    .parameter("entity_id", "string", "UUID of the entity")
    .parameter("nomenclature_id", "string", "UUID of the nomenclature to update")
    .parameter("lang", "string", "New language: 'ZH' or 'EN'")
    .parameter("full", "string", "New full name")
    .parameter("abbr", "string", "New abbreviation (optional, pass empty string to clear)")
    .required("entity_id")
    .required("nomenclature_id")
    .required("lang")
    .required("full")
    .build();

    agentik::core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik::core::tools::SimpleTool::new(move |input: Value| {
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
                let lang = input["lang"].as_str().ok_or("missing 'lang'")?;
                let full = input["full"].as_str().ok_or("missing 'full'")?;
                let abbr = input["abbr"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                let lang = match lang {
                    "EN" => kms::Language::EN,
                    _ => kms::Language::ZH,
                };
                let entity = svc
                    .update_nomenclature(entity_id, nom_id, lang, full.to_string(), abbr)
                    .await?;
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
                    "update_nomenclature",
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
