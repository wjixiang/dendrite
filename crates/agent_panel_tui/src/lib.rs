//! Reusable TUI panels for visualizing:
//!
//! - sub-agents managed by `agentik::ProcessManager` — see
//!   [`AgentPanelState`] and [`render_agent_panel`].
//! - a streaming chat conversation history — see
//!   [`ChatPanelState`], [`ChatMessage`], and [`render_chat_panel`].
//!
//! See the `mod` declarations below for the public surface.
//! This is the C-upgrade of the in-`kms_tui` panels; see the crate
//! `README` for the migration history.

#![allow(clippy::needless_lifetimes)] // trait method signatures may use them

// Chat module — owns ChatMessage, ChatPanelState, ChatPanelTheme,
// render_chat_panel, and the AgentEvent → ChatMessage translation
// helpers used by the host's event loop.
pub mod chat;

// Sub-agent module — owns AgentPanelState, AgentPanelTheme, etc.
mod events;
mod state;
mod theme;
mod tools;

#[cfg(test)]
mod tests;

// Renderer depends on the trait, so it has to be declared after
// `state` / `theme` / `tools`.
mod renderer;

// Public surface — kept narrow on purpose. Hosts wire a concrete
// `AgentPanelState` and call `render_agent_panel` per frame; theme
// and tool-name behavior are injected via the two traits.
pub use renderer::render_agent_panel;
pub use state::{
    AgentEntryLayout, AgentEntryStatus, AgentPanelEntry, AgentPanelEvent, AgentPanelState,
    MAX_VISIBLE_AGENTS, RECENT_COMPLETED_TTL_MS,
};
pub use theme::AgentPanelTheme;
pub use tools::{AgentPanelTools, DefaultAgentPanelTools};

// Re-export the chat panel's public surface at the crate root so
// hosts can write `agent_panel_tui::ChatMessage` rather than the
// longer `agent_panel_tui::chat::ChatMessage`. `DefaultChatPanelTheme`
// is intentionally NOT re-exported — hosts reach it via
// `agent_panel_tui::chat::theme::DefaultChatPanelTheme` to keep
// the public surface narrow (mirrors the `DefaultAgentPanelTheme`
// convention).
pub use chat::events::{
    agent_event_to_messages, append_streaming_assistant, append_streaming_thinking,
    finalize_streaming, handle_non_delta_event,
};
pub use chat::state::ChatPanelState;
pub use chat::theme::ChatPanelTheme;
pub use chat::ChatMessage;
pub use chat::renderer::render_chat_panel;

// Re-export the chat input / status row. `DefaultChatInputTheme`
// is NOT re-exported (mirrors the `DefaultChatPanelTheme` /
// `DefaultAgentPanelTheme` convention). `SPINNER_FRAMES` is
// re-exported because the sub-agent renderer also uses it.
pub use chat::input::{
    build_status_line, render_chat_input, ChatInputStatus, ChatInputTheme, RunningPhase,
    SPINNER_FRAMES,
};
