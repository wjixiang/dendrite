use std::sync::Arc;

use serde_json::Value;
use agentik::types::tools::{ToolBuilder, ToolResult};
use uuid::Uuid;

pub fn registration(svc: Arc<kms::KmsService>) -> agentik::core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_update_entity",
        "Update an entity's definition and/or nomenclatures. Use name_ref or id to locate the entity.",
    )
    .parameter("name_ref", "string", "Current nomenclature full name of the entity to update (use id if entity has no nomenclature)")
    .parameter("id", "string", "UUID of the entity to update (use when entity has no nomenclature)")
    .parameter("definition", "string", "New definition for the entity")
    .parameter("names", "array", "New nomenclature array: [{lang: 'ZH'|'EN', full: string, abbr?: string}]")
    .build();

    agentik::core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik::core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let name_ref = input["name_ref"].as_str();
                let id_str = input["id"].as_str();

                let entity = if let Some(id_str) = id_str {
                    let id = Uuid::parse_str(id_str).map_err(|_| "invalid 'id' UUID")?;
                    let definition = input["definition"].as_str();
                    let names = parse_names(&input["names"])?;
                    svc.update_entity_by_id(id, definition, names).await?
                } else {
                    let name_ref = name_ref.ok_or("missing 'name_ref' or 'id'")?;
                    let definition = input["definition"].as_str();
                    let names = parse_names(&input["names"])?;
                    svc.update_entity_by_ref(name_ref, definition, names).await?
                };

                let names_json: Vec<Value> = entity
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
                    "update_entity",
                    serde_json::json!({
                        "id": entity.id.to_string(),
                        "name": entity.name.first().map(|n| n.full.as_str()).unwrap_or(""),
                        "definition": entity.definition,
                        "names": names_json,
                    }),
                ))
            })
        })),
        vec![],
    )
}

fn parse_names(
    val: &Value,
) -> Result<Option<Vec<kms::Nomenclature>>, Box<dyn std::error::Error + Send + Sync>> {
    if !val.is_array() {
        return Ok(None);
    }
    let names_arr = val.as_array().unwrap();
    let mut nomenclatures = Vec::with_capacity(names_arr.len());
    for name_val in names_arr {
        let lang = name_val["lang"].as_str().unwrap_or("ZH");
        let full = name_val["full"].as_str().ok_or("missing 'full' in nomenclature")?;
        let abbr = name_val["abbr"].as_str().map(|s| s.to_string());
        nomenclatures.push(kms::Nomenclature {
            id: Uuid::new_v4(),
            lang: match lang {
                "EN" => kms::Language::EN,
                _ => kms::Language::ZH,
            },
            full: full.to_string(),
            abbr,
        });
    }
    Ok(Some(nomenclatures))
}
