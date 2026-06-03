use uuid::Uuid;

use crate::language::Language;

#[derive(Debug)]
pub struct Entity {
    pub id: Uuid,
    pub name: Vec<Nomenclature>,
    pub definition: String,
}

#[derive(Debug)]
pub struct Nomenclature {
    pub id: Uuid,
    pub lang: Language,
    pub full: String,
    pub abbr: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KnowledgeType {
    Aspect,
    Relation,
}

impl KnowledgeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            KnowledgeType::Aspect => "aspect",
            KnowledgeType::Relation => "relation",
        }
    }

    pub fn convert_from_str(s: &str) -> Self {
        match s {
            "relation" => KnowledgeType::Relation,
            _ => KnowledgeType::Aspect,
        }
    }
}

/// 知识数据模型
#[derive(Debug)]
pub struct Knowledge {
    pub id: Uuid,

    /// UNIQUE 约束，不允许Knowledge的title存在重复，以保证Agent对title的一对一引用
    pub title: String,
    pub knowledge_type: KnowledgeType,
    pub entities: Vec<Uuid>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TargetType {
    Group,
    Knowledge,
}

#[derive(Debug, Clone)]
pub struct Index {
    pub id: Uuid,
    pub title: Option<String>,
    pub target: Option<Uuid>,
    pub target_type: TargetType,
    pub parent_id: Option<Uuid>,
    pub position: i64,
}
