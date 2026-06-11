//! Tool-name → user-facing-string rendering.
//!
//! This is the one place that hard-codes KMS tool names. It's a
//! deliberate trade-off: keeping the knowledge of "what `kms_view_local`
//! means to a human" next to the renderer is convenient, and the
//! renderer never has to fall back to a raw JSON dump. If a future
//! host has different tool names, the cleanest path is to either
//! (a) edit this file's `match` directly, or (b) introduce a
//! `ToolNameRenderer` trait and parameterize [`render_agent_panel`]
//! on it (see the B/C roadmap in `mod.rs`).

use serde_json::Value;

/// Produce a short, human-readable label for a tool call. Falls
/// back to "name key: value" or the raw name if the input is empty.
pub(crate) fn tool_user_facing_name(name: &str, input: &Value) -> String {
    let first_str = |k: &str| -> Option<String> {
        input
            .as_object()
            .and_then(|o| o.get(k))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };
    let first_id = |k: &str| -> Option<String> {
        first_str(k).map(|s| {
            if s.len() > 8 {
                format!("{}…", &s[..8])
            } else {
                s
            }
        })
    };
    let first_kv = || -> Option<String> {
        let obj = input.as_object()?;
        let (k, v) = obj.iter().next()?;
        Some(format!("{}: {}", k, format_value_short(v)))
    };

    match name {
        "kms_view_local" => first_str("path").map(|p| format!("View {}", p)),
        "kms_create_knowledge" => {
            first_str("title").map(|t| format!("Create knowledge \"{}\"", truncate_inline(&t, 30)))
        }
        "kms_update_knowledge" => {
            first_str("id").map(|id| format!("Update knowledge {}", first_id("id").unwrap_or(id)))
        }
        "kms_rename_knowledge" => {
            first_str("title").map(|t| format!("Rename to \"{}\"", truncate_inline(&t, 30)))
        }
        "kms_delete_knowledge" => first_str("id")
            .map(|_| format!("Delete knowledge {}", first_id("id").unwrap_or_default())),
        "kms_get_knowledge" => first_id("id").map(|id| format!("Get knowledge {}", id)),
        "kms_search_entity" => {
            first_str("query").map(|q| format!("Search '{}'", truncate_inline(&q, 30)))
        }
        "kms_search_subtree" => {
            first_str("query").map(|q| format!("Search subtree '{}'", truncate_inline(&q, 30)))
        }
        "kms_get_entity" => first_id("id").map(|id| format!("Get entity {}", id)),
        "kms_get_entity_knowledge" => {
            first_id("entity_id").map(|id| format!("Get entity knowledge {}", id))
        }
        "kms_list_entities" => first_str("entity_type").map(|t| format!("List {} entities", t)),
        "kms_create_entity" => {
            first_str("name").map(|n| format!("Create entity \"{}\"", truncate_inline(&n, 30)))
        }
        "kms_update_entity" => first_id("id").map(|id| format!("Update entity {}", id)),
        "kms_delete_entity" => first_id("id").map(|id| format!("Delete entity {}", id)),
        "kms_create_index" => {
            first_str("title").map(|t| format!("Create group \"{}\"", truncate_inline(&t, 30)))
        }
        "kms_move_index" => first_id("id").map(|id| format!("Move group {}", id)),
        "kms_delete_index" => first_id("id").map(|id| format!("Delete group {}", id)),
        "kms_navigate" => {
            first_str("target").map(|t| format!("Navigate to {}", truncate_inline(&t, 30)))
        }
        "kms_add_nomenclature" => {
            first_str("term").map(|t| format!("Nomenclature +\"{}\"", truncate_inline(&t, 30)))
        }
        "kms_update_nomenclature" => first_id("id").map(|id| format!("Nomenclature update {}", id)),
        "kms_delete_nomenclature" => first_id("id").map(|id| format!("Nomenclature delete {}", id)),
        "kms_link_orphans" => Some("Link orphans".to_string()),
        "kms_reorganize_children" => {
            first_id("parent_id").map(|id| format!("Reorganize children of {}", id))
        }
        "kms_merge_subtree" => {
            first_str("target").map(|t| format!("Merge subtree → {}", truncate_inline(&t, 30)))
        }
        "kms_parallel_dispatch" => first_str("staging_title")
            .map(|t| format!("Dispatch subtask \"{}\"", truncate_inline(&t, 30))),
        _ => first_kv().map(|kv| format!("{} {}", name, kv)),
    }
    .unwrap_or_else(|| name.to_string())
}

pub(crate) fn truncate_inline(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

pub(crate) fn format_value_short(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => truncate_inline(s, 30),
        Value::Array(arr) => format!("[{} items]", arr.len()),
        Value::Object(_) => "{…}".to_string(),
    }
}

pub(crate) fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}

pub(crate) fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let end = s
        .char_indices()
        .take_while(|(i, _)| *i < max)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(max);
    format!("{}…", &s[..end])
}
