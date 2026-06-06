//! KMS adapter for the `agentik-core` runtime.
//!
//! This crate wires the domain-specific KMS service into the runtime-agnostic
//! [`agentik_core::context::AgentContext`] contract. It is intentionally thin
//! and is organised as follows:
//!
//! - [`context`]: the [`KmsContext`] implementation of [`AgentContext`].
//! - [`diagnostics`]: conversion from KMS diagnostics to runtime diagnostics.
//! - [`prompt`]: the system-prompt section injected into KMS-aware agents.
//! - [`tools`]: classification of KMS tools (read-only vs. mutation).

mod context;
mod diagnostics;
mod prompt;
mod tools;

pub use context::KmsContext;
