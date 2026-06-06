use std::sync::Arc;

use serde_json::Value;
use types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_navigate",
        "Navigate the index pointer. Supports single segment, relative paths with '..', and absolute paths starting with '/'.\nExamples:\n- '心力衰竭' — descend into a child node\n- '..' — go to parent\n- '../心力衰竭' — go to parent then descend into '心力衰竭'\n- '/循环系统疾病/心力衰竭' — absolute path from root",
    )
    .parameter("target", "string", "Navigation target: child title, '..', relative path like '../心力衰竭', or absolute path like '/循环系统疾病/心力衰竭'")
    .required("target")
    .build();

    tools::ToolRegistration::new(
        definition,
        Box::new(tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let target = input["target"].as_str().ok_or("missing 'target'")?;
                let location = svc.navigate(target).await?;
                Ok(ToolResult::success_json(
                    "navigate_index",
                    serde_json::json!({ "location": location }),
                ))
            })
        })),
        vec![],
    )
}
