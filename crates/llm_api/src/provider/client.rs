use types::messages::{ContentBlock, Message, Role};
use types::tools::ToolDefinition;
use types::errors::AnthropicError;
use types::messages::{MessageContent, ContentBlockParam, MessageCreateBuilder};
use crate::Anthropic;
use crate::model::ModelInfo;
use async_trait::async_trait;
use mockall::automock;

#[automock]
#[async_trait]
pub trait ApiClient: Send + Sync {
    async fn request(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        model_info: &ModelInfo,
    ) -> Result<Message, AnthropicError>;
    async fn test_connection(&self) -> Result<(), AnthropicError>;
}

pub struct AnthropicApiClient {
    client: Anthropic,
}

impl AnthropicApiClient {
    pub fn new(client: Anthropic) -> Self {
        Self { client }
    }
}

fn content_block_to_param(block: ContentBlock) -> ContentBlockParam {
    match block {
        ContentBlock::Text { text } => ContentBlockParam::Text { text },
        ContentBlock::Thinking { thinking, signature } => {
            ContentBlockParam::Thinking { thinking, signature }
        }
        ContentBlock::Image { source } => ContentBlockParam::Image { source },
        ContentBlock::ToolUse { id, name, input } => ContentBlockParam::ToolUse { id, name, input },
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => ContentBlockParam::ToolResult {
            tool_use_id,
            content,
            is_error,
        },
    }
}

fn message_to_content(msg: Message) -> MessageContent {
    MessageContent::Blocks(msg.content.into_iter().map(content_block_to_param).collect())
}

fn is_tool_result_message(msg: &Message) -> bool {
    msg.content.iter().any(|c| matches!(c, ContentBlock::ToolResult { .. }))
}

#[async_trait]
impl ApiClient for AnthropicApiClient {
    async fn request(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        model_info: &ModelInfo,
    ) -> Result<Message, AnthropicError> {
        let mut builder = MessageCreateBuilder::new(
            model_info.model_name.clone(),
            model_info.max_output_tokens as u32,
        );

        for msg in &messages {
            match msg.role {
                Role::User => {
                    if is_tool_result_message(msg) {
                        builder = builder.message(Role::User, message_to_content(msg.clone()));
                    } else {
                        builder = builder.message(Role::User, message_to_content(msg.clone()));
                    }
                }
                Role::Assistant => {
                    builder = builder.message(Role::Assistant, message_to_content(msg.clone()));
                }
            }
        }

        builder = builder.tools(tools);

        let params = builder.build();
        self.client.messages().create(params).await
    }

    async fn test_connection(&self) -> Result<(), AnthropicError> {
        self.client.test_connection().await
    }
}
