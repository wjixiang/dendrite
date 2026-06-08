//! `ParallelComposeContext` — the orchestrator agent's context.
//!
//! A pure KMS-backed context (same read/write pattern as [`KmsContext`]).
//! The system prompt and tool set (including `kms_parallel_dispatch` and
//! `kms_merge_subtree`) are configured at the builder call site, not
//! inside the context.

use std::collections::HashMap;
use std::sync::Arc;

use agentik_core::context::{AgentContext, ContextChanges, ContextSnapshot};
use async_trait::async_trait;
use serde_json::json;

use crate::diagnostics::convert_diagnostics_to_json;

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

    /// Initialize context with current KMS state (location + diagnostics).
    /// Must be called before the agent starts.
    pub async fn initialize(&self) -> Result<(), String> {
        let location = self.kms.render_location().await?;
        let diagnostics = self.kms.diagnose().await?;
        let mut data = HashMap::new();
        data.insert("location".into(), json!(location));
        data.insert("diagnostics".into(), convert_diagnostics_to_json(diagnostics));
        let mut guard = self.state.write().unwrap();
        *guard = ContextSnapshot { data, version: 1 };
        Ok(())
    }
}

#[async_trait]
impl AgentContext for ParallelComposeContext {
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
