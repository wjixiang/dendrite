use std::collections::HashMap;

use crate::Storage;
use crate::diagnostics::knowledge_rules::{NestedKnowledge, VagueTitle};
use crate::storage::repo::{EntityRepo, IndexRepo, KnowledgeRepo};
use crate::storage::types::{Index, TargetType};
use uuid::Uuid;

use super::entity_rules::EntityDiagnosticRule;
use super::index_rules::IndexDiagnosticRule;
use super::knowledge_rules::KnowledgeDiagnosticRule;
use super::{CodeDescription, Diagnostic, Severity};

// ── Location 文本渲染 ────────────────────────────────────────────

/// location 路径"最终指向"的种类，用于在 leaf 上追加文本标记，
/// 让 Agent 单从 location 字符串就能区分指向的是 index 节点还是 knowledge 节点。
#[derive(Clone, Copy)]
enum LocationLeafKind {
    /// 最终指向一个 index 节点（Group 分组或 Knowledge 链接都算 index 节点）。
    /// 叶子是 Group 时不加标记；叶子是 Knowledge-targeting index 时仍标注 [knowledge]，
    /// 因为此时叶子**承载**一条 knowledge，从 agent 视角与 Group 是不同语义。
    Index,
    /// 最终指向一条 knowledge 内容（诊断目标是 knowledge 而非 index）。
    Knowledge,
    /// 最终指向一个 entity。
    Entity,
}

fn leaf_kind_from_target(target_type: TargetType) -> LocationLeafKind {
    match target_type {
        TargetType::Group => LocationLeafKind::Index,
        TargetType::Knowledge => LocationLeafKind::Knowledge,
    }
}

/// 在 location 字符串的"叶子"（最后一个 ` > ` 分隔之后的片段）追加文本标记。
/// 这样 Agent 看到 `Root > 心血管疾病 > 冠心病 [knowledge]` 就能立刻知道
/// "冠心病" 实际是一条挂载的知识（target_type=knowledge），
/// 而 `Root > 心血管疾病 > 冠心病`（无标记）则是普通的 Group 索引节点。
fn format_location_with_leaf_marker(path: &str, leaf_kind: LocationLeafKind) -> String {
    let suffix = match leaf_kind {
        LocationLeafKind::Index => "",
        LocationLeafKind::Knowledge => " [knowledge]",
        LocationLeafKind::Entity => " [entity]",
    };
    if suffix.is_empty() {
        return path.to_string();
    }
    // 把标记追加到最后一个 ` > ` 之后；如果 path 中没有 ` > `（如 orphan 情况），
    // 就直接追加在末尾。
    match path.rfind(" > ") {
        Some(idx) => {
            let (prefix, last) = path.split_at(idx + " > ".len());
            format!("{}{}{}", prefix, last, suffix)
        }
        None => format!("{}{}", path, suffix),
    }
}

// ── Index ────────────────────────────────────────────────────────

async fn run_index_diagnostics(
    storage: &Storage,
) -> Result<(Vec<Diagnostic>, HashMap<Uuid, String>), String> {
    use super::index_rules::{
        DuplicateSiblingTitles, EmptyLeaf, ExcessiveChildren, InconsistentPrefixes,
    };

    let rules: Vec<Box<dyn IndexDiagnosticRule>> = vec![
        Box::new(EmptyLeaf),
        Box::new(ExcessiveChildren),
        Box::new(DuplicateSiblingTitles),
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
        // path_map 存原始路径（不带标记），供 knowledge 诊断构建 location 时复用，
        // 避免重复叠加 [knowledge] 标记。
        let raw_path = current_path.join(" > ");
        path_map.insert(node.id, raw_path.clone());
        // 诊断 location 追加 leaf 标记，区分 index 节点 vs knowledge 节点。
        let location = format_location_with_leaf_marker(
            &raw_path,
            leaf_kind_from_target(node.target_type),
        );

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
        let raw_location = knowledge_location
            .get(&k.id)
            .cloned()
            .unwrap_or_else(|| format!("(orphan) {}", k.title));
        let location = format_location_with_leaf_marker(&raw_location, LocationLeafKind::Knowledge);

        for rule in &rules {
            if let Some(mut d) = rule.check(&k) {
                d.location = location.clone();
                issues.push(d);
            }
        }

        if orphan_titles.contains(&k.title) {
            let orphan_raw = knowledge_location
                .get(&k.id)
                .cloned()
                .unwrap_or_else(|| format!("(orphan) {}", k.title));
            let orphan_location = format_location_with_leaf_marker(&orphan_raw, LocationLeafKind::Knowledge);
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
