//! [`AgentContext`] implementation backed by a [`kms::KmsService`].
//!
//! Stateless query model: the context is initialised once with a
//! `local_view` of the index root and a diagnostic snapshot. After
//! each mutation tool call, `write()` re-runs diagnostics and bumps
//! the version so the framework re-injects the updated snapshot into
//! the agent's memory. Agents can also inspect the tree on demand via
//! [`kms::KmsService::get_local_view_by_path`] (exposed as the
//! `kms_local` tool). This mirrors
//! [`agentik_knowledge::KnowledgeContext`](../../agent-knowledge/src/context.rs).

use std::collections::HashMap;
use std::sync::Arc;

use agentik_core::context::{AgentContext, ContextChanges, ContextSnapshot};
use async_trait::async_trait;
use serde_json::json;

pub struct KmsContext {
    kms: Arc<kms::KmsService>,
    corpus: Arc<corpus::CorpusService>,
    state: std::sync::RwLock<ContextSnapshot>,
}

impl KmsContext {
    pub fn new(kms: Arc<kms::KmsService>, corpus: Arc<corpus::CorpusService>) -> Self {
        Self {
            kms,
            corpus,
            state: std::sync::RwLock::new(ContextSnapshot::default()),
        }
    }

    pub async fn from_path(db_path: &str) -> Result<Self, String> {
        // Build the corpus first via the factory so KMS can validate
        // source_document_id references against it. Both services
        // point at the same DB file; the corpus migration runs first
        // and the KMS migration second.
        let corpus = corpus::CorpusService::open(corpus::Backend::Sqlite {
            path: db_path.to_string(),
        })
        .await
        .map_err(|e| e.to_string())?;
        let svc = Arc::new(kms::KmsService::new(db_path, corpus.clone()).await?);
        Ok(Self::new(svc, corpus))
    }

    /// Inject a one-shot `local_view` of the index root, a list of
    /// available documents, and the current diagnostic snapshot.
    /// Must be called before the agent starts so the framework's
    /// `inject_context_if_changed` fires once at startup (version 0 → 1).
    pub async fn initialize(&self) -> Result<(), String> {
        let view = self.kms.get_local_view_by_path("/").await?;
        let docs = self.corpus.list_documents().await.unwrap_or_default();
        let diags = self.kms.diagnose().await.unwrap_or_default();
        let mut data = HashMap::new();
        data.insert("local_view".into(), json!(render_local_view(&view)));
        data.insert(
            "available_documents".into(),
            json!(render_document_index(&docs)),
        );
        data.insert("diagnostics".into(), json!(render_diagnostics(&diags)));
        let mut guard = self.state.write().unwrap();
        *guard = ContextSnapshot { data, version: 1 };
        Ok(())
    }
}

#[async_trait]
impl AgentContext for KmsContext {
    async fn read(&self) -> ContextSnapshot {
        self.state.read().unwrap().clone()
    }

    /// Re-run diagnostics and bump the snapshot version so the
    /// framework re-injects the updated diagnostic block into the
    /// agent's memory at the next loop boundary.
    async fn write(&self, _changes: ContextChanges) -> Result<(), String> {
        let diags = self.kms.diagnose().await.unwrap_or_default();
        let mut guard = self.state.write().unwrap();
        guard
            .data
            .insert("diagnostics".into(), json!(render_diagnostics(&diags)));
        guard.version += 1;
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
fn render_document_index(docs: &[corpus::Document]) -> String {
    use std::fmt::Write;
    let mut s = String::new();

    let total_chunks: usize = docs.iter().map(|d| d.chunk_count).sum();

    if docs.is_empty() {
        let _ = writeln!(s, "## 语料库");
        let _ = writeln!(
            s,
            "当前无已上传文档。用户粘贴长文本时系统将自动切块并存储。"
        );
        return s;
    }

    let _ = writeln!(
        s,
        "## 语料库（启动时一次性注入）\n当前已上传 {} 个文档（合计 {} 块）：",
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
            d.id, d.title, d.chunk_count, d.char_count, source,
        );
    }
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "**如何阅读**：从 chunk_index=0 开始逐一调用 `corpus_get_window(doc_id, chunk_index=N, before=0, after=0)` \
         顺序精读每个块，逐块提取知识。禁止用 `corpus_search` 跳块阅读。\
         知识创建时把 `source_document_id` + `source_chunk_idx` 一起传入以便追溯。"
    );

    s
}

/// Render a list of [`kms::Diagnostic`] items into a human-readable
/// text block suitable for LLM injection.
pub(crate) fn render_diagnostics(diags: &[kms::Diagnostic]) -> String {
    use std::fmt::Write;
    let mut s = String::new();

    if diags.is_empty() {
        let _ = writeln!(s, "## 结构检查（无问题）");
        return s;
    }

    let _ = writeln!(s, "## 结构检查（{} 条问题）", diags.len());
    for d in diags {
        let _ = writeln!(s, "- [{}] {} — {}", d.severity.label(), d.code, d.message);
        if !d.location.is_empty() {
            let _ = writeln!(s, "  位置: {}", d.location);
        }
        for action in &d.suggested_actions {
            let _ = writeln!(s, "  → {}", action);
        }
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn renderer_handles_root() {
        let db_path = format!(
            "file:test-{}-{}?mode=memory&cache=shared",
            std::process::id(),
            uuid::Uuid::new_v4()
        );
        let corpus = corpus::CorpusService::open(corpus::Backend::Sqlite {
            path: db_path.clone(),
        })
        .await
        .unwrap();
        let svc = kms::KmsService::new(&db_path, corpus.clone()).await;
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
