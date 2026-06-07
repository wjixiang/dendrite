//! `SubTreeComposeContext` — `AgentContext` for sub-agents that
//! build knowledge inside a dedicated staging sub-tree.
//!
//! Behaves like [`KmsContext`](crate::KmsContext) (full diagnostic
//! feedback loop + location re-injection) but uses the
//! [`SUBTREE_COMPOSE_PROMPT`](crate::subtree_prompt::SUBTREE_COMPOSE_PROMPT)
//! that focuses the agent on its own staging area. The pointer has
//! already been pinned to the staging Group by
//! [`KmsService::with_pointer`](kms::KmsService::with_pointer) so any
//! `kms_navigate` calls stay within the subtree.

use std::sync::Arc;

use agentik_core::context::{AgentContext, ContextDiagnostic, ContextSnapshot};
use async_trait::async_trait;
use dendrite_tools::ToolRegistration;

use crate::diagnostics::convert_diagnostics;
use crate::subtree_prompt::SUBTREE_COMPOSE_PROMPT;
use crate::tools::is_mutation_tool;

pub struct SubTreeComposeContext {
    kms: Arc<kms::KmsService>,
}

impl SubTreeComposeContext {
    pub fn new(kms: Arc<kms::KmsService>) -> Self {
        Self { kms }
    }
}

#[async_trait]
impl AgentContext for SubTreeComposeContext {
    async fn on_startup_location(&self) -> Result<Option<String>, String> {
        let location = self.kms.render_location().await?;
        Ok(Some(location))
    }

    async fn on_startup_diagnostics(&self) -> Result<Vec<ContextDiagnostic>, String> {
        let issues = self.kms.diagnose().await?;
        Ok(convert_diagnostics(issues))
    }

    /// Real pointer snapshot. The agent runtime compares snapshots
    /// before/after each tool call and re-injects the current location
    /// when it has changed, so the sub-agent stays oriented inside its
    /// own subtree.
    async fn take_snapshot(&self) -> Result<ContextSnapshot, String> {
        Ok(ContextSnapshot::new(self.kms.get_pointer().await))
    }

    fn is_mutation_tool(&self, tool_name: &str) -> bool {
        is_mutation_tool(tool_name)
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
        SUBTREE_COMPOSE_PROMPT.to_string()
    }

    /// Full KMS write tool set. The pointer is pinned to the staging
    /// area, so the sub-agent can only navigate freely within its own
    /// subtree.
    fn tool_registrations(&self) -> Vec<ToolRegistration> {
        dendrite_tools::kms_tools::registrations(self.kms.clone())
    }
}
