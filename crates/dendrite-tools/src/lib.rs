//! External (domain-specific) tools that run on the agentik-core runtime.
//!
//! These are tools that depend on a specific domain — in this case the
//! knowledge management system (KMS). They are intentionally kept out
//! of `agentik-core`, so that the framework remains domain-agnostic
//! and can be reused across different domains.

pub mod kms_tools;

pub use agentik_core::tools::ToolRegistration;
pub use kms_tools::parallel_registrations;
pub use kms_tools::readonly_registrations;
pub use kms_tools::registrations;
pub use kms_tools::SubAgentConfig;
