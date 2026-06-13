use std::sync::Arc;

use serde_json::Value;
use agentik_sdk::types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_create_index",
        "Create an index entry under a parent index. Indexes organize entities and knowledge. \
         The new index's `title` must be unique among the parent's direct children — the call \
         fails with a duplicate-title error if a sibling already carries the same title (the \
         same title is allowed under a different parent). \
         \n\n\
         ⚠️ ADDRESSING — `parent_ref` accepts EITHER an absolute path \
         (recommended) OR a plain title: \
         \n  ✅ parent_ref=\"/\"                          (root, absolute path) \
         \n  ✅ parent_ref=\"/编程语言/Python\"            (absolute path, unambiguous) \
         \n  ✅ parent_ref=\"Python\"                     (plain title — resolved via title lookup) \
         \n  ❌ parent_ref=\"编程语言/Python\"              (missing leading `/`, ambiguous) \
         \nUse `kms_local` first if you are unsure of the absolute path.",
    )
    .parameter(
        "parent_ref",
        "string",
        "ABSOLUTE PATH (starts with `/`) of the parent index — use '/' for the root. \
         Plain titles also work but are ambiguous when the same title exists under multiple parents.",
    )
    .parameter("title", "string", "Title of this index entry")
    .parameter("target_ref", "string", "Name of knowledge to reference (optional)")
    .parameter("target_type", "string", "'knowledge' if linking to a knowledge entry (optional)")
    .required("parent_ref")
    .required("title")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let parent_ref = input["parent_ref"].as_str().ok_or("missing 'parent_ref'")?;
                let title = input["title"].as_str().ok_or("missing 'title'")?;
                let target_ref = input["target_ref"].as_str();
                let target_type = input["target_type"].as_str().map(|tt| match tt {
                    "knowledge" => kms::TargetType::Knowledge,
                    _ => kms::TargetType::Group,
                });

                svc.create_index_by_ref(parent_ref, Some(title.to_string()), target_ref, target_type)
                    .await?;

                Ok(ToolResult::success_json(
                    "create_index",
                    serde_json::json!({ "title": title }),
                ))
            })
        })),
        vec![],
    )
}
