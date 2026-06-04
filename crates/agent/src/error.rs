use thiserror::Error;

use llm_api::model::model_pool::ModelPoolError;
use types::errors::AnthropicError;
use types::tools::ToolUse;

use crate::types::ToolError;

use crate::memory::MemoryError;

pub trait Retryable {
    fn is_retryable(&self) -> bool;
    fn retry_message(&self) -> String;
}

impl Retryable for AnthropicError {
    fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::BadRequest { .. }
                | Self::RateLimit { .. }
                | Self::InternalServer { .. }
                | Self::Connection { .. }
                | Self::ConnectionTimeout
                | Self::StreamError(_)
                | Self::Timeout
                | Self::NetworkError(_)
                | Self::ServiceUnavailable { .. }
        )
    }

    fn retry_message(&self) -> String {
        format!("The previous API request failed: {self}. Please retry.")
    }
}

impl Retryable for ToolError {
    fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::NotFound { .. } | Self::ValidationFailed { .. } | Self::ExecutionFailed { .. } | Self::Timeout { .. }
        )
    }

    fn retry_message(&self) -> String {
        format!("A tool execution failed: {self}. Please retry with corrected parameters.")
    }
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("ModelPool error occured")]
    ModelPool(#[from] ModelPoolError),

    #[error("ApiClient request error: {0}")]
    ApiRequestError(#[from] AnthropicError),

    #[error("Memory error occured")]
    MemoryError(#[from] MemoryError),

    #[error("Agent did not use any tools")]
    NoneToolUse,

    #[error("Unknown tool requested:  {0:?}. Existed tools: {1:?}")]
    UnknownTool(Vec<ToolUse>, Vec<types::Tool>),

    #[error("Tool error: {0}")]
    Tool(#[from] ToolError),

    #[error("Max iterations ({0}) reached")]
    MaxIterations(usize),

    #[error("workflow failed at iteration {iteration}: {error}")]
    WorkflowFailed {
        iteration: usize,
        #[source]
        error: Box<AgentError>,
    },

    #[error("missing required config: {0}")]
    MissingConfig(String),
}

impl Retryable for AgentError {
    fn is_retryable(&self) -> bool {
        match self {
            Self::ApiRequestError(e) => e.is_retryable(),
            Self::Tool(e) => e.is_retryable(),
            Self::NoneToolUse => true,
            _ => false,
        }
    }

    fn retry_message(&self) -> String {
        match self {
            Self::ApiRequestError(e) => e.retry_message(),
            Self::Tool(e) => e.retry_message(),
            Self::NoneToolUse => {
                "Your previous response did not include any tool calls. You MUST use at least one tool to proceed. Please retry with an appropriate tool call.".to_string()
            }
            _ => format!("An error occurred: {self}."),
        }
    }
}
