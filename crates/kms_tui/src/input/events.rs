use agentik_types::AgentEvent;

use crate::chat::ChatMessage;
use crate::state::App;

/// Convert a high-level agent event to one or more chat messages.
pub fn agent_event_to_message(event: AgentEvent) -> Vec<ChatMessage> {
    match event {
        AgentEvent::LlmResponse(text) => {
            vec![ChatMessage::Assistant { text, streaming: false }]
        }
        AgentEvent::Thinking(text) => {
            vec![ChatMessage::Thinking { text, streaming: false }]
        }
        AgentEvent::ToolCall { name, input } => {
            if name == "kms_parallel_dispatch" {
                vec![
                    ChatMessage::ToolCall { name, input },
                    ChatMessage::ParallelBlock,
                ]
            } else {
                vec![ChatMessage::ToolCall { name, input }]
            }
        }
        AgentEvent::ToolResult { ok, content } => vec![ChatMessage::ToolResult { ok, content }],
        AgentEvent::Done => vec![ChatMessage::Done],
        AgentEvent::Error(msg) => vec![ChatMessage::Error { message: msg }],
        AgentEvent::Requesting => Vec::new(),
        AgentEvent::TextDelta(_)
        | AgentEvent::ThinkingDelta(_)
        | AgentEvent::UsageUpdate { .. }
        | AgentEvent::StreamStart { .. }
        | AgentEvent::ContentBlockStart { .. }
        | AgentEvent::ContentBlockStop { .. }
        | AgentEvent::StreamDelta { .. } => Vec::new(),
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
pub fn handle_final_event(app: &mut App, event: agentik_types::AgentEvent) {
    let kind = app.agent_kind;
    let history = app.agent_messages_map.get_mut(&kind).unwrap();

    match event {
        AgentEvent::LlmResponse(text) => {
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
        AgentEvent::Thinking(text) => {
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
