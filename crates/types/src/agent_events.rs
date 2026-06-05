use serde_json::Value;

/// Events emitted by the agent for external observation (e.g. TUI, logging).
/// Defined in the shared `types` crate to decouple from both agent business logic
/// and any specific UI layer.
#[derive(Debug, Clone)]
pub enum AgentUiEvent {
    /// LLM produced a text response.
    LlmResponse(String),
    /// LLM produced a thinking/reasoning block.
    Thinking(String),
    /// Agent is about to call the LLM API (waiting for response).
    Requesting,
    /// Agent is calling a tool. `input` carries the raw JSON arguments.
    ToolCall { name: String, input: Value },
    /// A tool returned a result. `content` is the raw text from the tool.
    ToolResult { ok: bool, content: String },
    /// Agent finished its workflow.
    Done,
    /// An error occurred.
    Error(String),
}
