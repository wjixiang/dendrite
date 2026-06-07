//! `ParallelComposeContext` — the orchestrator agent's context.
//!
//! Owns the full KMS write tool set plus the `kms_parallel_dispatch`
//! and `kms_merge_subtree` tools. Diagnostics ARE enabled so the
//! orchestrator can react to structural issues in the main tree
//! before deciding how to split a task.

use std::sync::Arc;

use agentik_core::context::{AgentContext, ContextDiagnostic, ContextSnapshot};
use agentik_sdk::model::model_pool::ModelPool;
use async_trait::async_trait;
use dendrite_tools::parallel_progress::ParallelProgressTx;
use dendrite_tools::ToolRegistration;

use crate::diagnostics::convert_diagnostics;
use crate::parallel_prompt::PARALLEL_COMPOSE_PROMPT;
use crate::subtree_context::SubTreeComposeContext;

pub struct ParallelComposeContext {
    kms: Arc<kms::KmsService>,
    pool: Arc<ModelPool>,
    progress_tx: ParallelProgressTx,
}

impl ParallelComposeContext {
    pub fn new(
        kms: Arc<kms::KmsService>,
        pool: Arc<ModelPool>,
        progress_tx: ParallelProgressTx,
    ) -> Self {
        Self {
            kms,
            pool,
            progress_tx,
        }
    }
}

#[async_trait]
impl AgentContext for ParallelComposeContext {
    async fn on_startup_location(&self) -> Result<Option<String>, String> {
        let location = self.kms.render_location().await?;
        Ok(Some(location))
    }

    async fn on_startup_diagnostics(&self) -> Result<Vec<ContextDiagnostic>, String> {
        let issues = self.kms.diagnose().await?;
        Ok(convert_diagnostics(issues))
    }

    async fn take_snapshot(&self) -> Result<ContextSnapshot, String> {
        Ok(ContextSnapshot::new(self.kms.get_pointer().await))
    }

    fn is_mutation_tool(&self, tool_name: &str) -> bool {
        // The parallel agent itself can mutate the main tree (e.g. to
        // pre-create target parents). Sub-agents dispatched through
        // `kms_parallel_dispatch` have their own contexts.
        tool_name.starts_with("kms_")
            && tool_name != "kms_parallel_dispatch"
            && tool_name != "kms_search_entity"
            && tool_name != "kms_navigate"
            && tool_name != "kms_get_entity_knowledge"
    }

    async fn on_mutation_diagnostics(&self) -> Result<Vec<ContextDiagnostic>, String> {
        let issues = self.kms.diagnose().await?;
        Ok(convert_diagnostics(issues))
    }

    async fn on_snapshot_change(
        &self,
        before: &ContextSnapshot,
        after: &ContextSnapshot,
    ) -> Result<Option<String>, String> {
        if before != after {
            let location = self.kms.render_location().await?;
            Ok(Some(location))
        } else {
            Ok(None)
        }
    }

    fn system_prompt_section(&self) -> String {
        PARALLEL_COMPOSE_PROMPT.to_string()
    }

    fn tool_registrations(&self) -> Vec<ToolRegistration> {
        let sub_factory: Arc<
            dyn Fn(Arc<kms::KmsService>) -> Arc<dyn agentik_core::context::AgentContext>
                + Send
                + Sync,
        > = Arc::new(|sub_svc: Arc<kms::KmsService>| {
            Arc::new(SubTreeComposeContext::new(sub_svc))
                as Arc<dyn agentik_core::context::AgentContext>
        });
        dendrite_tools::kms_tools::parallel_registrations(
            self.kms.clone(),
            self.pool.clone(),
            sub_factory,
            self.progress_tx.clone(),
        )
    }
}
