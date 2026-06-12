use std::sync::Arc;

use serde_json::Value;
use agentik_sdk::types::tools::{ToolBuilder, ToolResult};

/// Fetches multiple knowledge entries by title in a single call.
///
/// The LLM passes a list of titles; the service returns one result
/// per title in the **same order**. Missing or unresolvable titles
/// are reported as `status: "not_found"` (the batch as a whole does
/// not fail). Each result also resolves the linked entities' primary
/// names so the agent doesn't need a follow-up `kms_get_entity` per
/// title.
///
/// Use this to fan-in N related knowledges in one round trip rather
/// than making N separate `kms_get_knowledge` calls.
pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_get_knowledge_batch",
        "Stateless: fetch multiple knowledge entries by title in one call. \n\n\
         Returns one result per input title in the same order. \
         Missing or unresolvable titles are reported as `status: \"not_found\"` \
         (the batch as a whole succeeds). Each `ok` result includes the title, \
         knowledge type, resolved entity names, and full content. \n\n\
         Use this to fan-in N related knowledges in a single round trip rather \
         than making N separate `kms_get_knowledge` calls. Recommended when the \
         previous turn produced 2+ promising titles (e.g. from `kms_local` or \
         `kms_search_content`).",
    )
    .parameter(
        "titles",
        "array",
        "Array of knowledge titles to fetch (in the order you want them returned).",
    )
    .required("titles")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let titles = input["titles"]
                    .as_array()
                    .ok_or("missing 'titles' (must be an array)")?
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>();

                let results_raw = svc.get_knowledge_batch(titles).await?;

                // For each ok result, resolve entity names in parallel
                // so the agent doesn't have to follow up with N
                // `kms_get_entity` calls.
                let mut results: Vec<Value> = Vec::with_capacity(results_raw.len());
                for r in results_raw {
                    let entry = match (r.status, r.knowledge) {
                        (kms::BatchStatus::Ok, Some(kv)) => {
                            let entity_names: Vec<String> = {
                                futures::future::join_all(kv.entities.iter().map(|eid| {
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
                                .collect()
                            };
                            serde_json::json!({
                                "title": r.title,
                                "status": "ok",
                                "knowledge": {
                                    "title": kv.title,
                                    "knowledge_type": kv.knowledge_type,
                                    "entities": entity_names,
                                    "content": kv.content,
                                },
                            })
                        }
                        (kms::BatchStatus::Ok, None) => serde_json::json!({
                            "title": r.title,
                            "status": "not_found",
                        }),
                        (kms::BatchStatus::NotFound, _) => serde_json::json!({
                            "title": r.title,
                            "status": "not_found",
                        }),
                    };
                    results.push(entry);
                }

                Ok(ToolResult::success_json(
                    "get_knowledge_batch",
                    serde_json::json!({
                        "count": results.len(),
                        "results": results,
                    }),
                ))
            })
        })),
        vec![],
    )
}
