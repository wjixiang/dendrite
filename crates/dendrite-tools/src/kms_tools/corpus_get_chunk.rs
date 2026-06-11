use std::sync::Arc;

use serde_json::Value;
use agentik::types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<corpus::CorpusService>) -> agentik::core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "corpus_get_chunk",
        "Return the full text content of a single document chunk.",
    )
    .parameter("doc_id", "string", "Document UUID (from corpus_list).")
    .required("doc_id")
    .parameter("chunk_index", "integer", "Zero-based chunk index.")
    .required("chunk_index")
    .build();

    agentik::core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik::core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let doc_id_str = input["doc_id"].as_str().ok_or("missing 'doc_id'")?;
                let chunk_index = input["chunk_index"].as_u64().ok_or("missing 'chunk_index'")? as usize;
                let doc_id = uuid::Uuid::parse_str(doc_id_str).map_err(|e| e.to_string())?;

                let chunk = svc.get_document_chunk(doc_id, chunk_index).await?;
                Ok(ToolResult::success_json(
                    "corpus_get_chunk",
                    serde_json::json!({
                        "doc_id": doc_id_str,
                        "chunk_index": chunk.index,
                        "content": chunk.content,
                        "char_start": chunk.char_start,
                        "char_end": chunk.char_end,
                    }),
                ))
            })
        })),
        vec![],
    )
}
