//! Translation from `agentik_sdk::types::AgentEvent` to
//! [`ChatMessage`] mutations.
//!
//! All functions are pure state-machine operations on a
//! `&mut Vec<ChatMessage>`; they have no dependency on any host
//! `App` struct. The host reads events off its channel, picks the
//! right function, and passes the *active* history.
//!
//! Streaming deltas (`TextDelta`, `ThinkingDelta`) accumulate into
//! the trailing streaming message in place rather than producing
//! one new `ChatMessage` per token. Non-delta events
//! (`LlmResponse`, `Thinking`, `ToolCall`, `ToolResult`, `Done`,
//! `Error`) finalize any in-flight streaming message first and
//! then append one or more `ChatMessage` items.

use agentik_sdk::types::AgentEvent;

use super::ChatMessage;

/// Convert a single non-delta `AgentEvent` into the
/// `ChatMessage`(s) that should be appended to the history.
///
/// Streaming events (`TextDelta`, `ThinkingDelta`, `UsageUpdate`,
/// `StreamStart/Stop`, `ContentBlockStart/Stop`, `StreamDelta`)
/// return an empty `Vec` — they're handled separately by
/// [`append_streaming_assistant`] / [`append_streaming_thinking`]
/// / [`handle_non_delta_event`] because they need to mutate the
/// trailing message in place.
pub fn agent_event_to_messages(event: &AgentEvent) -> Vec<ChatMessage> {
    match event {
        AgentEvent::LlmResponse(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![ChatMessage::Assistant {
                    text: text.clone(),
                    streaming: false,
                }]
            }
        }
        AgentEvent::Thinking(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![ChatMessage::Thinking {
                    text: text.clone(),
                    streaming: false,
                }]
            }
        }
        AgentEvent::ToolCall { name, input } => vec![ChatMessage::ToolCall {
            name: name.clone(),
            input: input.clone(),
        }],
        AgentEvent::ToolResult { ok, content } => vec![ChatMessage::ToolResult {
            ok: *ok,
            content: content.clone(),
            // Parse once at insertion time so the renderer never
            // re-parses on every frame.
            parsed: serde_json::from_str(content).ok(),
        }],
        AgentEvent::Done => vec![ChatMessage::Done],
        AgentEvent::Error(msg) => vec![ChatMessage::Error {
            message: msg.clone(),
        }],
        // Streaming noise — handled elsewhere.
        AgentEvent::Requesting
        | AgentEvent::TextDelta(_)
        | AgentEvent::ThinkingDelta(_)
        | AgentEvent::UsageUpdate { .. }
        | AgentEvent::StreamStart { .. }
        | AgentEvent::StreamDelta { .. }
        | AgentEvent::ContentBlockStart { .. }
        | AgentEvent::ContentBlockStop { .. } => Vec::new(),
    }
}

/// Append a text delta to the last streaming
/// [`ChatMessage::Assistant`] in the history. If no such message
/// exists (history is empty, or the trailing message is not a
/// streaming Assistant), a new streaming `Assistant` is pushed.
///
/// Returns `true` if the history was mutated.
pub fn append_streaming_assistant(history: &mut Vec<ChatMessage>, token: &str) -> bool {
    // We only ever look at the *last* message: streaming deltas
    // accumulate into the trailing block. Older streaming
    // blocks, if any exist, are left untouched.
    if let Some(ChatMessage::Assistant { text, .. }) = history.iter_mut().rev().next() {
        text.push_str(token);
        return true;
    }
    push_streaming_assistant(history, token)
}

/// Append a thinking delta to the last streaming
/// [`ChatMessage::Thinking`] in the history. If no such message
/// exists, a new streaming `Thinking` is pushed.
///
/// Returns `true` if the history was mutated.
pub fn append_streaming_thinking(history: &mut Vec<ChatMessage>, token: &str) -> bool {
    if let Some(ChatMessage::Thinking { text, .. }) = history.iter_mut().rev().next() {
        text.push_str(token);
        return true;
    }
    push_streaming_thinking(history, token)
}

fn push_streaming_assistant(history: &mut Vec<ChatMessage>, token: &str) -> bool {
    history.push(ChatMessage::Assistant {
        text: token.to_string(),
        streaming: true,
    });
    true
}

fn push_streaming_thinking(history: &mut Vec<ChatMessage>, token: &str) -> bool {
    history.push(ChatMessage::Thinking {
        text: token.to_string(),
        streaming: true,
    });
    true
}

/// Walk the history in reverse, setting `streaming = false` on
/// every trailing `Assistant`/`Thinking` message. Stops the
/// moment we hit a non-streaming variant. This is what gets
/// called right before a non-delta event appends a new message
/// so that a later `█` block cursor doesn't dangle on a message
/// that just got superseded.
pub fn finalize_streaming(history: &mut [ChatMessage]) {
    for msg in history.iter_mut().rev() {
        match msg {
            ChatMessage::Assistant { streaming, .. } if *streaming => *streaming = false,
            ChatMessage::Thinking { streaming, .. } if *streaming => *streaming = false,
            _ => break,
        }
    }
}

