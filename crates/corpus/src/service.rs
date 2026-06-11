//! Public service API for the corpus crate.
//!
//! Backend selection is the only piece of configuration exposed
//! to callers. Construct a [`CorpusService`] via
//! [`CorpusService::open`] with a [`Backend`] variant. Once
//! constructed, all methods are backend-agnostic.

use std::sync::Arc;

use uuid::Uuid;

use crate::chunker::{
    self, ChunkHit, Document, DocumentChunk, DEFAULT_CHUNK_OVERLAP, DEFAULT_CHUNK_SIZE,
};
use crate::error::{BoxError, CorpusError};
use crate::repo::DocumentRepo;
use crate::storage::CorpusStorage;

/// Returns the current UTC time as an ISO-8601 string
/// (`YYYY-MM-DDTHH:MM:SSZ`). Uses `std::time::SystemTime` to avoid
/// adding a `chrono` dependency.
pub fn iso_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = d.as_secs();

    let mut days = (total_secs / 86400) as i64;
    let secs_of_day = (total_secs % 86400) as u32;
    let hh = secs_of_day / 3600;
    let mm = (secs_of_day % 3600) / 60;
    let ss = secs_of_day % 60;

    days += 719468;
    let era = if days >= 0 {
        days / 146097
    } else {
        (days - 146096) / 146097
    };
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 + doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Backend selection for the corpus data layer.
///
/// The enum is the single binding point between callers and the
/// underlying storage. To plug in a new backend (vector store,
/// remote service, in-memory test double), add a new variant here
/// and wire it up in [`CorpusService::open`].
#[derive(Clone)]
pub enum Backend {
    /// Local SQLite database at the given file path (or
    /// `sqlite::memory:` / `sqlite::memory:?cache=shared` for tests).
    Sqlite {
        /// Path to the database file. Use `:memory:` for an
        /// ephemeral in-process DB.
        path: String,
    },

    /// A caller-supplied `DocumentRepo` implementation. Use this to
    /// plug in custom backends without modifying the corpus crate.
    Custom(Arc<dyn DocumentRepo>),
}

/// Public service: orchestrates chunking + persistence for the
/// corpus module.
#[derive(Clone)]
pub struct CorpusService {
    storage: CorpusStorage,
}

impl CorpusService {
    /// **Factory function**: open a corpus service backed by the
    /// chosen backend. This is the only entry point that constructs
    /// a service from a caller-facing configuration.
    pub async fn open(backend: Backend) -> Result<Arc<Self>, CorpusError> {
        let storage = match backend {
            Backend::Sqlite { path } => CorpusStorage::open_sqlite(&path).await?,
            Backend::Custom(repo) => CorpusStorage::from_repo(repo),
        };
        Ok(Arc::new(Self { storage }))
    }

    /// Construct directly from a pre-built `DocumentRepo`. The
    /// storage is wrapped without any migration step.
    pub fn from_repo(repo: Arc<dyn DocumentRepo>) -> Arc<Self> {
        Arc::new(Self {
            storage: CorpusStorage::from_repo(repo),
        })
    }

    /// Ingest a long text document: split into chunks and persist.
    /// Returns the [`Document`] metadata.
    pub async fn ingest_document(
        &self,
        title: &str,
        source: Option<&str>,
        content: &str,
    ) -> Result<Document, String> {
        let id = Uuid::new_v4();
        let chunks = chunker::chunk_text(id, content, DEFAULT_CHUNK_SIZE, DEFAULT_CHUNK_OVERLAP);
        let char_count = content.chars().count();
        let chunk_count = chunks.len();
        let created_at = iso_now();

        self.storage
            .documents()
            .create_document(id, title, source, char_count, chunk_count, &created_at)
            .await
            .map_err(|e| e.to_string())?;

        for chunk in &chunks {
            self.storage
                .documents()
                .create_chunk(chunk)
                .await
                .map_err(|e| e.to_string())?;
        }

        Ok(Document {
            id,
            title: title.to_string(),
            source: source.map(|s| s.to_string()),
            char_count,
            chunk_count,
            created_at,
        })
    }

    /// List all stored documents (metadata only, no chunks).
    pub async fn list_documents(&self) -> Result<Vec<Document>, String> {
        self.storage
            .documents()
            .list_documents()
            .await
            .map_err(|e| e.to_string())
    }

    /// Get metadata for a single document.
    pub async fn get_document(&self, id: Uuid) -> Result<Document, String> {
        self.storage
            .documents()
            .get_document(id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("document not found: {id}"))
    }

    /// Get a single chunk by (doc_id, chunk_index).
    pub async fn get_document_chunk(
        &self,
        id: Uuid,
        chunk_index: usize,
    ) -> Result<DocumentChunk, String> {
        self.storage
            .documents()
            .get_chunk(id, chunk_index)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("chunk {chunk_index} not found in document {id}"))
    }

    /// Get a window of chunks: `[chunk_index - before, chunk_index + after]`.
    /// Automatically clamped to `[0, chunk_count)`.
    pub async fn get_document_chunk_window(
        &self,
        id: Uuid,
        chunk_index: usize,
        before: usize,
        after: usize,
    ) -> Result<Vec<DocumentChunk>, String> {
        let doc = self.get_document(id).await?;
        let start = chunk_index.saturating_sub(before);
        let end = (chunk_index + after).min(doc.chunk_count.saturating_sub(1));
        self.storage
            .documents()
            .get_chunks_window(id, start, end)
            .await
            .map_err(|e| e.to_string())
    }

    /// Search a document for a keyword. Returns the top `top_k`
    /// chunks ranked by occurrence count (descending).
    pub async fn search_document(
        &self,
        id: Uuid,
        keyword: &str,
        top_k: usize,
    ) -> Result<Vec<ChunkHit>, String> {
        let hits = self
            .storage
            .documents()
            .search_keyword(id, keyword)
            .await
            .map_err(|e| e.to_string())?;
        let _doc = self.get_document(id).await?;
        Ok(hits.into_iter().take(top_k).collect())
    }

    /// Delete a document and all its chunks.
    pub async fn delete_document(&self, id: Uuid) -> Result<(), String> {
        match self.storage.documents().delete_document(id).await {
            Ok(()) => Ok(()),
            Err(e) => {
                if let Some(corpus_err) = e.downcast_ref::<CorpusError>() {
                    if matches!(corpus_err, CorpusError::DocumentNotFound(_)) {
                        return Err(format!("document not found: {id}"));
                    }
                }
                Err(e.to_string())
            }
        }
    }

    /// Cheap existence check.
    pub async fn document_exists(&self, id: Uuid) -> bool {
        self.storage
            .documents()
            .document_exists(id)
            .await
            .unwrap_or(false)
    }
}

