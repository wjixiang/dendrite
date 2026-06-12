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
    .parameter("chunk_index", "number", "Centre chunk index (non-negative integer).")
    .required("chunk_index")
    .parameter("before", "number", "Number of chunks before the centre (default 1).")
    .parameter("after", "number", "Number of chunks after the centre (default 1).")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let doc_id_str = input["doc_id"].as_str().ok_or("missing 'doc_id'")?;
                let chunk_index = parse_usize(&input["chunk_index"], "chunk_index")
                    .ok_or_else(|| "missing or invalid 'chunk_index'".to_string())?;
                let before = parse_usize(&input["before"], "before").unwrap_or(1);
                let after = parse_usize(&input["after"], "after").unwrap_or(1);
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

/// Parse a numeric JSON value as `usize`. Accepts both integer literals
/// (`42`) and float literals that happen to be whole numbers (`42.0`).
/// Returns `None` for missing or non-numeric values (so callers can fall
/// back to a default) and `Some(Err)` is not used — explicit
/// out-of-range / non-integer numerics also resolve to `None` and let
/// the caller surface a generic error.
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
