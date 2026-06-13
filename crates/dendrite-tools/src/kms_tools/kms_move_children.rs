use std::sync::Arc;

use serde_json::Value;
use agentik_sdk::types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_move_children",
        "Move the named child indices from `source_path` into a group index \
         mounted under `remount_path`. \
         \n\n\
         ⚠️ ADDRESSING — `source_path` and `remount_path` are PATHS, NOT \
         titles. A bare title (e.g. `\"Python\"`) is silently treated as \
         a relative segment under whatever the current pointer happens to \
         be, so it often lands on the wrong node — or fails opaquely when \
         the same title exists under multiple parents. Always supply a \
         resolvable path: \
         \n  ✅ source_path=\"/编程语言/Python\"  (absolute, unambiguous) \
         \n  ✅ source_path=\"/\"                 (root) \
         \n  ✅ remount_path=\"/编程语言\"           (absolute) \
         \n  ❌ source_path=\"Python\"           (bare title — DO NOT USE) \
         \n  ❌ source_path=\"编程语言/Python\"     (missing leading `/`) \
         \nUse `kms_local` first if you are unsure of the absolute path. \
         \n\n\
         The two paths may be the same to regroup in place, or differ to \
         gather children from one subtree under a group in another \
         subtree. The destination group is **find-or-create**: if a \
         `Group`-typed child with the requested `new_group_title` already \
         exists under `remount_path` it is reused (the call is idempotent \
         — re-running the same regroup plan will append to the same \
         group), otherwise a fresh `Group`-typed index is created. A \
         non-Group (e.g. Knowledge-linker) child with the same title \
         causes the call to fail. \
         \n\n\
         `child_titles` IS title-based — each entry must match the title \
         of a direct child of `source_path`. Titles not found there cause \
         the call to fail. The split is deliberate: addressing the source/\
         destination subtrees needs full disambiguation (paths); naming \
         the leaves to pluck out is local enough to be unambiguous \
         (titles).",
    )
    .parameter(
        "source_path",
        "string",
        "ABSOLUTE PATH (starts with `/`) of the node whose direct children should be moved. \
         Use '/' for the root. Bare titles are NOT valid here — they resolve against \
         the implicit pointer and silently address the wrong node.",
    )
    .parameter(
        "remount_path",
        "string",
        "ABSOLUTE PATH (starts with `/`) of the node under which the destination group is mounted. \
         May be the same as source_path for an in-place regrouping. Bare titles are NOT valid here.",
    )
    .parameter(
        "new_group_title",
        "string",
        "Title of the destination group. If a `Group`-typed child with this title already exists under remount_path it is reused; otherwise a fresh group is created.",
    )
    .parameter(
        "child_titles",
        "array",
        "Titles (NOT paths) of child indexes to move under the destination group. \
         Each title must match a direct child of source_path.",
    )
    .required("source_path")
    .required("remount_path")
    .required("new_group_title")
    .required("child_titles")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let source_path = input["source_path"]
                    .as_str()
                    .ok_or("missing 'source_path'")?;
                let remount_path = input["remount_path"]
                    .as_str()
                    .ok_or("missing 'remount_path'")?;
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

                let result = svc
                    .move_children(source_path, remount_path, new_group_title, &child_titles)
                    .await?;

                Ok(ToolResult::success_json(
                    "move_children",
                    serde_json::json!({
                        "location": result.location,
                        "source_path": source_path,
                        "remount_path": remount_path,
                        "new_group_title": new_group_title,
                        "new_group_id": result.new_group_id,
                        "group_created": result.group_created,
                        "moved": child_titles,
                    }),
                ))
            })
        })),
        vec![],
    )
}
