use std::sync::Arc;

use serde_json::Value;
use agentik::types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik::core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_link_orphans",
        "Batch-link orphan knowledge entries under a parent index. Each knowledge title becomes a knowledge-type index child.",
    )
    .parameter("parent_ref", "string", "Title of the parent index node to link orphans under")
    .parameter("knowledge_titles", "array", "Array of orphan knowledge titles to link")
    .required("parent_ref")
    .required("knowledge_titles")
    .build();

    agentik::core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik::core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let parent_ref = input["parent_ref"].as_str().ok_or("missing 'parent_ref'")?;
                let knowledge_titles: Vec<&str> = input["knowledge_titles"]
                    .as_array()
                    .ok_or("missing 'knowledge_titles'")?
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect();

                if knowledge_titles.is_empty() {
                    return Err("knowledge_titles must not be empty".into());
                }

                let linked = svc.link_orphans(parent_ref, &knowledge_titles).await?;

                Ok(ToolResult::success_json(
                    "link_orphans",
                    serde_json::json!({
                        "linked": linked,
                        "count": linked.len(),
                    }),
                ))
            })
        })),
        vec![],
    )
}
