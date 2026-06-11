use std::sync::Arc;

use serde_json::Value;
use agentik::types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik::core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_reorganize_children",
        "Move specified child indexes under a newly created group index. Used to restructure the tree by grouping related siblings.",
    )
    .parameter("new_group_title", "string", "Title for the new grouping index")
    .parameter("child_titles", "array", "Titles of child indexes to move under the new group")
    .required("new_group_title")
    .required("child_titles")
    .build();

    agentik::core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik::core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let new_group_title = input["new_group_title"]
                    .as_str()
                    .ok_or("missing 'new_group_title'")?;
                let child_titles: Vec<String> = input["child_titles"]
                    .as_array()
                    .ok_or("missing 'child_titles'")?
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();

                if child_titles.is_empty() {
                    return Err("child_titles must not be empty".into());
                }

                let location = svc
                    .reorganize_children(new_group_title, &child_titles)
                    .await?;

                Ok(ToolResult::success_json(
                    "reorganize_children",
                    serde_json::json!({ "location": location }),
                ))
            })
        })),
        vec![],
    )
}
