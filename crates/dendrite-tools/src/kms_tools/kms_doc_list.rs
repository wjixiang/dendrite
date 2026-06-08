use std::sync::Arc;

use serde_json::Value;
use agentik_types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_doc_list",
        "List all documents in the document buffer. Returns metadata (id, title, \
         char_count, chunk_count, source, created_at) for each document — does NOT \
         return chunk contents.",
    )
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |_input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let docs = svc.list_documents().await?;
                let list: Vec<Value> = docs
                    .iter()
                    .map(|d| {
                        serde_json::json!({
                            "doc_id": d.id.to_string(),
                            "title": d.title,
                            "source": d.source,
                            "chars": d.char_count,
                            "chunks": d.chunk_count,
                            "created_at": d.created_at,
                        })
                    })
                    .collect();
                Ok(ToolResult::success_json("doc_list", serde_json::json!({ "documents": list })))
            })
        })),
        vec![],
    )
}
