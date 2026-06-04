//! # Design Principles
//!
//! Complete separation of tool invocation from tool execution within the agent system.
//! The core agent loop is behaviorally uniform across all agents — hardcoded logic provides
//! only generic capabilities (request–response cycling, lifecycle management, effect application)
//! and never encodes agent-specific behavior, tool selection, or prompt engineering at the
//! structural level. Agent personality and tooling are configured exclusively through the
//! toolset and system prompt, not through code paths.

use std::{sync::Arc, time::Duration, time::UNIX_EPOCH};

use crate::message_ext::AgentMessageExt;
use kms::Diagnostic;
use llm_api::model::model_pool::ModelPool;
use tracing::{Level, event, span};
use types::messages::{ContentBlock, Message, Role};
use types::tools::ToolUse;
use uuid::Uuid;

use types::ToolCallResponse;
use types::ToolCallResponseContent;

use crate::prompt::system_prompt_builder;
use crate::types::ToolEffect;

use crate::{
    error::{AgentError, Retryable},
    lifecycle::AgentLifecycle,
    memory::Memory,
    storage::{AgentSnapshot, AgentSnapshotStorage},
    toolset::{ToolRegistration, Toolset},
};

/// KMS tools that only read state and never mutate the knowledge tree.
const READONLY_KMS_TOOLS: &[&str] = &["kms_search_entity", "kms_navigate"];

fn format_diagnostics(issues: &[Diagnostic]) -> String {
    let mut lines = vec![format!("诊断发现 {} 个问题：", issues.len())];
    for d in issues {
        lines.push(format!(
            "[{}] {} — {} — {}",
            d.severity.label(),
            d.code,
            d.location,
            d.message
        ));
        for action in &d.suggested_actions {
            lines.push(format!("  → {}", action));
        }
    }
    lines.join("\n")
}

pub struct AgentConfig {
    pub max_iterations: usize,
    pub max_retries: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: 1000,
            max_retries: 10,
        }
    }
}

pub struct Agent {
    pub(crate) id: Uuid,
    pub(crate) model_pool: Arc<ModelPool>,
    pub(crate) memory: Memory,
    pub(crate) lifecycle: AgentLifecycle,
    pub(crate) toolset: Toolset,
    pub(crate) config: AgentConfig,
    pub(crate) storage: Option<Arc<dyn AgentSnapshotStorage>>,
    pub(crate) token_budget: TokenBudget,
    pub(crate) kms: Arc<kms::KmsService>,
    pub(crate) last_diagnostic_count: usize,
}

impl Agent {
    pub fn builder() -> crate::agent_builder::AgentBuilder {
        crate::agent_builder::AgentBuilder::new()
    }

    /// Register a single tool.
    pub fn register_tool(&mut self, registration: ToolRegistration) -> Result<(), AgentError> {
        self.toolset
            .register(registration)
            .map_err(AgentError::Tool)?;
        Ok(())
    }

    /// Register multiple tools at once.
    pub fn register_tools(
        &mut self,
        registrations: Vec<ToolRegistration>,
    ) -> Result<(), AgentError> {
        self.toolset
            .register_all(registrations)
            .map_err(AgentError::Tool)?;
        Ok(())
    }

    pub async fn snapshot(&self) -> AgentSnapshot {
        let snapshot = AgentSnapshot {
            ts: std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            agent_id: self.id,
            agent_status: *self.lifecycle.status(),
            memory: self.memory.clone(),
        };

        if let Some(storage) = self.storage.clone() {
            storage.as_ref().create_snapshot(snapshot.clone()).await;
        }

        snapshot
    }

    pub fn inject_message(&mut self, user_content: Vec<ContentBlock>) -> Result<(), AgentError> {
        let message = Message {
            id: Uuid::new_v4().to_string(),
            type_: "message".to_string(),
            role: Role::User,
            content: user_content,
            model: None,
            stop_reason: None,
            stop_sequence: None,
            usage: None,
            request_id: None,
        };
        self.memory.remember(message)?;
        Ok(())
    }

