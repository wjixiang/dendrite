use std::sync::Arc;

use llm_api::model::model_pool::ModelPool;
use uuid::Uuid;

use crate::agent::{Agent, AgentConfig, TokenBudget};
use crate::context::AgentContext;
use crate::error::AgentError;
use crate::storage::AgentSnapshotStorage;
use crate::{lifecycle::AgentLifecycle, memory::Memory, toolset::Toolset};

pub struct AgentBuilder {
    model_pool: Option<Arc<ModelPool>>,
    ctx: Option<Arc<dyn AgentContext>>,
    config: AgentConfig,
    storage: Option<Arc<dyn AgentSnapshotStorage>>,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            model_pool: None,
            ctx: None,
            config: AgentConfig::default(),
            storage: None,
        }
    }

    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_model_pool(mut self, pool: Arc<ModelPool>) -> Self {
        self.model_pool = Some(pool);
        self
    }

    pub fn with_context(mut self, ctx: Arc<dyn AgentContext>) -> Self {
        self.ctx = Some(ctx);
        self
    }

    pub fn with_storage(mut self, storage: Arc<dyn AgentSnapshotStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    pub async fn build(self) -> Result<Agent, AgentError> {
        let model_pool = self
            .model_pool
            .ok_or_else(|| AgentError::MissingConfig("model_pool".to_string()))?;
        let ctx = self
            .ctx
            .ok_or_else(|| AgentError::MissingConfig("context".to_string()))?;

        let mut toolset = Toolset::default();
        toolset.register_all(tools::lifecycle_registrations())?;
        toolset.register_all(ctx.tool_registrations())?;

        Ok(Agent {
            id: Uuid::new_v4(),
            model_pool,
            memory: Memory::new(),
            toolset,
            lifecycle: AgentLifecycle::new(),
            config: self.config,
            storage: self.storage,
            token_budget: TokenBudget::default(),
            ctx,
            last_diagnostic_count: 0,
            event_tx: None,
            current_model_name: None,
        })
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}