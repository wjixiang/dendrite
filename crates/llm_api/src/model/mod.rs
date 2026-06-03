pub mod model_pool;

use std::sync::Arc;

use crate::provider::client::ApiClient;
use types::errors::AnthropicError;
use types::messages::Message;
use types::tools::ToolDefinition;

#[derive(Clone, Debug, Default)]
pub struct ModelInfo {
    pub model_name: String,
    pub provider: String,
    pub context_length: u64,
    pub max_output_tokens: u64,
    pub vision_ability: bool,
    pub supports_function_calling: bool,
    pub supports_streaming: bool,
    pub supports_thinking: bool,
    pub input_token_price: f64,
    pub output_token_price: f64,
}

impl std::fmt::Display for ModelInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.model_name)
    }
}

pub struct Model {
    pub model_info: ModelInfo,
    client: Arc<dyn ApiClient>,
}

impl Model {
    pub fn new(model_info: ModelInfo, client: impl ApiClient + 'static) -> Self {
        Self {
            model_info,
            client: Arc::new(client),
        }
    }
    pub fn vision(mut self, enabled: bool) -> Self {
        self.model_info.vision_ability = enabled;
        self
    }
    pub fn set_context_window(mut self, window: u64) -> Self {
        self.model_info.context_length = window;
        self
    }

    pub fn context_length(&self) -> u64 {
        self.model_info.context_length
    }

    pub async fn request(
        &self,
        messages: Vec<Message>,
        tools: &[ToolDefinition],
    ) -> Result<Message, AnthropicError> {
        let response = self
            .client
            .request(messages, tools.to_vec(), &self.model_info)
            .await?;
        Ok(response)
    }
}
