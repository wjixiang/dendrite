use uuid::Uuid;

use crate::storage::{
    error::StorageError,
    types::{Entity, Index, Knowledge},
};

pub trait EntityRepo {
    async fn create(&self, entity: &Entity) -> Result<Uuid, StorageError>;
    async fn get(&self, id: Uuid) -> Result<Entity, StorageError>;
    async fn search_by_name(&self, keyword: &str) -> Result<Vec<Entity>, StorageError>;
    async fn find_by_exact_name(&self, name: &str) -> Result<Option<Entity>, StorageError>;
    async fn update(&self, entity: &Entity) -> Result<(), StorageError>;
    async fn delete(&self, id: Uuid) -> Result<(), StorageError>;
}

pub trait KnowledgeRepo {
    async fn create(&self, knowledge: &Knowledge) -> Result<Uuid, StorageError>;
    async fn get(&self, id: Uuid) -> Result<Knowledge, StorageError>;
    // WARN: 此处在Knowledge数量很多时有OOM风险，需要在增量计算诊断系统实现后进行防护
    async fn list_all(&self) -> Result<Vec<Knowledge>, StorageError>;
    async fn find_by_title(&self, title: &str) -> Result<Option<Knowledge>, StorageError>;
    async fn update(&self, knowledge: &Knowledge) -> Result<(), StorageError>;
    async fn delete(&self, id: Uuid) -> Result<(), StorageError>;
}

pub trait IndexRepo {
    async fn create(&self, entry: &Index) -> Result<Uuid, StorageError>;
    async fn get(&self, id: Uuid) -> Result<Index, StorageError>;
    async fn find_by_title(&self, title: &str) -> Result<Option<Index>, StorageError>;
    async fn find_root(&self) -> Result<Index, StorageError>;
    async fn update(&self, entry: &Index) -> Result<(), StorageError>;
    async fn delete(&self, id: Uuid) -> Result<(), StorageError>;
    async fn children_of(&self, parent_id: Option<Uuid>) -> Result<Vec<Index>, StorageError>;
    async fn subtree_knowledge_ids(&self, index_id: Uuid) -> Result<Vec<Uuid>, StorageError>;
    async fn reparent(
        &self,
        id: Uuid,
        new_parent_id: Uuid,
        position: i64,
    ) -> Result<(), StorageError>;
    async fn reindex_positions(&self, parent_id: Option<Uuid>) -> Result<(), StorageError>;
    async fn orphan_knowledge_titles(&self) -> Result<Vec<String>, StorageError>;
    async fn find_by_target(&self, target_id: Uuid) -> Result<Vec<Index>, StorageError>;
    async fn downgrade_to_group(&self, id: Uuid) -> Result<(), StorageError>;
}
