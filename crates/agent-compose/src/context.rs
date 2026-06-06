//! [`AgentContext`] implementation backed by a [`kms::KmsService`].

use std::sync::Arc;

use agentik_core::context::{AgentContext, ContextDiagnostic, ContextSnapshot};
use async_trait::async_trait;
use dendrite_tools::ToolRegistration;

use crate::diagnostics::convert_diagnostics;
use crate::prompt::KMS_SYSTEM_PROMPT;
use crate::tools::is_mutation_tool;

pub struct KmsContext {
    kms: Arc<kms::KmsService>,
}

impl KmsContext {
    pub fn new(kms: Arc<kms::KmsService>) -> Self {
        Self { kms }
    }

    pub async fn from_path(db_path: &str) -> Result<Self, String> {
        let svc = kms::KmsService::new(db_path).await?;
        Ok(Self::new(Arc::new(svc)))
    }
}

#[async_trait]
impl AgentContext for KmsContext {
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
        KMS_SYSTEM_PROMPT.to_string()
    }

    fn tool_registrations(&self) -> Vec<ToolRegistration> {
        dendrite_tools::kms_tools::registrations(self.kms.clone())
    }
}
