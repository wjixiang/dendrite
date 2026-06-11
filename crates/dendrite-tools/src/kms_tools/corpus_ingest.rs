use std::sync::Arc;

use serde_json::Value;
use agentik_sdk::types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<corpus::CorpusService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "corpus_ingest",
        "Explicitly ingest a long text document into the corpus. \
         The text will be split into ~2000-char chunks with 200-char overlap.\n\n\
         Returns the document metadata including chunk count.",
    )
    .parameter("title", "string", "Short human-readable title for the document.")
    .required("title")
    .parameter("content", "string", "Full text content to ingest.")
    .required("content")
    .parameter("source", "string", "Optional source description (e.g. file path, URL).")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let title = input["title"].as_str().ok_or("missing 'title'")?;
                let content = input["content"].as_str().ok_or("missing 'content'")?;
                let source = input["source"].as_str();

                let doc = svc
                    .ingest_document(title, source, content)
                    .await?;

                Ok(ToolResult::success_json(
                    "corpus_ingest",
                    serde_json::json!({
                        "doc_id": doc.id.to_string(),
                        "title": doc.title,
                        "chunks": doc.chunk_count,
                        "chars": doc.char_count,
                        "created_at": doc.created_at,
                    }),
                ))
            })
        })),
        vec![],
    )
}
