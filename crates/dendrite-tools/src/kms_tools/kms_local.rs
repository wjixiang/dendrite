use std::sync::Arc;

use serde_json::Value;
use agentik_sdk::types::tools::{ToolBuilder, ToolResult};

/// Stateless inspection tool. Returns a structured `LocalView` for any
/// node in the index tree without mutating the global pointer or
/// triggering `on_snapshot_change` injections.
///
/// Path syntax mirrors `kms_move_children` / `kms_move_index`:
///   - `/循环系统/心力衰竭` — absolute path from the root
///   - `..` — not allowed (no current-pointer context; use an
///     absolute path instead)
///   - single segment or `/`-separated segments — supported
pub fn registration(svc: Arc<kms::KmsService>) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_local",
        "Stateless: fetch a structured local view of any node in the index tree. \
         Returns the node's metadata, ancestor path, direct children, sibling count, \
         and subtree summary (node counts, max depth, up to 30 knowledge titles). \
         \n\n\
         This tool does NOT modify the global pointer and is safe to call repeatedly. \
         It is the primary structural inspection tool — use it whenever you need to see \
         the shape of a subtree (parent / children / siblings / depth / counts).\n\n\
         Path syntax:\n\
         - '/心血管/心力衰竭' — absolute path from root\n\
         - '心力衰竭' or '心血管/心力衰竭' — resolved against the current pointer\n\
         - '..' is NOT supported (stateless — supply an absolute path)\n\n\
         If `path` is omitted, returns the local view of the root node.\n\n\
         If `subtree.knowledge_titles.truncated` is `true`, the subtree holds more than \
         30 knowledge entries; call `kms_subtree_knowledge` on the same path to get the \
         full list (it returns the flat knowledge list, while this tool only returns the \
         structural snapshot).",
    )
    .parameter(
        "path",
        "string",
        "Optional. Absolute or relative path. Defaults to '/' (root).",
    )
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let path = input["path"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "/".to_string());

                let view = svc.get_local_view_by_path(&path).await?;

                let path_titles: Vec<String> = view
                    .path
                    .iter()
                    .map(|n| n.title.clone().unwrap_or_else(|| "(unnamed)".to_string()))
                    .collect();

                let children: Vec<Value> = view
                    .children
                    .iter()
                    .map(|c| {
                        let kind = match c.target_type {
                            kms::TargetType::Knowledge => "knowledge",
                            kms::TargetType::Group => "group",
                        };
                        serde_json::json!({
                            "id": c.id.to_string(),
                            "title": c.title,
                            "type": kind,
                            "position": c.position,
                        })
                    })
                    .collect();

                Ok(ToolResult::success_json(
                    "local",
                    serde_json::json!({
                        "path_resolved": path_titles,
                        "node": {
                            "id": view.node.id.to_string(),
                            "title": view.node.title,
                            "type": match view.node.target_type {
                                kms::TargetType::Knowledge => "knowledge",
                                kms::TargetType::Group => "group",
                            },
                        },
                        "sibling_count": view.sibling_count,
                        "children": children,
                        "subtree": {
                            "total_nodes": view.subtree_summary.total_nodes,
                            "knowledge_count": view.subtree_summary.knowledge_count,
                            "group_count": view.subtree_summary.group_count,
                            "max_depth": view.subtree_summary.max_depth,
                            "knowledge_titles": view.subtree_summary.knowledge_titles,
                            "truncated": view.subtree_summary.truncated,
                        }
                    }),
                ))
            })
        })),
        vec![],
    )
}
