pub mod diagnostics;
pub mod document;
mod language;
pub mod service;
pub mod storage;
pub mod view;

pub use diagnostics::{CodeDescription, Diagnostic, Severity};
pub use document::{ChunkHit, Document, DocumentChunk, DEFAULT_CHUNK_OVERLAP, DEFAULT_CHUNK_SIZE, chunk_text};
pub use language::Language;
pub use service::KmsService;
pub use service::EntityFilter;
pub use storage::Storage;
pub use storage::error::StorageError;
pub use storage::types::{Entity, Index, Knowledge, KnowledgeType, Nomenclature, TargetType};
pub use view::{IndexView, LocalView, SubtreeSummary, SUBTREE_TITLES_LIMIT};
