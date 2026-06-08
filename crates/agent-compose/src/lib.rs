//! Knowledge construction expert agent for the `agentik-core` runtime.
//!
//! This crate wires the domain-specific KMS service into the runtime-agnostic
//! [`agentik_core::context::AgentContext`] contract. It is intentionally thin
//! and is organised as follows:
//!
//! - [`context`]: the [`KmsContext`] implementation of [`AgentContext`].
//! - [`diagnostics`]: conversion from KMS diagnostics to runtime diagnostics.
//! - [`prompt`]: the system-prompt section injected into KMS-aware agents.
//! - [`subtree_context`]: [`SubTreeComposeContext`] for parallel sub-agents.
//! - [`parallel_context`]: [`ParallelComposeContext`] for the orchestrator.

mod context;
mod diagnostics;
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
