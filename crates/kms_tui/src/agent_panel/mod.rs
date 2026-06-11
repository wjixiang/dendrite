//! Agent Status Panel — displays all agents managed by `ProcessManager`.
//!
//! Driven by `agentik_core::ProcessEvent` instead of the old
//! `ParallelProgress` mpsc side-channel. Shows a list of managed agents
//! with their status, streaming text, tool calls, and event logs.
//!
//! This is a standalone panel (focusable via Tab), not embedded in the
//! chat history like the old `ParallelBlock`.
//!
//! # Module layout
//!
//! - [`state`] — data types (`AgentPanelState` / `AgentPanelEntry` /
//!   `AgentEntryStatus` / `AgentPanelEvent`) and the state-machine
//!   methods that mutate them.
//! - [`events`] — translation from `ProcessEvent` / `AgentEvent` into
//!   panel-internal events.
//! - [`tools`] — tool-name → user-facing-string rendering, used by
//!   `activity_hint` and the per-event renderer.
//! - [`theme`] — the `AgentPanelTheme` trait the renderer depends on,
//!   plus the blanket impl for the host's `Theme` type.
//! - [`renderer`] — pure render functions that turn panel state into
//!   `Line`s and write them to a `Frame`.
//!
//! The host crate is expected to drive the panel by calling
//! [`AgentPanelState::apply_process_event`] (or [`AgentPanelState::add_agent`])
//! for every process event, and to call [`render_agent_panel`] on each
//! frame.

mod events;
mod renderer;
mod state;
pub(crate) mod theme;
mod tools;

#[cfg(test)]
mod tests;

pub(crate) use renderer::render_agent_panel;
pub(crate) use state::AgentPanelState;

/// Theme surface used by the panel's renderer.
///
/// This trait deliberately exposes only the fields and style methods
/// the renderer actually touches. It exists so the panel can be
/// reused by a host whose theme has different field names or whose
/// `Style` builder is parameterized differently — the host just
/// provides a new `impl AgentPanelTheme for MyTheme`, no field
/// surgery required.
///
/// A blanket impl for `Theme` (the host's existing global theme
/// type) keeps every current call site compiling unchanged. The
/// renderer still takes `&Theme` today; switching it to
/// `&dyn AgentPanelTheme` is the B/C upgrade's first step.
///
/// `#[allow(dead_code)]` — no caller uses the trait yet. This is
/// groundwork; remove the allow when the renderer switches to
/// `&dyn AgentPanelTheme`.
#[allow(dead_code)]
pub trait AgentPanelTheme {
    // Foreground colors the renderer reaches for directly.
    fn text_primary(&self) -> ratatui::style::Color;
    fn text_secondary(&self) -> ratatui::style::Color;
    fn text_muted(&self) -> ratatui::style::Color;
    fn spinner(&self) -> ratatui::style::Color;
    fn tool_ok(&self) -> ratatui::style::Color;
    fn tool_err(&self) -> ratatui::style::Color;

    // Reusable styles. Returning `ratatui::style::Style` keeps the
    // renderer free of any host-specific style type.
    fn error_style(&self) -> ratatui::style::Style;
    fn success_style(&self) -> ratatui::style::Style;
    fn tool_call_bold_style(&self) -> ratatui::style::Style;
}
