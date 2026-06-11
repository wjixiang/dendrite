use std::collections::HashMap;
use std::sync::Arc;

use agentik::core::context::{AgentContext, ContextChanges, ContextSnapshot};
use async_trait::async_trait;
use serde_json::json;

pub struct KnowledgeContext {
    kms: Arc<kms::KmsService>,
    state: std::sync::RwLock<ContextSnapshot>,
}

impl KnowledgeContext {
    pub fn new(kms: Arc<kms::KmsService>) -> Self {
        Self {
            kms,
            state: std::sync::RwLock::new(ContextSnapshot::default()),
        }
    }

    pub async fn from_path(db_path: &str) -> Result<Self, String> {
        // Build corpus first via the factory so KMS can validate
        // source_document_id references against it.
        let corpus = corpus::CorpusService::open(corpus::Backend::Sqlite {
            path: db_path.to_string(),
        })
        .await
        .map_err(|e| e.to_string())?;
        let svc = kms::KmsService::new(db_path, corpus).await?;
        Ok(Self::new(Arc::new(svc)))
    }

    /// Initialize context with a root local view.
    /// Must be called before the agent starts so the local view is
    /// injected once at startup (version 0 → 1). Since `write()` is a
    /// no-op for this context, the version never changes again and
    /// the view is never re-injected.
    pub async fn initialize(&self) -> Result<(), String> {
        let view = self.kms.get_local_view_by_path("/").await?;
        let mut data = HashMap::new();
        data.insert("local_view".into(), json!(render_local_view(&view)));
        let mut guard = self.state.write().unwrap();
        *guard = ContextSnapshot { data, version: 1 };
        Ok(())
    }
}

#[async_trait]
impl AgentContext for KnowledgeContext {
    async fn read(&self) -> ContextSnapshot {
        self.state.read().unwrap().clone()
    }

    /// No-op: the knowledge agent is read-only and its context never
    /// changes. The version stays at whatever `initialize()` set (1).
    async fn write(&self, _changes: ContextChanges) -> Result<(), String> {
        Ok(())
    }
}

/// Render a [`kms::LocalView`] as a human-readable text block suitable
/// for injection into the LLM's context window.
fn render_local_view(view: &kms::LocalView) -> String {
    use std::fmt::Write;
    let mut s = String::new();

    let _ = writeln!(s, "## 根节点局部视图（启动时一次性注入）");

    // Ancestor path. For the root this is just "Root", but the helper
    // handles non-root views too.
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

    // Direct children.
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

    // Subtree summary.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn render_local_view_handles_root() {
        // Smoke test: build a service with no children and ensure the
        // renderer doesn't panic.
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
        let svc = kms::KmsService::new(&db_path, corpus).await;
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
