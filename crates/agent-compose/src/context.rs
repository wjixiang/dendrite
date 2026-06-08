//! [`AgentContext`] implementation backed by a [`kms::KmsService`].

use std::collections::HashMap;
use std::sync::Arc;

use agentik_core::context::{AgentContext, ContextChanges, ContextSnapshot};
use async_trait::async_trait;
use serde_json::json;

use crate::diagnostics::convert_diagnostics_to_json;

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

    /// Initialize context with current KMS state (location + diagnostics).
    /// Must be called before the agent starts so that the agent loop's
    /// `inject_context_if_changed` fires once at startup (version 0 → 1).
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
impl AgentContext for KmsContext {
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
