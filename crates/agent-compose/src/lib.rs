//! Knowledge construction expert agent for the `agentik-core` runtime.
//!
//! This crate wires the domain-specific KMS service into the runtime-agnostic
//! [`agentik_core::context::AgentContext`] contract. It is intentionally thin
//! and is organised as follows:
//!
//! - [`context`]: the [`KmsContext`] implementation of [`AgentContext`].
//! - [`prompt`]: the system-prompt section injected into KMS-aware agents.
//! - [`subtree_context`]: [`SubTreeComposeContext`] for parallel sub-agents.
//! - [`parallel_context`]: [`ParallelComposeContext`] for the orchestrator.
//!
//! All three contexts follow the **stateless query model** used by
//! [`agent_knowledge`]: `initialize()` injects a one-shot `local_view` of the
//! index root (or the staging subtree for sub-agents) at version 1, and
//! `write()` is a no-op. There is no per-tool-call re-injection of the
//! global pointer, the rendered "location" block, or the diagnostic
//! snapshot — agents inspect the tree on demand via `kms_view_local`.

mod context;
mod parallel_context;
mod parallel_prompt;
mod prompt;
mod subtree_context;
mod subtree_prompt;

pub use context::KmsContext;
pub use parallel_context::ParallelComposeContext;
pub use subtree_context::SubTreeComposeContext;

pub use parallel_prompt::PARALLEL_COMPOSE_PROMPT;
pub use prompt::KMS_SYSTEM_PROMPT;
pub use subtree_prompt::SUBTREE_COMPOSE_PROMPT;
