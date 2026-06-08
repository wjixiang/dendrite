use std::sync::Arc;

use serde_json::Value;
use agentik_types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_doc_get_metadata",
        "Get metadata for a single document, including a preview of the \
         first chunk's first 200 characters.",
    )
    .parameter("doc_id", "string", "Document UUID.")
    .required("doc_id")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let doc_id_str = input["doc_id"].as_str().ok_or("missing 'doc_id'")?;
                let doc_id = uuid::Uuid::parse_str(doc_id_str).map_err(|e| e.to_string())?;

                let doc = svc.get_document(doc_id).await?;

                // Peek at chunk 0 for a preview.
                let preview = match svc.get_document_chunk(doc_id, 0).await {
                    Ok(chunk) => {
                        let chars: String = chunk.content.chars().take(200).collect();
                        if chunk.content.chars().count() > 200 {
                            format!("{chars}…")
                        } else {
                            chars
                        }
                    }
                    Err(_) => "(empty)".to_string(),
                };

                Ok(ToolResult::success_json(
                    "doc_get_metadata",
                    serde_json::json!({
                        "doc_id": doc.id.to_string(),
                        "title": doc.title,
                        "source": doc.source,
                        "chars": doc.char_count,
                        "chunks": doc.chunk_count,
                        "created_at": doc.created_at,
                        "preview": preview,
                    }),
                ))
            })
        })),
        vec![],
    )
}
