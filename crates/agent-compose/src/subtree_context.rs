//! `SubTreeComposeContext` — `AgentContext` for sub-agents that
//! build knowledge inside a dedicated staging sub-tree.
//!
//! Stateless query model: `initialize(path)` injects a one-shot
//! `local_view` of the staging subtree (or any other path) and
//! `write()` is a no-op. Sub-agents navigate by absolute path via
//! `kms_view_local`; they no longer rely on a pinned global pointer.

use std::collections::HashMap;
use std::sync::Arc;

use agentik_core::context::{AgentContext, ContextChanges, ContextSnapshot};
use async_trait::async_trait;
use serde_json::json;

pub struct SubTreeComposeContext {
    kms: Arc<kms::KmsService>,
    state: std::sync::RwLock<ContextSnapshot>,
}

impl SubTreeComposeContext {
    pub fn new(kms: Arc<kms::KmsService>) -> Self {
        Self {
            kms,
            state: std::sync::RwLock::new(ContextSnapshot::default()),
        }
    }

    /// Inject a one-shot `local_view` of the staging subtree at `path`.
    /// The caller (e.g. `kms_parallel_dispatch`) provides the absolute
    /// path of the staging Group it just created. After this returns,
    /// the framework fires `inject_context_if_changed` once
    /// (version 0 → 1) and the snapshot is never refreshed.
    pub async fn initialize(&self, path: &str) -> Result<(), String> {
        let view = self.kms.get_local_view_by_path(path).await?;
        let mut data = HashMap::new();
        data.insert("local_view".into(), json!(render_local_view(&view)));
        data.insert(
            "staging_path".into(),
            json!(path.to_string()),
        );
        let mut guard = self.state.write().unwrap();
        *guard = ContextSnapshot { data, version: 1 };
        Ok(())
    }
}

#[async_trait]
impl AgentContext for SubTreeComposeContext {
    fn read(&self) -> ContextSnapshot {
        self.state.read().unwrap().clone()
    }

    /// No-op: the snapshot is fixed at `initialize()` time. Sub-agents
    /// explore the staging area on demand via `kms_view_local(path)`
    /// using the absolute staging path.
    async fn write(&self, _changes: ContextChanges) -> Result<(), String> {
        Ok(())
    }
}

fn render_local_view(view: &kms::LocalView) -> String {
    use std::fmt::Write;
    let mut s = String::new();

    let _ = writeln!(s, "## Staging 子树局部视图（启动时一次性注入）");

    let path_titles: Vec<String> = view
        .path
        .iter()
        .map(|n| {
            let kind = match n.target_type {
                kms::TargetType::Knowledge => " [knowledge]",
                kms::TargetType::Group => "",
            };
            format!(
                "{}{}",
                n.title.clone().unwrap_or_else(|| "(unnamed)".to_string()),
                kind
            )
        })
        .collect();
    let _ = writeln!(s, "### 当前路径: {}", path_titles.join(" / "));

    if view.children.is_empty() {
        let _ = writeln!(s, "### 直接子节点: (empty)");
    } else {
        let _ = writeln!(s, "### 直接子节点 ({}):", view.children.len());
        for c in &view.children {
            let kind = match c.target_type {
                kms::TargetType::Knowledge => " [knowledge]",
                kms::TargetType::Group => "",
            };
            let _ = writeln!(s, "  - {}{}", c.title, kind);
        }
    }

    let _ = writeln!(s, "### 子树统计:");
    let _ = writeln!(s, "  - 总节点数: {}", view.subtree_summary.total_nodes);
    let _ = writeln!(
        s,
        "  - 知识条目: {} / 分组: {}",
        view.subtree_summary.knowledge_count, view.subtree_summary.group_count
    );
    let _ = writeln!(s, "  - 最大深度: {}", view.subtree_summary.max_depth);

    if !view.subtree_summary.knowledge_titles.is_empty() {
        let _ = writeln!(s, "### 知识条目预览 (最多 30 条):");
        for t in &view.subtree_summary.knowledge_titles {
            let _ = writeln!(s, "  - {}", t);
        }
        if view.subtree_summary.truncated {
            let _ = writeln!(
                s,
                "  …(列表已截断；用 kms_subtree_knowledge 取完整列表)"
            );
        }
    }

    s
}
