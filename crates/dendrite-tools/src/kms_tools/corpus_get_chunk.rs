use std::sync::Arc;

use serde_json::Value;
use agentik_sdk::types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<corpus::CorpusService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "corpus_get_chunk",
        "Return the full text content of a single document chunk.",
    )
    .parameter("doc_id", "string", "Document UUID (from corpus_list).")
    .required("doc_id")
    .parameter("chunk_index", "number", "Zero-based chunk index (non-negative integer).")
    .required("chunk_index")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let doc_id_str = input["doc_id"].as_str().ok_or("missing 'doc_id'")?;
                let chunk_index = parse_usize(&input["chunk_index"], "chunk_index")
                    .ok_or_else(|| "missing or invalid 'chunk_index'".to_string())?;
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

/// Parse a numeric JSON value as `usize`. Accepts both integer literals
/// (`42`) and float literals that happen to be whole numbers (`42.0`).
/// Returns `None` for missing or non-numeric values (so callers can
/// surface a generic error or fall back to a default).
fn parse_usize(value: &Value, field: &str) -> Option<usize> {
    let _ = field;
    if value.is_null() {
        return None;
    }
    if let Some(n) = value.as_u64() {
        return Some(n as usize);
    }
    if let Some(f) = value.as_f64() {
        if f.is_finite() && f >= 0.0 && f <= usize::MAX as f64 && f.fract() == 0.0 {
            return Some(f as usize);
        }
    }
    None
}
