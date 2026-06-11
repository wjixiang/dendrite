//! Corpus — backend-agnostic raw-corpus management module.
//!
//! Long user-pasted text is split into chunks and stored so the LLM
//! only ever sees lightweight references (`[doc:uuid, chunks=N]`)
//! in its context window, fetching slices on demand through the
//! `corpus_*` tools.
//!
//! # Public surface
//!
//! - [`CorpusService`] — public API used by tools, contexts, and the
//!   TUI. Constructed via [`CorpusService::open`] with a [`Backend`]
//!   variant.
//! - [`Backend`] — enum selecting the data backend. The only
//!   binding point between callers and a specific storage technology.
//! - [`DocumentRepo`] — trait for pluggable backends. The default
//!   SQLite implementation lives in `storage::sqlite` and is the
//!   only module in the crate that references sqlx directly.
//! - [`chunker`] — chunking algorithm and domain types
//!   ([`Document`], [`DocumentChunk`], [`ChunkHit`]).
//! - [`CorpusError`] / [`BoxError`] — backend-agnostic error types.

pub mod chunker;
pub mod error;
pub mod repo;
pub mod service;
pub(crate) mod storage;

/// **Test-only hook**: exposes the SQLite backend's repo and
/// migration helper so that integration tests in other crates can
/// share an in-memory DB with the corpus service without going
/// through [`Backend::Sqlite`]. Hidden from rustdoc and the public
/// API surface.
#[doc(hidden)]
pub mod __sqlite_backend {
    pub use crate::storage::sqlite::{SqliteDocumentRepo, run_migrations_on_pool};
}

pub use chunker::{
    ChunkHit, Document, DocumentChunk, DEFAULT_CHUNK_OVERLAP, DEFAULT_CHUNK_SIZE, chunk_text,
};
pub use error::{BoxError, CorpusError};
pub use repo::DocumentRepo;
pub use service::{Backend, CorpusService, BoxBackendError, iso_now};
