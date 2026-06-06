use std::sync::Arc;

use serde_json::Value;
use agentik_types::tools::{ToolBuilder, ToolResult};

/// Searches the subtree rooted at `path` for knowledge entries whose
/// title contains `keyword` (case-insensitive substring match).
///
/// Stateless — does not move the global pointer. Use this in
/// preference to `kms_get_entity_knowledge` + manual `kms_navigate`
/// when the user is hunting for a specific topic.
pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_search_subtree",
        "Stateless: search the subtree rooted at `path` for knowledge entries whose \
         title contains `keyword` (case-insensitive substring match).\n\n\
         Returns at most 50 matching knowledge entries, ordered by title.",
    )
    .parameter(
        "path",
        "string",
        "Path to the subtree root. Use '/' for the entire tree.",
    )
    .parameter("keyword", "string", "Substring to search for in knowledge titles (case-insensitive).")
    .required("path")
    .required("keyword")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let path = input["path"].as_str().ok_or("missing 'path'")?;
                let keyword = input["keyword"].as_str().ok_or("missing 'keyword'")?;

                let node_id = svc
                    .get_local_view_by_path(path)
                    .await?
                    .node
                    .id;
                let matches = svc.search_knowledge_titles(node_id, keyword).await?;

                let results: Vec<Value> = matches
                    .into_iter()
                    .take(50)
                    .map(|k| {
                        serde_json::json!({
                            "title": k.title,
                            "knowledge_type": format!("{:?}", k.knowledge_type),
                            "entity_count": k.entities.len(),
                        })
                    })
                    .collect();

                Ok(ToolResult::success_json(
                    "search_subtree",
                    serde_json::json!({
                        "path": path,
                        "keyword": keyword,
                        "count": results.len(),
                        "matches": results,
                    }),
                ))
            })
        })),
        vec![],
    )
}
