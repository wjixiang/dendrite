use std::sync::Arc;

use serde_json::Value;
use agentik_sdk::types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<corpus::CorpusService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "corpus_get_window",
        "Return a window of chunks centred on a given chunk index. \
         Useful for reading context around a search hit.\n\n\
         Returns chunks [chunk_index - before, chunk_index + after], \
         automatically clamped to valid bounds.",
    )
    .parameter("doc_id", "string", "Document UUID.")
    .required("doc_id")
    .parameter("chunk_index", "integer", "Centre chunk index.")
    .required("chunk_index")
    .parameter("before", "integer", "Number of chunks before the centre (default 1).")
    .parameter("after", "integer", "Number of chunks after the centre (default 1).")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let doc_id_str = input["doc_id"].as_str().ok_or("missing 'doc_id'")?;
                let chunk_index = input["chunk_index"].as_u64().ok_or("missing 'chunk_index'")? as usize;
                let before = input["before"].as_u64().unwrap_or(1) as usize;
                let after = input["after"].as_u64().unwrap_or(1) as usize;
                let doc_id = uuid::Uuid::parse_str(doc_id_str).map_err(|e| e.to_string())?;

                let chunks = svc.get_document_chunk_window(doc_id, chunk_index, before, after).await?;

                // Render chunks with clear separators.
                let mut parts: Vec<String> = Vec::with_capacity(chunks.len());
                for c in &chunks {
                    parts.push(format!("=== chunk {} (chars {}–{}) ===\n{}", c.index, c.char_start, c.char_end, c.content));
                }

                Ok(ToolResult::success_json(
                    "corpus_get_window",
                    serde_json::json!({
                        "doc_id": doc_id_str,
                        "chunks": chunks.len(),
                        "content": parts.join("\n\n"),
                    }),
                ))
            })
        })),
        vec![],
    )
}
