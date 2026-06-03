use crate::storage::types::Knowledge;

use super::{CodeDescription, Diagnostic, Severity};

// ── Trait ────────────────────────────────────────────────────────

pub trait KnowledgeDiagnosticRule: Send + Sync {
    fn check(&self, knowledge: &Knowledge) -> Option<Diagnostic>;
    fn name(&self) -> &str;
}

// ── Rules ───────────────────────────────────────────────────────

pub struct OrphanKnowledge;

impl KnowledgeDiagnosticRule for OrphanKnowledge {
    fn check(&self, _knowledge: &Knowledge) -> Option<Diagnostic> {
        None
    }

    fn name(&self) -> &str {
        "knowledge.orphan"
    }
}

pub struct EmptyContent;

impl KnowledgeDiagnosticRule for EmptyContent {
    fn check(&self, knowledge: &Knowledge) -> Option<Diagnostic> {
        match &knowledge.content {
            Some(c) if c.is_empty() => Some(Diagnostic {
                code: self.name().to_string(),
                code_description: Some(CodeDescription {
                    href: "kms://diagnostics/knowledge/empty-content".to_string(),
                }),
                location: knowledge.title.clone(),
                severity: Severity::Hint,
                message: "知识条目内容为空".to_string(),
                suggested_actions: vec![
                    "使用 kms_update_knowledge 补充该知识条目的内容".to_string(),
                ],
            }),
            _ => None,
        }
    }

    fn name(&self) -> &str {
        "knowledge.empty_content"
    }
}

pub struct NoEntities;

impl KnowledgeDiagnosticRule for NoEntities {
    fn check(&self, knowledge: &Knowledge) -> Option<Diagnostic> {
        if knowledge.entities.is_empty() {
            Some(Diagnostic {
                code: self.name().to_string(),
                code_description: Some(CodeDescription {
                    href: "kms://diagnostics/knowledge/no-entities".to_string(),
                }),
                location: knowledge.title.clone(),
                severity: Severity::Warning,
                message: "知识条目没有关联任何实体".to_string(),
                suggested_actions: vec![
                    "该知识条目应关联至少一个实体，请在创建时指定 entities 字段".to_string(),
                ],
            })
        } else {
            None
        }
    }

    fn name(&self) -> &str {
        "knowledge.no_entities"
    }
}

pub struct TitleMissingEntityPrefix;

impl KnowledgeDiagnosticRule for TitleMissingEntityPrefix {
    fn check(&self, knowledge: &Knowledge) -> Option<Diagnostic> {
        if !knowledge.title.contains(" · ") {
            Some(Diagnostic {
                code: self.name().to_string(),
                code_description: Some(CodeDescription {
                    href: "kms://diagnostics/knowledge/title-missing-entity-prefix".to_string(),
                }),
                location: knowledge.title.clone(),
                severity: Severity::Warning,
                message: "知识标题未遵循 \"实体名 · 方面描述\" 命名规范".to_string(),
                suggested_actions: vec![
                    "知识标题应包含实体名前缀，格式如 \"急性心力衰竭 · 药物治疗\"".to_string(),
                ],
            })
        } else {
            None
        }
    }

    fn name(&self) -> &str {
        "knowledge.title_missing_entity_prefix"
    }
}
