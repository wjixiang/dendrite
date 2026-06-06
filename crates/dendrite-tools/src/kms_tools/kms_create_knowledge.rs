use std::sync::Arc;

use serde_json::Value;
use types::tools::{ToolBuilder, ToolResult};

pub fn registration(svc: Arc<kms::KmsService>) -> tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_create_knowledge",
        "Create a knowledge entry about an entity or entities. Knowledge can be an 'aspect' (about one entity) or 'relation' (between multiple entities).",
    )
    .parameter("title", "string", "Title of the knowledge entry")
    .parameter("knowledge_type", "string", "'aspect' or 'relation'")
    .parameter("entities", "array", "Array of all entity names mentioned in the content (wrapping each in [[...]])")
    .parameter("content", "string", "The knowledge content/notes — use [[entity name]] to mark every entity mention")
    .required("title")
    .required("knowledge_type")
    .required("entities")
    .build();

    tools::ToolRegistration::new(
        definition,
        Box::new(tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let title = input["title"].as_str().ok_or("missing 'title'")?;
                validate_knowledge_title(title)?;
                let knowledge_type = match input["knowledge_type"].as_str() {
                    Some("relation") => kms::KnowledgeType::Relation,
                    _ => kms::KnowledgeType::Aspect,
                };
                let entity_refs: Vec<&str> = input["entities"]
                    .as_array()
                    .ok_or("missing 'entities'")?
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect();
                let content = input["content"].as_str().map(|s| s.to_string());

                let content = content.map(|c| flatten_nested_headings(&c));

                let knowledge = svc
                    .create_knowledge_by_ref(title, knowledge_type, entity_refs, content)
                    .await?;

                Ok(ToolResult::success_json(
                    "create_knowledge",
                    serde_json::json!({ "title": knowledge.title }),
                ))
            })
        })),
        vec![],
    )
}

/// Flatten nested markdown headings (##, ###, etc.) to bold-prefixed plain text
/// to prevent `internal_nested` diagnostic warnings.
/// e.g. "## 心脏结构" → "**心脏结构**"
fn flatten_nested_headings(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    for line in content.lines() {
        let trimmed = line.trim_start();
        let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
        if hash_count >= 2 {
            let text = trimmed[hash_count..].trim();
            result.push_str("**");
            result.push_str(text);
            result.push_str("**\n");
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    result
}

/// Vague title suffixes that indicate the aspect is too generic.
/// Must be kept in sync with `kms::diagnostics::knowledge_rules::VAGUE_TITLE_KEYWORDS`.
const VAGUE_TITLE_SUFFIXES: &[&str] = &[
    "概述", "总结", "小结", "定义", "简介", "说明", "介绍", "基本概念", "疾病特征",
];

fn validate_knowledge_title(
    title: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let suffix = title.split(" · ").nth(1).unwrap_or(title);
    for &keyword in VAGUE_TITLE_SUFFIXES {
        if suffix.contains(keyword) {
            return Err(format!(
                "标题 \"{title}\" 的切面描述包含模糊词汇 \"{keyword}\"。\
                 切面描述必须是具体的方面（如 \"药物治疗\"、\"诊断标准\"、\"发病机制\"），\
                 不能使用泛化术语。请选择一个更精确的切面名称后重试。"
            )
            .into());
        }
    }
    Ok(())
}
