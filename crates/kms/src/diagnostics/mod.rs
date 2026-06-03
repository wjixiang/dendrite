pub mod entity_rules;
pub mod index_rules;
pub mod knowledge_rules;

use std::collections::HashMap;

use crate::Storage;
use crate::storage::repo::{EntityRepo, IndexRepo, KnowledgeRepo};
use crate::storage::types::Index;
use uuid::Uuid;

// ── Types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Error => "ERROR",
            Severity::Warning => "WARN",
            Severity::Information => "INFO",
            Severity::Hint => "HINT",
        }
    }
}

pub struct CodeDescription {
    pub href: String,
}

/// 诊断信息数据结构体
pub struct Diagnostic {
    pub code: String,
    pub code_description: Option<CodeDescription>,
    pub location: String,
    pub severity: Severity,
    pub message: String,
    pub suggested_actions: Vec<String>,
}

// ── Runner ───────────────────────────────────────────────────────

async fn run_index_diagnostics(
    storage: &Storage,
) -> Result<(Vec<Diagnostic>, HashMap<Uuid, String>), String> {
    use index_rules::{EmptyLeaf, ExcessiveChildren, IndexDiagnosticRule};

    let rules: Vec<Box<dyn IndexDiagnosticRule>> = vec![
        Box::new(EmptyLeaf),
        Box::new(ExcessiveChildren),
    ];

    let mut issues = Vec::new();
    let mut path_map = HashMap::new();

    let root = storage.index.find_root().await.map_err(|e| e.to_string())?;
    let mut stack: Vec<(Index, usize, Vec<String>)> = vec![(root, 0, vec![])];

    while let Some((node, depth, path)) = stack.pop() {
        let title = node.title.as_deref().unwrap_or("(unnamed)");
        let mut current_path = path.clone();
        if depth > 0 {
            current_path.push(title.to_string());
        } else {
            current_path = vec!["Root".to_string()];
        }
        let location = current_path.join(" > ");
        path_map.insert(node.id, location.clone());

        let children = match storage.index.children_of(Some(node.id)).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        for rule in &rules {
            if let Some(d) = rule.check(&node, depth, &location, &children) {
                issues.push(d);
            }
        }

        for child in children.into_iter().rev() {
            stack.push((child, depth + 1, current_path.clone()));
        }
    }

    Ok((issues, path_map))
}

async fn run_knowledge_diagnostics(
    storage: &Storage,
    index_path_map: &HashMap<Uuid, String>,
) -> Result<Vec<Diagnostic>, String> {
    use knowledge_rules::{
        EmptyContent, NoEntities, OrphanKnowledge, TitleMissingEntityPrefix,
        KnowledgeDiagnosticRule,
    };

    let rules: Vec<Box<dyn KnowledgeDiagnosticRule>> = vec![
        Box::new(OrphanKnowledge),
        Box::new(EmptyContent),
        Box::new(NoEntities),
        Box::new(TitleMissingEntityPrefix),
    ];

    let mut issues: Vec<Diagnostic> = Vec::new();

    let orphan_titles = storage
        .index
        .orphan_knowledge_titles()
        .await
        .map_err(|e| e.to_string())?;

    // Build knowledge_id → index_path reverse mapping
    let linking_rows: Vec<(String, String)> = sqlx::query_as::<sqlx::Sqlite, (String, String)>(
        "SELECT id, target FROM indexes WHERE target_type = 'knowledge' AND target IS NOT NULL",
    )
    .fetch_all(storage.pool())
    .await
    .map_err(|e| e.to_string())?;

    let mut knowledge_location: HashMap<Uuid, String> = HashMap::new();
    for (index_id_str, target_str) in linking_rows {
        if let (Ok(index_id), Ok(knowledge_id)) =
            (Uuid::parse_str(&index_id_str), Uuid::parse_str(&target_str))
            && let Some(path) = index_path_map.get(&index_id)
        {
            knowledge_location.insert(knowledge_id, path.clone());
        }
    }

    let all_titles: Vec<String> = sqlx::query_as::<sqlx::Sqlite, (String,)>(
        "SELECT title FROM knowledges",
    )
    .fetch_all(storage.pool())
    .await
    .map_err(|e| e.to_string())?
    .into_iter()
    .map(|(t,)| t)
    .collect();

    for title in &all_titles {
        let knowledge = match storage.knowledge.find_by_title(title).await {
            Ok(Some(k)) => k,
            _ => continue,
        };

        let location = knowledge_location
            .get(&knowledge.id)
            .cloned()
            .unwrap_or_else(|| format!("(orphan) {}", knowledge.title));

        for rule in &rules {
            if let Some(mut d) = rule.check(&knowledge) {
                d.location = location.clone();
                issues.push(d);
            }
        }

        if orphan_titles.contains(title) {
            let orphan_location = knowledge_location
                .get(&knowledge.id)
                .cloned()
                .unwrap_or_else(|| format!("(orphan) {}", knowledge.title));
            issues.push(Diagnostic {
                code: "knowledge.orphan".to_string(),
                code_description: Some(CodeDescription {
                    href: "kms://diagnostics/knowledge/orphan".to_string(),
                }),
                location: orphan_location,
                severity: Severity::Warning,
                message: "有知识条目但没有被任何索引节点引用".to_string(),
                suggested_actions: vec![
                    "使用 kms_link_orphans 将该知识条目链接到适当的索引节点下".to_string(),
                ],
            });
        }
    }

    Ok(issues)
}

async fn run_entity_diagnostics(
    storage: &Storage,
) -> Result<Vec<Diagnostic>, String> {
    use entity_rules::{EmptyDefinition, MissingZhNomenclature, NoNomenclature, EntityDiagnosticRule};

    let rules: Vec<Box<dyn EntityDiagnosticRule>> = vec![
        Box::new(NoNomenclature),
        Box::new(EmptyDefinition),
        Box::new(MissingZhNomenclature),
    ];

    let mut issues: Vec<Diagnostic> = Vec::new();

    let rows = sqlx::query_as::<sqlx::Sqlite, (String,)>(
        "SELECT id FROM entities",
    )
    .fetch_all(storage.pool())
    .await
    .map_err(|e| e.to_string())?;

    for (id_str,) in rows {
        let id = Uuid::parse_str(&id_str).map_err(|e| e.to_string())?;
        let entity = match storage.entity.get(id).await {
            Ok(e) => e,
            Err(_) => continue,
        };

        for rule in &rules {
            if let Some(d) = rule.check(&entity) {
                issues.push(d);
            }
        }
    }

    Ok(issues)
}

pub async fn run_diagnostics(
    storage: &Storage,
) -> Result<Vec<Diagnostic>, String> {
    let mut all_issues = Vec::new();

    let (index_issues, index_path_map) = run_index_diagnostics(storage).await?;
    all_issues.extend(index_issues);

    let knowledge_issues = run_knowledge_diagnostics(storage, &index_path_map).await?;
    all_issues.extend(knowledge_issues);

    let entity_issues = run_entity_diagnostics(storage).await?;
    all_issues.extend(entity_issues);

    Ok(all_issues)
}
