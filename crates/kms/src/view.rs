//! Local view types for stateless knowledge-tree navigation.
//!
//! These types support a "local view" pattern for read-only agents, which
//! lets them obtain structured information about any tree node without
//! mutating the global pointer (in contrast to the stateful `navigate`
//! method, which is preserved for backwards compatibility).

use uuid::Uuid;

use crate::storage::types::Index;
use crate::storage::types::TargetType;

pub const SUBTREE_TITLES_LIMIT: usize = 30;

/// A compact projection of a single tree node for use in `LocalView` and
/// tool output. Avoids leaking the full `Index` (no `parent_id`,
/// `target` UUID) to the agent when it is not needed.
#[derive(Debug, Clone)]
pub struct IndexView {
    pub id: Uuid,
    pub title: String,
    pub target_type: TargetType,
    pub position: i64,
}

/// Aggregate statistics about a node's subtree. Knowledge titles are
/// truncated to `SUBTREE_TITLES_LIMIT` to keep the agent's context window
/// bounded. Use `KmsService::get_subtree_knowledge` to fetch the full list.
#[derive(Debug, Clone)]
pub struct SubtreeSummary {
    pub total_nodes: usize,
    pub knowledge_count: usize,
    pub group_count: usize,
    pub max_depth: usize,
    pub knowledge_titles: Vec<String>,
    pub truncated: bool,
}

/// A stateless snapshot of a tree node and its immediate context.
///
/// `LocalView` is the central data type of the local-view pattern. It
/// captures everything an agent typically needs to make a navigation
/// decision:
///   * the node itself (`node`)
///   * the path from the root to that node (`path`)
///   * the node's direct children (`children`)
///   * aggregate statistics about the node's subtree (`subtree_summary`)
///
/// `LocalView` is always produced by stateless methods on
/// [`crate::KmsService`]; constructing it does not mutate the global
/// pointer.
#[derive(Debug, Clone)]
pub struct LocalView {
    pub node: Index,
    pub path: Vec<Index>,
    pub children: Vec<IndexView>,
    pub sibling_count: usize,
    pub subtree_summary: SubtreeSummary,
}