    pub async fn start(&mut self) -> Result<(), AgentError> {
        self.lifecycle.set_running();

        if let Ok(location) = self.kms.render_location().await {
            self.memory.remember(Message::user(location))?;
        }

        if let Ok(issues) = self.kms.diagnose().await {
            self.last_diagnostic_count = issues.len();
            if !issues.is_empty() {
                self.memory
                    .remember(Message::user(format_diagnostics(&issues)))?;
            }
        }

        let mut iteration = 0;
        let mut consecutive_retries = 0;
        let mut retry_feedback: Option<String> = None;

        while self.lifecycle.is_running() && iteration < self.config.max_iterations {
            iteration += 1;
            match self.agent_workflow(retry_feedback.take()).await {
                Ok(()) => consecutive_retries = 0,
                Err(e) if e.is_retryable() && consecutive_retries < self.config.max_retries => {
                    consecutive_retries += 1;
                    tracing::warn!(
                        "retryable error at iteration {}/{}, retry {}/{}: {e}",
                        iteration,
                        self.config.max_iterations,
                        consecutive_retries,
                        self.config.max_retries
                    );
                    let delay = Duration::from_secs(1) * (1 << (consecutive_retries - 1));
                    tracing::warn!("exponential backoff: sleeping {delay:?} before retry");
                    tokio::time::sleep(delay).await;

                    // 记录错误反馈信息
                    self.memory.remember(Message::user(e.retry_message()))?;

                    continue;
                }
                Err(e) => {
                    tracing::error!("{}", e.to_string());
                    return Err(AgentError::WorkflowFailed {
                        iteration,
                        error: Box::new(e),
                    });
                }
            }
        }

        if iteration >= self.config.max_iterations {
            return Err(AgentError::MaxIterations(self.config.max_iterations));
        }

        Ok(())
    }

    /// Agent核心工作流程
    ///
    /// 基本过程：构建上下文 -> 请求API -> 执行工具调用 -> 追加记忆
    async fn agent_workflow(&mut self, retry_feedback: Option<String>) -> Result<(), AgentError> {
        if let Some(feedback) = retry_feedback {
            self.inject_message(vec![ContentBlock::Text { text: feedback }])
                .unwrap();
        }

        let context = self.build_context().await?;
        let response_message = self.request(context).await?;
        event!(Level::INFO, "",);
        let last_usage = response_message.usage.clone().unwrap_or_default();

        self.token_budget.latest_usage = last_usage.input_tokens + last_usage.output_tokens;

        let toolcalls = self.extract_toolcalls(&response_message)?;

        self.memory.remember(response_message)?;

        if toolcalls.is_empty() {
            return Err(AgentError::NoneToolUse);
        }

        let pointer_before = self.kms.get_pointer().await;

        let tool_results = self.toolset.execute(&toolcalls).await?;
        dbg!(&tool_results);

        for tr in &tool_results {
            let text: String = tr
                .content
                .iter()
                .filter_map(|c| match c {
                    ToolCallResponseContent::Text(t) => Some(t.as_str()),
                    ToolCallResponseContent::Image(_) => None,
                })
                .collect::<Vec<_>>()
                .join("");
            self.memory.remember(Message::tool_result(
                tr.tool_use_id.clone(),
                text,
                tr.is_error.unwrap_or_default(),
            ))?;
        }

        // Re-run diagnostics after KMS mutating tool calls
        let has_kms_mutation = toolcalls.iter().any(|tc| {
            tc.name.starts_with("kms_") && !READONLY_KMS_TOOLS.contains(&tc.name.as_str())
        });
        if has_kms_mutation {
            if let Ok(issues) = self.kms.diagnose().await {
                if issues.len() != self.last_diagnostic_count {
                    self.last_diagnostic_count = issues.len();
                    if !issues.is_empty() {
                        self.memory
                            .remember(Message::user(format_diagnostics(&issues)))?;
                    } else {
                        self.memory
                            .remember(Message::user(String::from("诊断刷新：所有问题已修复。")))?;
                    }
                }
            }
        }

        self.handle_effect(&tool_results).await;

        if self.kms.get_pointer().await != pointer_before {
            if let Ok(location) = self.kms.render_location().await {
                self.memory.remember(Message::user(location))?;
            }
        }

        Ok(())
    }

