use uuid::Uuid;

use crate::storage::{
    error::StorageError,
    types::{Entity, Index, Knowledge, Nomenclature},
};
use crate::DocumentChunk;

/// Raw ancestor path row, ordered from the requested node up to the root.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AncestorRow {
    pub id: String,
    pub title: Option<String>,
    pub target: Option<String>,
    pub target_type: Option<String>,
    pub parent_id: Option<String>,
    pub position: i64,
    pub depth: i64,
}

/// Lightweight child row for projection into `IndexView`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ChildRow {
    pub id: String,
    pub title: Option<String>,
    pub target_type: Option<String>,
    pub position: i64,
}

/// Aggregate statistics about a subtree.
#[derive(Debug, Clone, Default)]
pub struct SubtreeStatsRow {
    pub total_nodes: usize,
    pub knowledge_count: usize,
    pub group_count: usize,
    pub max_depth: usize,
    pub knowledge_titles: Vec<String>,
    pub truncated: bool,
}

pub trait EntityRepo {
    async fn create(&self, entity: &Entity) -> Result<Uuid, StorageError>;
    async fn get(&self, id: Uuid) -> Result<Entity, StorageError>;
    async fn list_all(&self) -> Result<Vec<Entity>, StorageError>;
    async fn search_by_name(&self, keyword: &str) -> Result<Vec<Entity>, StorageError>;
    async fn find_by_exact_name(&self, name: &str) -> Result<Option<Entity>, StorageError>;
    async fn update(&self, entity: &Entity) -> Result<(), StorageError>;
    async fn delete(&self, id: Uuid) -> Result<(), StorageError>;
    async fn add_nomenclature(&self, entity_id: Uuid, nomenclature: &Nomenclature) -> Result<(), StorageError>;
    async fn update_nomenclature(&self, entity_id: Uuid, nomenclature: &Nomenclature) -> Result<(), StorageError>;
    async fn delete_nomenclature(&self, nomenclature_id: Uuid) -> Result<(), StorageError>;
}

pub trait KnowledgeRepo {
    async fn create(&self, knowledge: &Knowledge) -> Result<Uuid, StorageError>;
    async fn get(&self, id: Uuid) -> Result<Knowledge, StorageError>;
    // WARN: 此处在Knowledge数量很多时有OOM风险，需要在增量计算诊断系统实现后进行防护
    async fn list_all(&self) -> Result<Vec<Knowledge>, StorageError>;
    async fn find_by_title(&self, title: &str) -> Result<Option<Knowledge>, StorageError>;
    async fn find_by_entity(&self, entity_id: Uuid) -> Result<Vec<Knowledge>, StorageError>;
    async fn update(&self, knowledge: &Knowledge) -> Result<(), StorageError>;
    async fn delete(&self, id: Uuid) -> Result<(), StorageError>;
}

pub trait IndexRepo {
    async fn create(&self, entry: &Index) -> Result<Uuid, StorageError>;
    async fn get(&self, id: Uuid) -> Result<Index, StorageError>;
    async fn list_all(&self) -> Result<Vec<Index>, StorageError>;
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

    // ---------- local-view (stateless) primitives ----------

    /// Return the ancestor path of `node_id`, ordered from `node_id`
    /// (depth 0) up to the root. Includes the node itself as the first
    /// row.
    async fn ancestor_path_rows(&self, node_id: Uuid) -> Result<Vec<AncestorRow>, StorageError>;

    /// Return the direct children of `node_id` as lightweight rows
    /// suitable for projection into `IndexView`. Ordered by `position`.
    async fn child_rows(&self, node_id: Uuid) -> Result<Vec<ChildRow>, StorageError>;

    /// Return aggregate statistics about the subtree rooted at `node_id`,
    /// including a truncated list of knowledge titles.
    async fn subtree_stats(
        &self,
        node_id: Uuid,
        title_limit: usize,
    ) -> Result<SubtreeStatsRow, StorageError>;

    /// Count the direct siblings of `node_id` (i.e. the number of
    /// children of its parent, including itself).
    async fn sibling_count(&self, node_id: Uuid) -> Result<usize, StorageError>;
}

/// Raw document row returned from the `documents` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DocumentRow {
    pub id: String,
    pub title: String,
    pub source: Option<String>,
    pub char_count: i64,
    pub chunk_count: i64,
    pub created_at: String,
}

/// Raw document chunk row returned from the `document_chunks` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DocumentChunkRow {
    pub document_id: String,
    pub chunk_index: i64,
    pub content: String,
    pub char_start: i64,
    pub char_end: i64,
}

pub trait DocumentRepo {
    async fn create_document(
        &self,
        id: Uuid,
        title: &str,
        source: Option<&str>,
        char_count: usize,
        chunk_count: usize,
        created_at: &str,
    ) -> Result<(), StorageError>;

    async fn create_chunk(&self, chunk: &DocumentChunk) -> Result<(), StorageError>;

    async fn list_documents(&self) -> Result<Vec<DocumentRow>, StorageError>;

    async fn get_document(&self, id: Uuid) -> Result<Option<DocumentRow>, StorageError>;

    async fn get_chunk(
        &self,
        doc_id: Uuid,
        chunk_index: usize,
    ) -> Result<Option<DocumentChunkRow>, StorageError>;

    async fn get_chunks_window(
        &self,
        doc_id: Uuid,
        start: usize,
        end: usize,
    ) -> Result<Vec<DocumentChunkRow>, StorageError>;

    async fn delete_document(&self, id: Uuid) -> Result<(), StorageError>;

    /// Search all chunks of a document for a keyword (case-insensitive
    /// substring). Returns `(chunk_index, snippet)` pairs sorted by
    /// the number of occurrences (descending).
    async fn search_keyword(
        &self,
        doc_id: Uuid,
        keyword: &str,
    ) -> Result<Vec<(usize, String)>, StorageError>;
}
