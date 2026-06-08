//! `ParallelComposeContext` — the orchestrator agent's context.
//!
//! Stateless query model: `initialize()` injects a one-shot root
//! `local_view` and diagnostic snapshot. After each mutation tool
//! call, `write()` re-runs diagnostics and bumps the version so the
//! framework re-injects the updated snapshot into the agent's memory.
//! The orchestrator analyses the user's input and dispatches work via
//! `kms_parallel_dispatch`.

use std::collections::HashMap;
use std::sync::Arc;

use agentik_core::context::{AgentContext, ContextChanges, ContextSnapshot};
use async_trait::async_trait;
use serde_json::json;

use crate::context::render_diagnostics;

pub struct ParallelComposeContext {
    kms: Arc<kms::KmsService>,
    state: std::sync::RwLock<ContextSnapshot>,
}

impl ParallelComposeContext {
    pub fn new(kms: Arc<kms::KmsService>) -> Self {
        Self {
            kms,
            state: std::sync::RwLock::new(ContextSnapshot::default()),
        }
    }

    /// Inject a one-shot root `local_view` and diagnostic snapshot so
    /// the framework fires `inject_context_if_changed` once at startup
    /// (version 0 → 1).
    pub async fn initialize(&self) -> Result<(), String> {
        let view = self.kms.get_local_view_by_path("/").await?;
        let diags = self.kms.diagnose().await.unwrap_or_default();
        let mut data = HashMap::new();
        data.insert("local_view".into(), json!(render_local_view(&view)));
        data.insert("diagnostics".into(), json!(render_diagnostics(&diags)));
        let mut guard = self.state.write().unwrap();
        *guard = ContextSnapshot { data, version: 1 };
        Ok(())
    }
}

#[async_trait]
impl AgentContext for ParallelComposeContext {
    async fn read(&self) -> ContextSnapshot {
        self.state.read().unwrap().clone()
    }

    /// Re-run diagnostics and bump the snapshot version so the
    /// framework re-injects the updated diagnostic block.
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
                kms::TargetType::Knowledge => "knowledge",
                kms::TargetType::Group => "group",
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

    s
}