    /// Apply agent-level effects declared by tool results (e.g. lifecycle transitions).
    async fn handle_effect(&mut self, tool_results: &[ToolCallResponse]) {
        let effects: Vec<ToolEffect> = tool_results
            .iter()
            .flat_map(|ts| ts.effects.clone())
            .collect();

        effects.iter().for_each(|e| match e {
            ToolEffect::AttemptComplete => {
                self.lifecycle.set_idle();
            }
            ToolEffect::Abort => {
                self.lifecycle.set_aborted();
            }
        });
    }

    async fn build_context(&mut self) -> Result<Vec<Message>, AgentError> {
        use crate::prompt::context::Context;

        let system_prompt = system_prompt_builder::SystemPromptBuilder::default()
            .build_identity()
            .build_kms()
            .parse();

        let context_messages = self.memory.render_context()?.to_vec();

        let context = Context::new()
            .with_system_prompt(system_prompt)
            .with_conversations(context_messages)
            .build();

        Ok(context)
    }

    async fn request(&mut self, context: Vec<Message>) -> Result<Message, AgentError> {
        let span = span!(Level::TRACE, "API Request");
        let _enter = span.enter();

        let model = self.model_pool.as_ref().get_model_roundrobin().unwrap();

        let est_totol_token = self.token_budget.estimate_total_token(context.len() as u64);

        if est_totol_token * 9 > (model.model_info.context_length * 10) {
            dbg!(est_totol_token, model.model_info.context_length);
            self.memory.compact(model.as_ref()).await?;
        }

        let response = model
            .request(context, self.toolset.tools().as_ref())
            .await?;

        dbg!(&response);

        Ok(response)
    }

