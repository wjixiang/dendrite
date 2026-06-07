//! Knowledge construction expert agent for the `agentik-core` runtime.
//!
//! This crate wires the domain-specific KMS service into the runtime-agnostic
//! [`agentik_core::context::AgentContext`] contract. It is intentionally thin
//! and is organised as follows:
//!
//! - [`context`]: the [`KmsContext`] implementation of [`AgentContext`].
//! - [`diagnostics`]: conversion from KMS diagnostics to runtime diagnostics.
//! - [`prompt`]: the system-prompt section injected into KMS-aware agents.
//! - [`tools`]: classification of KMS tools (read-only vs. mutation).
//! - [`subtree_context`]: [`SubTreeComposeContext`] for parallel sub-agents.
//! - [`parallel_context`]: [`ParallelComposeContext`] for the orchestrator.

mod context;
mod diagnostics;
mod parallel_context;
mod parallel_prompt;
mod prompt;
mod subtree_context;
mod subtree_prompt;
mod tools;

pub use context::KmsContext;
pub use parallel_context::ParallelComposeContext;
pub use subtree_context::SubTreeComposeContext;
