//! Data layer abstraction for the corpus.
//!
//! The trait [`DocumentRepo`] defines the operations a corpus backend
//! must support. Methods return domain types ([`Document`],
//! [`DocumentChunk`], [`ChunkHit`]) directly — backends are free to
//! use whatever row representation they like internally; the row
//! types do not appear on the trait surface.
//!
//! The default SQLite implementation lives in
//! [`crate::storage::sqlite`]. In-memory and remote backends can be
//! plugged in by implementing this trait.

use async_trait::async_trait;
use uuid::Uuid;

use crate::chunker::{ChunkHit, Document, DocumentChunk};
use crate::error::BoxError;

#[async_trait]
pub trait DocumentRepo: Send + Sync {
    async fn create_document(
        &self,
        id: Uuid,
        title: &str,
        source: Option<&str>,
        char_count: usize,
        chunk_count: usize,
        created_at: &str,
    ) -> Result<(), BoxError>;

    async fn create_chunk(&self, chunk: &DocumentChunk) -> Result<(), BoxError>;

    async fn list_documents(&self) -> Result<Vec<Document>, BoxError>;

    async fn get_document(&self, id: Uuid) -> Result<Option<Document>, BoxError>;

    async fn get_chunk(
        &self,
        doc_id: Uuid,
        chunk_index: usize,
    ) -> Result<Option<DocumentChunk>, BoxError>;

    async fn get_chunks_window(
        &self,
        doc_id: Uuid,
        start: usize,
        end: usize,
    ) -> Result<Vec<DocumentChunk>, BoxError>;

    async fn delete_document(&self, id: Uuid) -> Result<(), BoxError>;

    /// Search all chunks of a document for a keyword (case-insensitive
    /// substring). Returns `(chunk_index, snippet)` pairs sorted by
    /// the number of occurrences (descending). The snippet
    /// formatting is backend-defined.
    async fn search_keyword(
        &self,
        doc_id: Uuid,
        keyword: &str,
    ) -> Result<Vec<ChunkHit>, BoxError>;

    /// Returns `true` if a document with the given ID exists.
    async fn document_exists(&self, id: Uuid) -> Result<bool, BoxError> {
        Ok(self.get_document(id).await?.is_some())
    }
}