/// Handle a non-delta event: finalize any in-flight streaming
/// message, then append the converted `ChatMessage`(s).
///
/// For `LlmResponse` and `Thinking`, the function tries to fold
/// the final text into the trailing streaming message in place
/// (so the user sees a single uninterrupted block of text)
/// before falling back to pushing a new non-streaming message.
pub fn handle_non_delta_event(history: &mut Vec<ChatMessage>, event: &AgentEvent) {
    match event {
        AgentEvent::LlmResponse(text) => {
            if text.is_empty() {
                finalize_streaming(history);
                return;
            }
            // Try to fold into a trailing streaming Assistant
            // *before* finalizing, so the final text replaces
            // the in-progress text in place. We only ever look
            // at the last message; if it isn't a streaming
            // Assistant we fall through to push a new one.
            if let Some(ChatMessage::Assistant { streaming, text: slot }) =
                history.iter_mut().rev().next()
            {
                if *streaming {
                    *slot = text.clone();
                    *streaming = false;
                    return;
                }
            }
            finalize_streaming(history);
            history.push(ChatMessage::Assistant {
                text: text.clone(),
                streaming: false,
            });
        }
        AgentEvent::Thinking(text) => {
            if text.is_empty() {
                finalize_streaming(history);
                return;
            }
            if let Some(ChatMessage::Thinking { streaming, text: slot }) =
                history.iter_mut().rev().next()
            {
                if *streaming {
                    *slot = text.clone();
                    *streaming = false;
                    return;
                }
            }
            finalize_streaming(history);
            history.push(ChatMessage::Thinking {
                text: text.clone(),
                streaming: false,
            });
        }
        other => {
            // Any other non-delta event finalizes streaming first,
            // then appends whatever the converter produced.
            finalize_streaming(history);
            let converted = agent_event_to_messages(other);
            history.extend(converted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn history() -> Vec<ChatMessage> {
        vec![ChatMessage::Divider]
    }

    #[test]
    fn append_to_empty_streaming_assistant() {
        let mut h = history();
        append_streaming_assistant(&mut h, "hello ");
        append_streaming_assistant(&mut h, "world");
        assert_eq!(h.len(), 2);
        match &h[1] {
            ChatMessage::Assistant { text, streaming } => {
                assert_eq!(text, "hello world");
                assert!(*streaming);
            }
            _ => panic!("expected streaming Assistant"),
        }
    }

    #[test]
    fn append_creates_new_when_no_streaming_in_flight() {
        let mut h = history();
        // First finalize: there's nothing streaming, so the next
        // delta should create a new message.
        finalize_streaming(&mut h);
        append_streaming_assistant(&mut h, "a");
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn finalize_drops_streaming_flag() {
        let mut h = vec![ChatMessage::Assistant {
            text: "hi".into(),
            streaming: true,
        }];
        finalize_streaming(&mut h);
        match &h[0] {
            ChatMessage::Assistant { streaming, .. } => assert!(!*streaming),
            _ => panic!(),
        }
    }

    #[test]
    fn llm_response_folds_into_trailing_streaming_assistant() {
        let mut h = vec![ChatMessage::Assistant {
            text: "par".into(),
            streaming: true,
        }];
        handle_non_delta_event(
            &mut h,
            &AgentEvent::LlmResponse("partial-final-text".into()),
        );
        assert_eq!(h.len(), 1);
        match &h[0] {
            ChatMessage::Assistant { text, streaming } => {
                assert_eq!(text, "partial-final-text");
                assert!(!*streaming);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn llm_response_pushes_new_when_no_streaming_in_flight() {
        let mut h = vec![ChatMessage::Divider];
        handle_non_delta_event(&mut h, &AgentEvent::LlmResponse("hello".into()));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn tool_call_appends_after_finalize() {
        let mut h = vec![ChatMessage::Assistant {
            text: "x".into(),
            streaming: true,
        }];
        handle_non_delta_event(
            &mut h,
            &AgentEvent::ToolCall {
                name: "kms_local".into(),
                input: json!({ "path": "src/lib.rs" }),
            },
        );
        assert_eq!(h.len(), 2);
        assert!(matches!(
            h[0],
            ChatMessage::Assistant {
                streaming: false,
                ..
            }
        ));
        assert!(matches!(h[1], ChatMessage::ToolCall { .. }));
    }

    #[test]
    fn tool_result_pre_parses_json() {
        let mut h = Vec::new();
        handle_non_delta_event(
            &mut h,
            &AgentEvent::ToolResult {
                ok: true,
                content: r#"{"a": 1}"#.into(),
            },
        );
        match &h[0] {
            ChatMessage::ToolResult { ok, parsed, .. } => {
                assert!(*ok);
                assert_eq!(parsed.as_ref().unwrap(), &json!({ "a": 1 }));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn done_appends_done_marker() {
        let mut h = Vec::new();
        handle_non_delta_event(&mut h, &AgentEvent::Done);
        assert!(matches!(h[0], ChatMessage::Done));
    }

    #[test]
    fn error_appends_error() {
        let mut h = Vec::new();
        handle_non_delta_event(&mut h, &AgentEvent::Error("boom".into()));
        match &h[0] {
            ChatMessage::Error { message } => assert_eq!(message, "boom"),
            _ => panic!(),
        }
    }

    #[test]
    fn streaming_events_produce_no_messages() {
        let mut h = Vec::new();
        handle_non_delta_event(&mut h, &AgentEvent::Requesting);
        // StreamStart carries a `Message` which is heavy to
        // construct for a test; instead test the converter for
        // the lighter-weight streaming variants.
        let messages = agent_event_to_messages(&AgentEvent::ContentBlockStop { index: 0 });
        assert!(messages.is_empty());
        assert!(h.is_empty());
    }
}