    fn extract_toolcalls(&self, message: &Message) -> Result<Vec<ToolUse>, AgentError> {
        let toolcalls: Vec<ToolUse> = message
            .content
            .iter()
            .filter_map(|c| {
                if let ContentBlock::ToolUse { id, name, input } = c {
                    Some(ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();
        if toolcalls.is_empty() {
            return Err(AgentError::NoneToolUse);
        }

        Ok(toolcalls)
    }
}

#[derive(Default)]
pub struct TokenBudget {
    append_tokens: u64,
    latest_usage: u64,
}
impl TokenBudget {
    pub fn count_token_est(&self, msg: &Message) -> u64 {
        if let Some(usage) = &msg.usage {
            return usage.input_tokens;
        }

        let content_str = serde_json::to_string(&msg.content)
            .expect("Convert message to JSON string failed during counting token budget");

        content_str.len() as u64 / 4
    }

    pub fn increament_new_msg(&mut self, msg: &Message) {
        self.append_tokens = self.count_token_est(msg);
    }

    pub fn estimate_total_token(&self, system_prompt_token: u64) -> u64 {
        self.append_tokens + self.latest_usage + system_prompt_token
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::AgentLifecycleStatus;
    use async_trait::async_trait;
    use llm_api::model::model_pool::ModelPool;
    use llm_api::model::{Model, ModelInfo};
    use llm_api::provider::client::MockApiClient;
    use serde_json::Value;
    use tools::ToolFunction;
    use types::messages::Message;
    use types::shared::Usage;
    use types::tools::{ToolBuilder, ToolUse};

    fn dummy_model_info(name: &str) -> ModelInfo {
        ModelInfo {
            model_name: name.into(),
            provider: "test".into(),
            context_length: 200000,
            max_output_tokens: 1024,
            vision_ability: true,
            supports_function_calling: true,
            supports_streaming: true,
            supports_thinking: false,
            input_token_price: 1.0,
            output_token_price: 2.0,
        }
    }

    fn mock_model_pool(model_name: &str, mock_tool_calls: Vec<ToolUse>) -> ModelPool {
        let mut mock = MockApiClient::new();
        let mock_msgs: Vec<Message> = mock_tool_calls
            .iter()
            .cloned()
            .map(|t| Message::assistant_tool_use(t.id, t.name, t.input))
            .collect();

        let mock_usage = Usage {
            input_tokens: 1024,
            output_tokens: 128,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
            server_tool_use: None,
            service_tier: None,
        };
        let default_mock_msg =
            Message::assistant_tool_use("test_uuid", "test_tool", serde_json::Value::Null)
                .with_usage(mock_usage.clone());
        let mut msg_iter = mock_msgs.into_iter();

        mock.expect_request().returning(move |_, _, _| {
            let msg = msg_iter
                .next()
                .unwrap_or(default_mock_msg.clone())
                .clone()
                .with_usage(mock_usage.clone());
            Ok(msg)
        });

        let model = Model::new(dummy_model_info(model_name), mock);
        let mut pool = ModelPool::new();
        pool.add_model(model);
        pool
    }

    async fn get_test_agent(mock_tool_call: Vec<ToolUse>) -> Agent {
        let model_pool = mock_model_pool("test_model", mock_tool_call);

        Agent::builder()
            .with_model_pool(Arc::new(model_pool))
            .with_kms(Arc::new(
                kms::KmsService::new("sqlite::memory:").await.unwrap(),
            ))
            .build()
            .await
            .expect("failed to create test agent")
    }

    /// A mock tool that returns a simple text result.
    struct MockEchoTool;
    #[async_trait]
    impl ToolFunction for MockEchoTool {
        async fn execute(
            &self,
            _input: Value,
        ) -> Result<types::tools::ToolResult, Box<dyn std::error::Error + Send + Sync>> {
            Ok(types::tools::ToolResult::success("mock_id", "echo"))
        }
    }

    #[tokio::test]
    async fn test_agent_basic_execution() {
        let test_tool_def_1 = ToolBuilder::new("test_tool1", "A test tool for some operation")
            .parameter("task", "string", "say some thing")
            .build();

        let test_tool_def_2 = ToolBuilder::new(
            "attempt_complete",
            "Signal that the current task is complete",
        )
        .parameter("reason", "string", "reason for completion")
        .required("reason")
        .build();

        let mock_tool_calls = vec![
            ToolUse {
                id: "tc1".to_string(),
                name: test_tool_def_1.name.clone(),
                input: serde_json::Value::Null,
            },
            ToolUse {
                id: "tc2".to_string(),
                name: test_tool_def_2.name.clone(),
                input: serde_json::json!({ "reason": "task done" }),
            },
        ];

        let mut agent = get_test_agent(mock_tool_calls).await;

        agent
            .register_tools(vec![
                ToolRegistration {
                    definition: test_tool_def_1,
                    implementation: Box::new(MockEchoTool),
                    effects: vec![],
                },
                // attempt_complete_registration(),
            ])
            .unwrap();

        agent
            .inject_message(vec![ContentBlock::Text {
                text: "hello".to_string(),
            }])
            .unwrap();

        agent.start().await.unwrap();
        let snapshot = agent.snapshot().await;
        dbg!(&snapshot);
        // Verify the agent completed (IDLE) and produced conversation messages
        assert_eq!(snapshot.agent_status, AgentLifecycleStatus::IDLE);
        assert!(snapshot.memory.items.last().unwrap().messages.len() >= 3);
    }

    #[tokio::test]
    async fn test_agent_request() {
        let mut agent = get_test_agent(vec![ToolUse {
            id: "tc1".to_string(),
            name: "test_tool".to_string(),
            input: serde_json::Value::Null,
        }])
        .await;

        // Register a dummy tool so the model has tools available
        agent
            .register_tool(ToolRegistration {
                definition: ToolBuilder::new("test_tool", "A test tool").build(),
                implementation: Box::new(MockEchoTool),
                effects: vec![],
            })
            .unwrap();

        agent
            .inject_message(vec![ContentBlock::Text {
                text: "hello".to_string(),
            }])
            .unwrap();

        let msg = agent.build_context().await.unwrap();
        dbg!(msg);
    }
}
