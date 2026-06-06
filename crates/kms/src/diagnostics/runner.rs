use std::collections::HashMap;

use crate::Storage;
use crate::diagnostics::knowledge_rules::{NestedKnowledge, VagueTitle};
use crate::storage::repo::{EntityRepo, IndexRepo, KnowledgeRepo};
use crate::storage::types::Index;
use uuid::Uuid;

use super::entity_rules::EntityDiagnosticRule;
use super::index_rules::IndexDiagnosticRule;
use super::knowledge_rules::KnowledgeDiagnosticRule;
use super::{CodeDescription, Diagnostic, Severity};

// ── Index ────────────────────────────────────────────────────────

async fn run_index_diagnostics(
    storage: &Storage,
) -> Result<(Vec<Diagnostic>, HashMap<Uuid, String>), String> {
    use super::index_rules::{EmptyLeaf, ExcessiveChildren, InconsistentPrefixes};

    let rules: Vec<Box<dyn IndexDiagnosticRule>> = vec![
        Box::new(EmptyLeaf),
        Box::new(ExcessiveChildren),
        Box::new(InconsistentPrefixes),
    ];

    let mut issues = Vec::new();
    let mut path_map = HashMap::new();

    let root = storage.index.find_root().await.map_err(|e| e.to_string())?;
    let all_nodes = storage.index.list_all().await.map_err(|e| e.to_string())?;

    let mut children_map: HashMap<Option<Uuid>, Vec<Index>> = HashMap::new();
    for node in &all_nodes {
        children_map.entry(node.parent_id).or_default().push(node.clone());
    }

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

        let children = children_map.get(&Some(node.id)).cloned().unwrap_or_default();

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

// ── Knowledge ───────────────────────────────────────────────────

async fn run_knowledge_diagnostics(
    storage: &Storage,
    index_path_map: &HashMap<Uuid, String>,
) -> Result<Vec<Diagnostic>, String> {
    use super::knowledge_rules::{
        BoldAsHeading, EmptyContent, NoEntities, OrphanKnowledge, TitleMissingEntityPrefix,
    };

    let rules: Vec<Box<dyn KnowledgeDiagnosticRule>> = vec![
        Box::new(BoldAsHeading),
        Box::new(OrphanKnowledge),
        Box::new(EmptyContent),
        Box::new(NoEntities),
        Box::new(TitleMissingEntityPrefix),
        Box::new(NestedKnowledge),
        Box::new(VagueTitle),
    ];

    let mut issues: Vec<Diagnostic> = Vec::new();

    let orphan_titles = storage
        .index
        .orphan_knowledge_titles()
        .await
        .map_err(|e| e.to_string())?;

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

    let all_knowledges = storage
        .knowledge
        .list_all()
        .await
        .map_err(|e| e.to_string())?;

    for k in all_knowledges {
        let location = knowledge_location
            .get(&k.id)
            .cloned()
            .unwrap_or_else(|| format!("(orphan) {}", k.title));

        for rule in &rules {
            if let Some(mut d) = rule.check(&k) {
                d.location = location.clone();
                issues.push(d);
            }
        }

        if orphan_titles.contains(&k.title) {
            let orphan_location = knowledge_location
                .get(&k.id)
                .cloned()
                .unwrap_or_else(|| format!("(orphan) {}", k.title));
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

// ── Entity ─────────────────────────────────────────────────────

async fn run_entity_diagnostics(storage: &Storage) -> Result<Vec<Diagnostic>, String> {
    use super::entity_rules::{EmptyDefinition, MissingZhNomenclature, NoNomenclature};

    let rules: Vec<Box<dyn EntityDiagnosticRule>> = vec![
        Box::new(NoNomenclature),
        Box::new(EmptyDefinition),
        Box::new(MissingZhNomenclature),
    ];

    let mut issues: Vec<Diagnostic> = Vec::new();

    let all_entities = storage
        .entity
        .list_all()
        .await
        .map_err(|e| e.to_string())?;

    for entity in all_entities {
        for rule in &rules {
            if let Some(d) = rule.check(&entity) {
                issues.push(d);
            }
        }
    }

    Ok(issues)
}

// ── Entry ────────────────────────────────────────────────────────

pub async fn run_diagnostics(storage: &Storage) -> Result<Vec<Diagnostic>, String> {
    let mut all_issues = Vec::new();

    let (index_issues, index_path_map) = run_index_diagnostics(storage).await?;
    all_issues.extend(index_issues);

    let knowledge_issues = run_knowledge_diagnostics(storage, &index_path_map).await?;
    all_issues.extend(knowledge_issues);

    let entity_issues = run_entity_diagnostics(storage).await?;
    all_issues.extend(entity_issues);

    Ok(all_issues)
}