/// Type alias for callers that want to refer to a generic boxed
/// backend error without pulling in `Box<dyn Error>` themselves.
pub type BoxBackendError = BoxError;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::DocumentRepo;

    async fn setup() -> Arc<CorpusService> {
        CorpusService::open(Backend::Sqlite {
            path: "sqlite::memory:".to_string(),
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_ingest_and_read() {
        let svc = setup().await;
        let doc = svc
            .ingest_document("title", Some("src"), "Para one.\n\nPara two.")
            .await
            .unwrap();
        assert_eq!(doc.chunk_count, 1);
        assert!(svc.document_exists(doc.id).await);
        assert!(!svc.document_exists(Uuid::new_v4()).await);

        let listed = svc.list_documents().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, doc.id);

        let chunk = svc.get_document_chunk(doc.id, 0).await.unwrap();
        assert!(chunk.content.contains("Para one"));
    }

    #[tokio::test]
    async fn test_delete() {
        let svc = setup().await;
        let doc = svc
            .ingest_document("title", None, "hello world")
            .await
            .unwrap();
        assert!(svc.document_exists(doc.id).await);
        svc.delete_document(doc.id).await.unwrap();
        assert!(!svc.document_exists(doc.id).await);
    }

    /// Test the [`Backend::Custom`] factory path: a tiny in-memory
    /// repo implementation is enough to exercise the factory without
    /// touching SQLite.
    #[tokio::test]
    async fn test_custom_backend_factory() {
        use std::collections::HashMap;
        use std::sync::Mutex;
        use async_trait::async_trait;

        struct InMemoryRepo {
            docs: Mutex<HashMap<Uuid, Document>>,
            chunks: Mutex<HashMap<(Uuid, usize), DocumentChunk>>,
        }

        #[async_trait]
        impl DocumentRepo for InMemoryRepo {
            async fn create_document(
                &self,
                id: Uuid,
                title: &str,
                source: Option<&str>,
                char_count: usize,
                chunk_count: usize,
                created_at: &str,
            ) -> Result<(), BoxError> {
                self.docs.lock().unwrap().insert(
                    id,
                    Document {
                        id,
                        title: title.to_string(),
                        source: source.map(String::from),
                        char_count,
                        chunk_count,
                        created_at: created_at.to_string(),
                    },
                );
                Ok(())
            }
            async fn create_chunk(&self, chunk: &DocumentChunk) -> Result<(), BoxError> {
                self.chunks
                    .lock()
                    .unwrap()
                    .insert((chunk.document_id, chunk.index), chunk.clone());
                Ok(())
            }
            async fn list_documents(&self) -> Result<Vec<Document>, BoxError> {
                Ok(self.docs.lock().unwrap().values().cloned().collect())
            }
            async fn get_document(&self, id: Uuid) -> Result<Option<Document>, BoxError> {
                Ok(self.docs.lock().unwrap().get(&id).cloned())
            }
            async fn get_chunk(
                &self,
                doc_id: Uuid,
                chunk_index: usize,
            ) -> Result<Option<DocumentChunk>, BoxError> {
                Ok(self.chunks.lock().unwrap().get(&(doc_id, chunk_index)).cloned())
            }
            async fn get_chunks_window(
                &self,
                doc_id: Uuid,
                start: usize,
                end: usize,
            ) -> Result<Vec<DocumentChunk>, BoxError> {
                Ok(self
                    .chunks
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|((d, i), _)| *d == doc_id && *i >= start && *i <= end)
                    .map(|(_, c)| c.clone())
                    .collect())
            }
            async fn delete_document(&self, id: Uuid) -> Result<(), BoxError> {
                self.docs.lock().unwrap().remove(&id);
                self.chunks
                    .lock()
                    .unwrap()
                    .retain(|(d, _), _| *d != id);
                Ok(())
            }
            async fn search_keyword(
                &self,
                _doc_id: Uuid,
                _keyword: &str,
            ) -> Result<Vec<ChunkHit>, BoxError> {
                Ok(vec![])
            }
        }

        let repo: Arc<dyn DocumentRepo> = Arc::new(InMemoryRepo {
            docs: Mutex::new(HashMap::new()),
            chunks: Mutex::new(HashMap::new()),
        });
        let svc = CorpusService::from_repo(repo.clone());
        let doc = svc
            .ingest_document("custom", None, "hello world")
            .await
            .unwrap();
        assert!(svc.document_exists(doc.id).await);

        // Also exercise the open(Backend::Custom) factory.
        let svc2 = CorpusService::open(Backend::Custom(repo)).await.unwrap();
        assert!(svc2.document_exists(doc.id).await);
    }
}
