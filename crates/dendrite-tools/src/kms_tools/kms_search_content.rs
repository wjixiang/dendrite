use std::sync::Arc;

use serde_json::Value;
use agentik_sdk::types::tools::{ToolBuilder, ToolResult};

/// Searches the subtree rooted at `path` for knowledge entries whose
/// CONTENT body contains `keyword` (case-insensitive substring).
///
/// Complement to `kms_search_subtree`, which only sees titles. This
/// tool is the right one when the user's question references a
/// concept that may appear inside the body (a drug name, a finding,
/// a guideline clause) but not in the title. Knowledge entries with
/// no content (`content = None`) are skipped.
///
/// Returns up to `top_k` (default 20) hits, ranked by match count
/// (descending). Each hit carries a short snippet of the first match.
pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_search_content",
        "Stateless: search the subtree rooted at `path` for knowledge entries whose \
         CONTENT body contains `keyword` (case-insensitive substring). \n\n\
         Returns up to `top_k` (default 20) hits ranked by match count (descending). \
         Each hit includes the title, a short snippet around the first match, and the \
         total occurrence count.\n\n\
         Knowledge entries with no content are skipped. This tool complements \
         `kms_search_subtree` (title-only) and is preferred when the user's \
         question references a concept that may live inside the body rather than \
         the title.\n\n\
         Use `kms_get_knowledge(title)` to fetch the full content for any \
         interesting hit.",
    )
    .parameter(
        "path",
        "string",
        "Path to the subtree root. Use '/' for the entire tree.",
    )
    .parameter("keyword", "string", "Substring to search for in knowledge content (case-insensitive).")
    .parameter("top_k", "number", "Max hits to return (default 20).")
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
                let top_k = input["top_k"]
                    .as_u64()
                    .or_else(|| input["top_k"].as_f64().map(|v| v as u64))
                    .unwrap_or(20) as usize;

                let node_id = svc
                    .get_local_view_by_path(path)
                    .await?
                    .node
                    .id;
                let hits = svc.search_knowledge_content(node_id, keyword, top_k).await?;

                let results: Vec<Value> = hits
                    .into_iter()
                    .map(|h| {
                        serde_json::json!({
                            "title": h.knowledge.title,
                            "knowledge_type": h.knowledge.knowledge_type,
                            "entity_count": h.knowledge.entities.len(),
                            "match_count": h.match_count,
                            "snippet": h.snippet,
                            "source_document_id": h.knowledge.source_document_id,
                            "source_chunk_idx": h.knowledge.source_chunk_idx,
                        })
                    })
                    .collect();

                Ok(ToolResult::success_json(
                    "search_content",
                    serde_json::json!({
                        "path": path,
                        "keyword": keyword,
                        "count": results.len(),
                        "hits": results,
                    }),
                ))
            })
        })),
        vec![],
    )
}
