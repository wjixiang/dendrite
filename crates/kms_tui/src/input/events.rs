use agentik_types::AgentUiEvent;

use crate::chat::ChatMessage;
use crate::state::App;

/// Convert a high-level agent event to one or more chat messages.
pub fn agent_event_to_message(event: AgentUiEvent) -> Vec<ChatMessage> {
    match event {
        AgentUiEvent::LlmResponse(text) => {
            vec![ChatMessage::Assistant { text, streaming: false }]
        }
        AgentUiEvent::Thinking(text) => {
            vec![ChatMessage::Thinking { text, streaming: false }]
        }
        AgentUiEvent::ToolCall { name, input } => {
            if name == "kms_parallel_dispatch" {
                vec![
                    ChatMessage::ToolCall { name, input },
                    ChatMessage::ParallelBlock,
                ]
            } else {
                vec![ChatMessage::ToolCall { name, input }]
            }
        }
        AgentUiEvent::ToolResult { ok, content } => vec![ChatMessage::ToolResult { ok, content }],
        AgentUiEvent::Done => vec![ChatMessage::Done],
        AgentUiEvent::Error(msg) => vec![ChatMessage::Error { message: msg }],
        AgentUiEvent::Requesting => Vec::new(),
        AgentUiEvent::TextDelta(_)
        | AgentUiEvent::ThinkingDelta(_)
        | AgentUiEvent::UsageUpdate { .. }
        | AgentUiEvent::StreamStart { .. }
        | AgentUiEvent::ContentBlockStart { .. }
        | AgentUiEvent::ContentBlockStop { .. }
        | AgentUiEvent::StreamDelta { .. } => Vec::new(),
    }
}

/// Append a text delta token to the last streaming `Assistant` message in
/// the current agent's chat history. If no streaming assistant message
/// exists, a new one is created.
pub fn append_to_streaming_assistant(app: &mut App, token: &str) {
    let kind = app.agent_kind;
    let history = app.agent_messages_map.get_mut(&kind).unwrap();

    let found = history.iter_mut().rev().find(|m| {
        matches!(m, ChatMessage::Assistant { streaming: true, .. })
    });
    if let Some(ChatMessage::Assistant { text, .. }) = found {
        text.push_str(token);
    } else {
        history.push(ChatMessage::Assistant {
            text: token.to_string(),
            streaming: true,
        });
    }
}

/// Append a thinking delta token to the last streaming `Thinking` message.
pub fn append_to_streaming_thinking(app: &mut App, token: &str) {
    let kind = app.agent_kind;
    let history = app.agent_messages_map.get_mut(&kind).unwrap();

    let found = history.iter_mut().rev().find(|m| {
        matches!(m, ChatMessage::Thinking { streaming: true, .. })
    });
    if let Some(ChatMessage::Thinking { text, .. }) = found {
        text.push_str(token);
    } else {
        history.push(ChatMessage::Thinking {
            text: token.to_string(),
            streaming: true,
        });
    }
}

/// Finalize any trailing streaming messages (set `streaming = false`).
/// Walks from the end of history and stops at the first non-Assistant/
/// non-Thinking variant so we only affect the most recent cluster.
pub fn finalize_streaming_history(history: &mut Vec<ChatMessage>) {
    for msg in history.iter_mut().rev() {
        match msg {
            ChatMessage::Assistant { streaming, .. }
            | ChatMessage::Thinking { streaming, .. } => {
                if *streaming {
                    *streaming = false;
                }
            }
            _ => break,
        }
    }
}

/// Handle a non-delta, non-lifecycle event (LlmResponse, Thinking,
/// ToolCall, ToolResult, Error). For `LlmResponse`/`Thinking`, we
/// finalize the in-flight streaming message rather than pushing a
/// duplicate. For tool events, we finalize streaming first, then push
/// the new message.
pub fn handle_final_event(app: &mut App, event: agentik_types::AgentUiEvent) {
    let kind = app.agent_kind;
    let history = app.agent_messages_map.get_mut(&kind).unwrap();

    match event {
        AgentUiEvent::LlmResponse(text) => {
            let finalized = history.iter_mut().rev().find(|m| {
                matches!(m, ChatMessage::Assistant { streaming: true, .. })
            });
            if let Some(ChatMessage::Assistant {
                text: streaming_text,
                streaming,
            }) = finalized
            {
                *streaming_text = text;
                *streaming = false;
            } else {
                history.push(ChatMessage::Assistant {
                    text,
                    streaming: false,
                });
            }
        }
        AgentUiEvent::Thinking(text) => {
            let finalized = history.iter_mut().rev().find(|m| {
                matches!(m, ChatMessage::Thinking { streaming: true, .. })
            });
            if let Some(ChatMessage::Thinking {
                text: streaming_text,
                streaming,
            }) = finalized
            {
                *streaming_text = text;
                *streaming = false;
            } else {
                history.push(ChatMessage::Thinking {
                    text,
                    streaming: false,
                });
            }
        }
        event => {
            finalize_streaming_history(history);
            let messages = agent_event_to_message(event);
            for msg in messages {
                history.push(msg);
            }
        }
    }
}
