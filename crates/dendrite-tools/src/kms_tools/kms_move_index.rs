use std::sync::Arc;

use serde_json::Value;
use agentik_sdk::types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_move_index",
        "Move an index node (and its entire subtree) to a new parent. \
         \n\n\
         ⚠️ ADDRESSING — `index_path` and `new_parent_path` are PATHS, NOT \
         titles. A bare title (e.g. `\"心力衰竭\"`) is silently treated as \
         a relative segment under whatever the current pointer happens to \
         be, so it often lands on the wrong node when the same title \
         exists under multiple parents. Always supply an absolute path: \
         \n  ✅ index_path=\"/呼吸系统/哮喘\"        (absolute, unambiguous) \
         \n  ✅ new_parent_path=\"/儿科/常见疾病\"   (absolute) \
         \n  ❌ index_path=\"哮喘\"                  (bare title — DO NOT USE) \
         \n  ❌ new_parent_path=\"常见疾病\"          (bare title — DO NOT USE) \
         \nUse `kms_local` first if you are unsure of the absolute path. \
         \n\n\
         The root index cannot be moved, the new parent must not be a \
         descendant of the moved node, and a no-op move (target equals \
         current parent) is rejected.",
    )
    .parameter(
        "index_path",
        "string",
        "ABSOLUTE PATH (starts with `/`) of the index node to move (its full subtree follows). \
         Bare titles are NOT valid here — they resolve against the implicit pointer and silently \
         address the wrong node when titles repeat.",
    )
    .parameter(
        "new_parent_path",
        "string",
        "ABSOLUTE PATH (starts with `/`) of the new parent index. Must already exist. \
         Bare titles are NOT valid here.",
    )
    .required("index_path")
    .required("new_parent_path")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let index_path = input["index_path"]
                    .as_str()
                    .ok_or("missing 'index_path'")?;
                let new_parent_path = input["new_parent_path"]
                    .as_str()
                    .ok_or("missing 'new_parent_path'")?;

                let result = svc.move_index(index_path, new_parent_path).await?;

                Ok(ToolResult::success_json(
                    "move_index",
                    serde_json::json!({
                        "message": result,
                        "index_path": index_path,
                        "new_parent_path": new_parent_path,
                    }),
                ))
            })
        })),
        vec![],
    )
}
