use std::sync::Arc;

use serde_json::Value;
use agentik_types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_doc_search",
        "Search a document for a keyword. Returns the top matching chunks \
         ranked by occurrence count (descending). Each hit includes the \
         chunk index and a short snippet.\n\n\
         Use this to locate relevant sections before calling kms_doc_get_window.",
    )
    .parameter("doc_id", "string", "Document UUID to search within.")
    .required("doc_id")
    .parameter("keyword", "string", "Keyword to search for (case-insensitive).")
    .required("keyword")
    .parameter("top_k", "integer", "Maximum number of hits to return (default 10).")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let doc_id_str = input["doc_id"].as_str().ok_or("missing 'doc_id'")?;
                let keyword = input["keyword"].as_str().ok_or("missing 'keyword'")?;
                let top_k = input["top_k"].as_u64().unwrap_or(10) as usize;
                let doc_id = uuid::Uuid::parse_str(doc_id_str).map_err(|e| e.to_string())?;

                let hits = svc.search_document(doc_id, keyword, top_k).await?;

                let results: Vec<Value> = hits
                    .iter()
                    .map(|h| {
                        serde_json::json!({
                            "chunk_index": h.index,
                            "snippet": h.snippet,
                        })
                    })
                    .collect();

                Ok(ToolResult::success_json(
                    "doc_search",
                    serde_json::json!({
                        "doc_id": doc_id_str,
                        "keyword": keyword,
                        "hits": results,
                        "total": hits.len(),
                    }),
                ))
            })
        })),
        vec![],
    )
}
