use std::sync::Arc;

use serde_json::Value;
use agentik_sdk::types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<corpus::CorpusService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "corpus_delete",
        "Delete a document and all its chunks. Knowledge entries that reference \
         this document will have their source_document_id set to NULL.",
    )
    .parameter("doc_id", "string", "Document UUID to delete.")
    .required("doc_id")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let doc_id_str = input["doc_id"].as_str().ok_or("missing 'doc_id'")?;
                let doc_id = uuid::Uuid::parse_str(doc_id_str).map_err(|e| e.to_string())?;

                svc.delete_document(doc_id).await?;

                Ok(ToolResult::success_json(
                    "corpus_delete",
                    serde_json::json!({
                        "doc_id": doc_id_str,
                        "deleted": true,
                    }),
                ))
            })
        })),
        vec![],
    )
}
