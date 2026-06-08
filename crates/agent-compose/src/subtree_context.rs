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

use agentik_core::context::{AgentContext, ContextChanges, ContextSnapshot};
use async_trait::async_trait;
use serde_json::json;

use crate::diagnostics::convert_diagnostics_to_json;

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
}

#[async_trait]
impl AgentContext for SubTreeComposeContext {
    fn read(&self) -> ContextSnapshot {
        self.state.read().unwrap().clone()
    }

    async fn write(&self, _changes: ContextChanges) -> Result<(), String> {
        let location = self.kms.render_location().await?;
        let diagnostics = self.kms.diagnose().await?;
        let mut guard = self.state.write().unwrap();
        guard.data.insert("location".into(), json!(location));
        guard.data.insert("diagnostics".into(), convert_diagnostics_to_json(diagnostics));
        guard.version += 1;
        Ok(())
    }
}
