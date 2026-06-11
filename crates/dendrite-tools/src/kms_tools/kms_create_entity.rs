use std::sync::Arc;

use serde_json::Value;
use agentik_sdk::types::tools::{ToolBuilder, ToolResult};
use uuid::Uuid;

pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_create_entity",
        "Create a new entity in the knowledge graph. Each (lang, full) combination must be unique — do NOT send duplicate names within the same call (e.g. two entries with the same lang and full). Duplicates will be silently removed.",
    )
    .parameter("names", "array", "Array of nomenclatures: [{lang: 'ZH'|'EN', full: string, abbr?: string}]. Each (lang, full) pair must be unique.")
    .parameter("definition", "string", "Brief definition of the entity")
    .required("names")
    .required("definition")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let definition = input["definition"].as_str().ok_or("missing 'definition'")?;
                if definition.is_empty() {
                    return Err("'definition' must not be empty".into());
                }
                let names_arr = input["names"].as_array().ok_or("missing 'names'")?;
                if names_arr.is_empty() {
                    return Err("'names' must not be empty".into());
                }

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

                let (entity, existed) = svc.create_entity(nomenclatures, definition).await?;

                Ok(ToolResult::success_json(
                    "create_entity",
                    serde_json::json!({
                        "name": entity.name.first().map(|n| n.full.as_str()).unwrap_or(""),
                        "definition": entity.definition,
                        "existed": existed
                    }),
                ))
            })
        })),
        vec![],
    )
}
