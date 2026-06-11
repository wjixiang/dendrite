use std::sync::Arc;

use serde_json::Value;
use agentik::types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik::core::tools::ToolRegistration {
    let definition =
        ToolBuilder::new("kms_search_entity", "Search entities by nomenclature name (prefix match).")
            .parameter("keyword", "string", "Search keyword")
            .required("keyword")
            .build();

    agentik::core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik::core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let keyword = input["keyword"].as_str().ok_or("missing 'keyword'")?;
                let entities = svc.search_entity(keyword).await?;

                let results: Vec<Value> = entities
                    .into_iter()
                    .map(|e| {
                        serde_json::json!({
                            "name": e.name.first().map(|n| n.full.as_str()).unwrap_or(""),
                            "definition": e.definition
                        })
                    })
                    .collect();

                Ok(ToolResult::success_json(
                    "search_entity",
                    serde_json::Value::Array(results),
                ))
            })
        })),
        vec![],
    )
}
