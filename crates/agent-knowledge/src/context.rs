use std::sync::Arc;

use agentik_core::context::{AgentContext, ContextDiagnostic, ContextSnapshot};
use async_trait::async_trait;
use dendrite_tools::ToolRegistration;

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
    async fn on_startup_location(&self) -> Result<Option<String>, String> {
        let location = self.kms.render_location().await?;
        Ok(Some(location))
    }

    async fn on_startup_diagnostics(&self) -> Result<Vec<ContextDiagnostic>, String> {
        Ok(vec![])
    }

    async fn take_snapshot(&self) -> Result<ContextSnapshot, String> {
        Ok(ContextSnapshot::new(self.kms.get_pointer().await))
    }

    fn is_mutation_tool(&self, _tool_name: &str) -> bool {
        false
    }

    async fn on_mutation_diagnostics(&self) -> Result<Vec<ContextDiagnostic>, String> {
        Ok(vec![])
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
        KNOWLEDGE_RETRIEVAL_PROMPT.to_string()
    }

    fn tool_registrations(&self) -> Vec<ToolRegistration> {
        dendrite_tools::kms_tools::readonly_registrations(self.kms.clone())
    }
}
