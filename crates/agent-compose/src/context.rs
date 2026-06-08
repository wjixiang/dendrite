//! [`AgentContext`] implementation backed by a [`kms::KmsService`].
//!
//! Stateless query model: the context is initialised once with a
//! `local_view` of the index root, and `write()` is a no-op. There is
//! no per-tool-call re-injection of the global pointer, the rendered
//! "location" block, or the diagnostic snapshot — agents inspect the
//! tree on demand via [`kms::KmsService::get_local_view_by_path`]
//! (exposed as the `kms_view_local` tool). This mirrors
//! [`agentik_knowledge::KnowledgeContext`](../../agent-knowledge/src/context.rs).

use std::collections::HashMap;
use std::sync::Arc;

use agentik_core::context::{AgentContext, ContextChanges, ContextSnapshot};
use async_trait::async_trait;
use serde_json::json;

pub struct KmsContext {
    kms: Arc<kms::KmsService>,
    state: std::sync::RwLock<ContextSnapshot>,
}

impl KmsContext {
    pub fn new(kms: Arc<kms::KmsService>) -> Self {
        Self {
            kms,
            state: std::sync::RwLock::new(ContextSnapshot::default()),
        }
    }

    pub async fn from_path(db_path: &str) -> Result<Self, String> {
        let svc = kms::KmsService::new(db_path).await?;
        Ok(Self::new(Arc::new(svc)))
    }

    /// Inject a one-shot `local_view` of the index root plus a list
    /// of available documents. Must be called before the agent starts
    /// so the framework's `inject_context_if_changed` fires once at
    /// startup (version 0 → 1). Because `write()` is a no-op, the
    /// version never changes again and the snapshot is never
    /// re-injected.
    pub async fn initialize(&self) -> Result<(), String> {
        let view = self.kms.get_local_view_by_path("/").await?;
        let docs = self.kms.list_documents().await.unwrap_or_default();
        let mut data = HashMap::new();
        data.insert("local_view".into(), json!(render_local_view(&view)));
        data.insert(
            "available_documents".into(),
            json!(render_document_index(&docs)),
        );
        let mut guard = self.state.write().unwrap();
        *guard = ContextSnapshot { data, version: 1 };
        Ok(())
    }
}

#[async_trait]
impl AgentContext for KmsContext {
    fn read(&self) -> ContextSnapshot {
        self.state.read().unwrap().clone()
    }

    /// No-op: the snapshot is fixed at `initialize()` time. The compose
    /// agent inspects the tree on demand via `kms_view_local` rather
    /// than receiving a refreshed location/diagnostics block on every
    /// tool call.
    async fn write(&self, _changes: ContextChanges) -> Result<(), String> {
        Ok(())
    }
}

/// Render a [`kms::LocalView`] as a human-readable text block. Mirrors
/// the renderer in `agent-knowledge` so the two agents share the same
/// "local view" vocabulary.
fn render_local_view(view: &kms::LocalView) -> String {
    use std::fmt::Write;
    let mut s = String::new();

    let _ = writeln!(s, "## 索引树根节点局部视图（启动时一次性注入）");

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
                "  …(列表已截断；用 kms_subtree_knowledge('/') 获取完整列表)"
            );
        }
    }

    s
}

/// Render the list of available documents as a concise index block.
fn render_document_index(docs: &[kms::Document]) -> String {
    use std::fmt::Write;
    let mut s = String::new();

    let total_chunks: usize = docs.iter().map(|d| d.chunk_count).sum();

    if docs.is_empty() {
        let _ = writeln!(s, "## 文档缓冲层");
        let _ = writeln!(s, "当前无已上传文档。用户粘贴长文本时系统将自动切块并存储。");
        return s;
    }

    let _ = writeln!(
        s,
        "## 文档缓冲层（启动时一次性注入）\n当前已上传 {} 个文档（合计 {} 块）：",
        docs.len(),
        total_chunks
    );
    for d in docs {
        let source = d
            .source
            .as_deref()
            .map(|src| format!(", source=\"{src}\""))
            .unwrap_or_default();
        let _ = writeln!(
            s,
            "- [doc:{}, title=\"{}\", chunks={}, chars={}{}]",
            d.id,
            d.title,
            d.chunk_count,
            d.char_count,
            source,
        );
    }
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "**如何阅读**：用 `kms_doc_search(\"\", \"关键词\", top_k=5)` 找到相关块，\
         再用 `kms_doc_get_window(\"\", chunk_index=17, before=1, after=1)` 读取上下文。\
         知识创建时把 `source_document_id` + `source_chunk_idx` 一起传入以便追溯。"
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn renderer_handles_root() {
        let svc = kms::KmsService::new(":memory:").await;
        if let Ok(svc) = svc {
            let view = svc.get_local_view_by_path("/").await;
            if let Ok(view) = view {
                let s = render_local_view(&view);
                assert!(s.contains("根节点局部视图"));
                assert!(s.contains("总节点数"));
            }
        }
    }
}
