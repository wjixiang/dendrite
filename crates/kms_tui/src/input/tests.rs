use agent_panel_tui::ChatMessage;
use std::collections::HashMap;

/// Mirror of the relevant slice of `spawn_agent_task`: snapshot
/// the compact input, expand the placeholders for the agent,
/// and return (compact_for_history, expanded_for_agent).
fn expand_for_agent(
    user_input: String,
    pastes: &mut Vec<(String, String)>,
) -> (String, String) {
    let compact_for_history = user_input.clone();
    let mut expanded = user_input;
    for (placeholder, full) in pastes.iter() {
        if expanded.contains(placeholder) {
            expanded = expanded.replacen(placeholder, full, 1);
        }
    }
    pastes.retain(|(placeholder, _)| expanded.contains(placeholder));
    (compact_for_history, expanded)
}

#[test]
fn chat_history_keeps_placeholder_after_submit() {
    let placeholder = "[Pasted ~3 lines]".to_string();
    let full = "alpha\nbeta\ngamma".to_string();
    let mut pastes = vec![(placeholder.clone(), full.clone())];
    let user_input = format!("please summarise: {placeholder} thanks");

    let (compact_for_history, expanded_for_agent) =
        expand_for_agent(user_input, &mut pastes);

    // The history-side string still contains the placeholder.
    assert!(compact_for_history.contains(&placeholder));
    assert!(
        !compact_for_history.contains(&full),
        "history must NOT contain the full pasted text"
    );

    // The agent-side string has the placeholder replaced by the
    // full text.
    assert!(expanded_for_agent.contains(&full));
    assert!(!expanded_for_agent.contains(&placeholder));
}

#[test]
fn no_paste_means_history_equals_input() {
    // No paste side-channel: history and agent both see the
    // verbatim text the user typed.
    let mut pastes: Vec<(String, String)> = Vec::new();
    let user_input = "just a short message".to_string();
    let (compact, expanded) = expand_for_agent(user_input.clone(), &mut pastes);
    assert_eq!(compact, "just a short message");
    assert_eq!(expanded, "just a short message");
}

#[test]
fn side_channel_placeholder_not_in_input_is_dropped() {
    // The placeholder is not in the input (user deleted it
    // before submitting). The side-channel entry is dropped —
    // `spawn_agent_task` does not retain orphans since the
    // `mem::take` is unconditional and unused entries serve no
    // further purpose. This is the simpler KISS behaviour.
    let placeholder = "[Pasted ~3 lines]".to_string();
    let full = "alpha\nbeta\ngamma".to_string();
    let mut pastes = vec![(placeholder.clone(), full.clone())];
    let (compact, expanded) = expand_for_agent(
        "no placeholder here, user deleted it".to_string(),
        &mut pastes,
    );
    assert_eq!(compact, "no placeholder here, user deleted it");
    assert_eq!(expanded, "no placeholder here, user deleted it");
    // Side-channel entry is dropped.
    assert!(pastes.is_empty());
}

#[test]
fn message_record_carries_compact_form() {
    // Drives the actual `ChatMessage::User` shape used by the
    // TUI. This is what ends up in the chat history; rendering
    // will see the placeholder, not the full text.
    let placeholder = "[Pasted ~3 lines]".to_string();
    let full = "alpha\nbeta\ngamma".to_string();
    let mut pastes = vec![(placeholder.clone(), full.clone())];
    let user_input = format!("intro {placeholder} outro");
    let (compact, _expanded) = expand_for_agent(user_input, &mut pastes);
    let mut messages: HashMap<crate::state::AgentKind, Vec<ChatMessage>> = HashMap::new();
    messages.insert(crate::state::AgentKind::Compose, Vec::new());
    let history = messages
        .get_mut(&crate::state::AgentKind::Compose)
        .unwrap();
    history.push(ChatMessage::User { text: compact });
    history.push(ChatMessage::Divider);

    match &history[0] {
        ChatMessage::User { text } => {
            assert!(text.contains(&placeholder));
            assert!(!text.contains(&full));
        }
        _ => panic!(),
    }
}

#[test]
fn default_focus_is_messages() {
    use crate::state::ChatFocus;
    assert_eq!(ChatFocus::default(), ChatFocus::Messages);
}
