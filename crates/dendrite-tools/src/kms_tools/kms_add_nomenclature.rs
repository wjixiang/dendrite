use std::sync::Arc;

use serde_json::Value;
use agentik_sdk::types::tools::{ToolBuilder, ToolResult};
use uuid::Uuid;

pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_add_nomenclature",
        "Add a new nomenclature (name variant) to an existing entity. Use this when an entity needs an additional name in another language, an abbreviation, or an alias.",
    )
    .parameter("entity_id", "string", "UUID of the entity")
    .parameter("lang", "string", "Language of the nomenclature: 'ZH' or 'EN'")
    .parameter("full", "string", "Full name")
    .parameter("abbr", "string", "Abbreviation (optional)")
    .required("entity_id")
    .required("lang")
    .required("full")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let id_str = input["entity_id"].as_str().ok_or("missing 'entity_id'")?;
                let id = Uuid::parse_str(id_str).map_err(|_| "invalid 'entity_id' UUID")?;
                let lang = input["lang"].as_str().ok_or("missing 'lang'")?;
                let full = input["full"].as_str().ok_or("missing 'full'")?;
                let abbr = input["abbr"].as_str().map(|s| s.to_string());
                let lang = match lang {
                    "EN" => kms::Language::EN,
                    _ => kms::Language::ZH,
                };
                let entity = svc
                    .add_nomenclature(id, lang, full.to_string(), abbr)
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
                    "add_nomenclature",
                    serde_json::json!({
                        "entity_id": id_str,
                        "names": names_json
                    }),
                ))
            })
        })),
        vec![],
    )
}
