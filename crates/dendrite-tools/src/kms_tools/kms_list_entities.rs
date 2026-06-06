use std::sync::Arc;

use serde_json::Value;
use agentik_types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_list_entities",
        "List entities, optionally filtered by condition. Used to find entities with empty definitions or no nomenclatures.",
    )
    .parameter("filter", "string", "Filter condition: 'empty_definition', 'no_nomenclature', or 'all' (default: 'all')")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let filter = match input["filter"].as_str() {
                    Some("empty_definition") => kms::EntityFilter::EmptyDefinition,
                    Some("no_nomenclature") => kms::EntityFilter::NoNomenclature,
                    _ => kms::EntityFilter::All,
                };

                let entities = svc.list_entities(filter).await?;

                let results: Vec<Value> = entities
                    .into_iter()
                    .map(|e| {
                        let names: Vec<Value> = e
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
                        serde_json::json!({
                            "id": e.id.to_string(),
                            "names": names,
                            "definition": e.definition,
                        })
                    })
                    .collect();

                Ok(ToolResult::success_json(
                    "list_entities",
                    serde_json::json!({
                        "count": results.len(),
                        "entities": results,
                    }),
                ))
            })
        })),
        vec![],
    )
}
