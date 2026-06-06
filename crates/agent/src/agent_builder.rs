use std::sync::Arc;

use llm_api::model::model_pool::ModelPool;
use uuid::Uuid;

use crate::agent::{Agent, AgentConfig, TokenBudget};
use crate::error::AgentError;
use crate::storage::AgentSnapshotStorage;
use crate::{lifecycle::AgentLifecycle, memory::Memory, toolset::Toolset};

pub struct AgentBuilder {
    model_pool: Option<Arc<ModelPool>>,
    kms: Option<Arc<kms::KmsService>>,
    kms_db_path: Option<String>,
    config: AgentConfig,
    storage: Option<Arc<dyn AgentSnapshotStorage>>,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            model_pool: None,
            kms: None,
            kms_db_path: None,
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

    pub fn with_kms(mut self, kms: Arc<kms::KmsService>) -> Self {
        self.kms = Some(kms);
        self
    }

    pub fn with_kms_path(mut self, path: impl Into<String>) -> Self {
        self.kms_db_path = Some(path.into());
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
        let kms = if let Some(kms) = self.kms {
            kms
        } else {
            let db_path = self
                .kms_db_path
                .ok_or_else(|| AgentError::MissingConfig("kms or kms_db_path".to_string()))?;
            Arc::new(
                kms::KmsService::new(&db_path)
                    .await
                    .map_err(AgentError::MissingConfig)?,
            )
        };

        let mut toolset = Toolset::default();
        toolset.register_all(tools::registrations::lifecycle_registrations())?;
        toolset.register_all(tools::registrations::kms_registrations(kms.clone()))?;

        Ok(Agent {
            id: Uuid::new_v4(),
            model_pool,
            memory: Memory::new(),
            toolset,
            lifecycle: AgentLifecycle::new(),
            config: self.config,
            storage: self.storage,
            token_budget: TokenBudget::default(),
            kms,
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
