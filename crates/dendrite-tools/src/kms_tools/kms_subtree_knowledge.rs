use std::sync::Arc;

use serde_json::Value;
use agentik::types::tools::{ToolBuilder, ToolResult};

/// Lists every knowledge entry inside the subtree rooted at `path`.
/// Stateless alternative to `kms_navigate` + `kms_get_entity_knowledge`
/// for the case where the agent already knows which subtree to inspect.
///
/// Use this when `kms_view_local`'s `subtree.knowledge_titles` is
/// truncated (`truncated: true`) and the agent needs the full list.
pub fn registration(svc: Arc<kms::KmsService>) -> agentik::core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_subtree_knowledge",
        "Stateless: list every knowledge entry inside the subtree rooted at `path`. \
         Each entry includes the title, knowledge type, and primary entity name. \
         \n\n\
         Does NOT modify the global pointer. Path syntax matches `kms_view_local`.",
    )
    .parameter(
        "path",
        "string",
        "Path to the subtree root. Use '/' for the entire tree.",
    )
    .required("path")
    .build();

    agentik::core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik::core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let path = input["path"].as_str().ok_or("missing 'path'")?;
                let knowledge_list = svc.get_subtree_knowledge_by_path(path).await?;

                // Best-effort: attach the primary entity name to each
                // knowledge entry to help the agent decide relevance.
                let mut results: Vec<Value> = Vec::with_capacity(knowledge_list.len());
                for k in knowledge_list {
                    let primary_entity = k
                        .entities
                        .first()
                        .map(|eid| {
                            // We deliberately do not block on entity
                            // fetching: the title is the most
                            // discriminating field and the agent can
                            // call `kms_get_entity` for definitions.
                            format!("entity:{}", eid)
                        })
                        .unwrap_or_default();
                    results.push(serde_json::json!({
                        "title": k.title,
                        "knowledge_type": format!("{:?}", k.knowledge_type),
                        "primary_entity_hint": primary_entity,
                        "entity_count": k.entities.len(),
                    }));
                }

                Ok(ToolResult::success_json(
                    "subtree_knowledge",
                    serde_json::json!({
                        "path": path,
                        "count": results.len(),
                        "knowledges": results,
                    }),
                ))
            })
        })),
        vec![],
    )
}
