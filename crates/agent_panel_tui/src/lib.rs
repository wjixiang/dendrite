//! Reusable TUI panel for visualizing sub-agents managed by
//! `agentik::ProcessManager`.
//!
//! See the `mod` declarations below for the public surface.
//! This is the C-upgrade of the in-`kms_tui` panel; see the crate
//! `README` for the migration history.

#![allow(clippy::needless_lifetimes)] // trait method signatures may use them

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
