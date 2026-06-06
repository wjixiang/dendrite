use std::sync::Arc;

use agentik_core::context::{AgentContext, ContextDiagnostic, ContextSnapshot};
use async_trait::async_trait;
use dendrite_tools::ToolRegistration;
use uuid::Uuid;

use crate::prompt::KNOWLEDGE_RETRIEVAL_PROMPT;

pub struct KnowledgeContext {
    kms: Arc<kms::KmsService>,
}

impl KnowledgeContext {
    pub fn new(kms: Arc<kms::KmsService>) -> Self {
        Self { kms }
    }

    pub async fn from_path(db_path: &str) -> Result<Self, String> {
        let svc = kms::KmsService::new(db_path).await?;
        Ok(Self::new(Arc::new(svc)))
    }
}

#[async_trait]
impl AgentContext for KnowledgeContext {
    /// Inject a **local view of the root node** at startup. This gives
    /// the agent a global overview (top-level children + subtree
    /// statistics + a 30-title preview) in a single round-trip, with no
    /// state mutation and no chance of conflicting with mutating
    /// agents that share the same `KmsService`.
    async fn on_startup_location(&self) -> Result<Option<String>, String> {
        let view = self.kms.get_local_view_by_path("/").await?;
        Ok(Some(render_local_view(&view)))
    }

    async fn on_startup_diagnostics(&self) -> Result<Vec<ContextDiagnostic>, String> {
        Ok(vec![])
    }

    /// **Stateless snapshot.** The retrieval agent is read-only and
    /// never wants pointer changes to be re-injected as fresh context.
    /// Returning a constant `nil`-UUID snapshot makes
    /// [`on_snapshot_change`](Self::on_snapshot_change) a permanent
    /// no-op.
    async fn take_snapshot(&self) -> Result<ContextSnapshot, String> {
        Ok(ContextSnapshot::new(Uuid::nil()))
    }

    fn is_mutation_tool(&self, _tool_name: &str) -> bool {
        false
    }

    async fn on_mutation_diagnostics(&self) -> Result<Vec<ContextDiagnostic>, String> {
        Ok(vec![])
    }

    /// No-op: snapshots are always equal (see [`take_snapshot`]).
    async fn on_snapshot_change(
        &self,
        _before: &ContextSnapshot,
        _after: &ContextSnapshot,
    ) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn system_prompt_section(&self) -> String {
        KNOWLEDGE_RETRIEVAL_PROMPT.to_string()
    }

    fn tool_registrations(&self) -> Vec<ToolRegistration> {
        dendrite_tools::kms_tools::readonly_registrations(self.kms.clone())
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
