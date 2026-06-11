//! Error types for the corpus crate.
//!
//! All public errors are decoupled from any specific backend
//! implementation. Backends wrap their own errors into
//! [`BoxError`]; the corpus module never leaks `sqlx::Error` or
//! any other backend-specific type through its public surface.

use thiserror::Error;
use uuid::Uuid;

/// A boxed, type-erased error that any backend can produce.
///
/// Equivalent to `Box<dyn std::error::Error + Send + Sync + 'static>`.
/// Backends (SQLite, in-memory, remote) all funnel their native
/// errors through this type so callers see a uniform interface.
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug, Error)]
pub enum CorpusError {
    #[error("backend error: {0}")]
    Backend(#[source] BoxError),

    #[error("document not found: {0}")]
    DocumentNotFound(Uuid),
}

impl From<BoxError> for CorpusError {
    fn from(e: BoxError) -> Self {
        CorpusError::Backend(e)
    }
}
