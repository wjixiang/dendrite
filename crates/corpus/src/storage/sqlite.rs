//! SQLite implementation of [`DocumentRepo`].
//!
//! This module is the **only** place in the corpus crate that
//! references sqlx directly. Row types ([`DocumentRow`],
//! [`DocumentChunkRow`]) and the [`SqliteDocumentRepo`] type are
//! private to the module; the trait surface returns only domain
//! types ([`Document`], [`DocumentChunk`], [`ChunkHit`]).

use async_trait::async_trait;
use sqlx::{Pool, Sqlite, SqlitePool, sqlite::SqlitePoolOptions};
use uuid::Uuid;

use crate::chunker::{ChunkHit, Document, DocumentChunk, DEFAULT_CHUNK_SIZE};
use crate::error::BoxError;
use crate::repo::DocumentRepo;

/// Build a `SqlitePool` for the given DB path (file path or
/// `sqlite::memory:` / `sqlite::memory:?cache=shared`).
pub(crate) async fn create_pool(db_path: &str) -> Result<Pool<Sqlite>, BoxError> {
    let url = if db_path.starts_with("sqlite:") {
        db_path.to_string()
    } else if db_path.contains('?') {
        // Caller already supplied query params (e.g. shared in-memory
        // test DBs). Pass through verbatim.
        format!("sqlite://{db_path}")
    } else {
        format!("sqlite://{db_path}?mode=rwc")
    };
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await?;
    Ok(pool)
}

/// Private row type for the SQLite `documents` table. Never escapes
/// this module — the trait surface returns [`Document`].
#[derive(Debug, Clone, sqlx::FromRow)]
struct DocumentRow {
    id: String,
    title: String,
    source: Option<String>,
    char_count: i64,
    chunk_count: i64,
    created_at: String,
}

impl TryFrom<DocumentRow> for Document {
    type Error = BoxError;
    fn try_from(r: DocumentRow) -> Result<Self, Self::Error> {
        Ok(Document {
            id: Uuid::parse_str(&r.id).map_err(|e| Box::new(e) as BoxError)?,
            title: r.title,
            source: r.source,
            char_count: r.char_count.max(0) as usize,
            chunk_count: r.chunk_count.max(0) as usize,
            created_at: r.created_at,
        })
    }
}

/// Private row type for the SQLite `document_chunks` table.
#[derive(Debug, Clone, sqlx::FromRow)]
struct DocumentChunkRow {
    document_id: String,
    chunk_index: i64,
    content: String,
    char_start: i64,
    char_end: i64,
}

impl TryFrom<DocumentChunkRow> for DocumentChunk {
    type Error = BoxError;
    fn try_from(r: DocumentChunkRow) -> Result<Self, Self::Error> {
        Ok(DocumentChunk {
            document_id: Uuid::parse_str(&r.document_id)
                .map_err(|e| Box::new(e) as BoxError)?,
            index: r.chunk_index.max(0) as usize,
            content: r.content,
            char_start: r.char_start.max(0) as usize,
            char_end: r.char_end.max(0) as usize,
        })
    }
}

#[derive(Clone)]
pub struct SqliteDocumentRepo {
    pool: Pool<Sqlite>,
}

impl SqliteDocumentRepo {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DocumentRepo for SqliteDocumentRepo {
    async fn create_document(
        &self,
        id: Uuid,
        title: &str,
        source: Option<&str>,
        char_count: usize,
        chunk_count: usize,
        created_at: &str,
    ) -> Result<(), BoxError> {
        sqlx::query::<Sqlite>(
            "INSERT INTO documents (id, title, source, char_count, chunk_count, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(title)
        .bind(source)
        .bind(char_count as i64)
        .bind(chunk_count as i64)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn create_chunk(&self, chunk: &DocumentChunk) -> Result<(), BoxError> {
        sqlx::query::<Sqlite>(
            "INSERT INTO document_chunks (document_id, chunk_index, content, char_start, char_end) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(chunk.document_id.to_string())
        .bind(chunk.index as i64)
        .bind(&chunk.content)
        .bind(chunk.char_start as i64)
        .bind(chunk.char_end as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_documents(&self) -> Result<Vec<Document>, BoxError> {
        let rows: Vec<DocumentRow> = sqlx::query_as(
            "SELECT id, title, source, char_count, chunk_count, created_at FROM documents ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(Document::try_from).collect()
    }

    async fn get_document(&self, id: Uuid) -> Result<Option<Document>, BoxError> {
        let row: Option<DocumentRow> = sqlx::query_as(
            "SELECT id, title, source, char_count, chunk_count, created_at FROM documents WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(Document::try_from).transpose()
    }

    async fn get_chunk(
        &self,
        doc_id: Uuid,
        chunk_index: usize,
    ) -> Result<Option<DocumentChunk>, BoxError> {
        let row: Option<DocumentChunkRow> = sqlx::query_as(
            "SELECT document_id, chunk_index, content, char_start, char_end FROM document_chunks WHERE document_id = ? AND chunk_index = ?",
        )
        .bind(doc_id.to_string())
        .bind(chunk_index as i64)
        .fetch_optional(&self.pool)
        .await?;
        row.map(DocumentChunk::try_from).transpose()
    }

    async fn get_chunks_window(
        &self,
        doc_id: Uuid,
        start: usize,
        end: usize,
    ) -> Result<Vec<DocumentChunk>, BoxError> {
        let rows: Vec<DocumentChunkRow> = sqlx::query_as(
            "SELECT document_id, chunk_index, content, char_start, char_end FROM document_chunks WHERE document_id = ? AND chunk_index >= ? AND chunk_index <= ? ORDER BY chunk_index",
        )
        .bind(doc_id.to_string())
        .bind(start as i64)
        .bind(end as i64)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(DocumentChunk::try_from).collect()
    }

    async fn delete_document(&self, id: Uuid) -> Result<(), BoxError> {
        let result = sqlx::query("DELETE FROM documents WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            // Translate to a BoxError; the service layer maps this to
            // a CorpusError::DocumentNotFound.
            return Err(Box::new(crate::error::CorpusError::DocumentNotFound(id)));
        }
        Ok(())
    }

    async fn search_keyword(
        &self,
        doc_id: Uuid,
        keyword: &str,
    ) -> Result<Vec<ChunkHit>, BoxError> {
        let lower = keyword.to_lowercase();
        let rows: Vec<DocumentChunkRow> = sqlx::query_as(
            "SELECT document_id, chunk_index, content, char_start, char_end FROM document_chunks WHERE document_id = ?",
        )
        .bind(doc_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut hits: Vec<(usize, String)> = Vec::new();
        for row in rows {
            let content_lower = row.content.to_lowercase();
            if !content_lower.contains(&lower) {
                continue;
            }
            let count = content_lower.matches(&lower).count();
            let snippet = extract_snippet(&row.content, keyword, 120);
            hits.push((row.chunk_index as usize, format!("[×{count}] {snippet}")));
        }
        // Sort by occurrence count descending.
        hits.sort_by(|a, b| {
            let ca: usize = a
                .1
                .split_once("[×")
                .and_then(|(_, r)| r.split_once(']').map(|(n, _)| n.parse::<usize>().unwrap_or(0)))
                .unwrap_or(0);
            let cb: usize = b
                .1
                .split_once("[×")
                .and_then(|(_, r)| r.split_once(']').map(|(n, _)| n.parse::<usize>().unwrap_or(0)))
                .unwrap_or(0);
            cb.cmp(&ca)
        });

        Ok(hits
            .into_iter()
            .map(|(idx, snippet)| {
                let chunk_start = idx.saturating_sub(1) * DEFAULT_CHUNK_SIZE;
                ChunkHit {
                    document_id: doc_id,
                    index: idx,
                    snippet,
                    char_start: chunk_start,
                    char_end: chunk_start + DEFAULT_CHUNK_SIZE,
                }
            })
            .collect())
    }
}

/// Extract a short snippet around the first occurrence of `keyword` in
/// `text`, centred on the match and padded to at most `radius`
/// characters on each side.
fn extract_snippet(text: &str, keyword: &str, radius: usize) -> String {
    let lower = text.to_lowercase();
    let kw_lower = keyword.to_lowercase();
    let start = match lower.find(&kw_lower) {
        Some(i) => i,
        None => return text.chars().take(radius * 2).collect(),
    };
    let before = text.chars().take(start).count();
    let skip = if before > radius { before - radius } else { 0 };
    let snippet_start = text
        .char_indices()
        .nth(skip)
        .map(|(b, _)| b)
        .unwrap_or(0);
    let snippet_end = text
        .char_indices()
        .nth(skip.min(text.chars().count()) + radius * 2)
        .map(|(b, _)| b)
        .unwrap_or(text.len());
    let snippet = &text[snippet_start..snippet_end.min(text.len())];
    let mut s = String::new();
    if skip > 0 {
        s.push('…');
    }
    s.push_str(snippet.trim());
    if snippet_end < text.len() {
        s.push('…');
    }
    s
}

/// Apply the corpus SQLite migrations to an externally-owned pool.
/// Used by tests that share an in-memory DB with KMS.
///
/// This is the only sqlx migration entry point in the public API and
/// is intentionally not part of [`crate::CorpusService`] — calling
/// code should use [`crate::CorpusService::open`] for production.
pub async fn run_migrations_on_pool(
    pool: &SqlitePool,
) -> Result<(), BoxError> {
    // Best-effort cleanup of any corpus-owned version row in
    // `_sqlx_migrations`. This is a no-op on fresh databases and
    // idempotent on already-cleaned ones.
    let _ = sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 20260608120000")
        .execute(pool)
        .await;

    let mut migrator = sqlx::migrate::Migrator::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations/sqlite"),
    )
    .await?;
    migrator.set_locking(false);
    migrator.dangerous_set_table_name("_corpus_sqlx_migrations");
    migrator.run(pool).await?;
    Ok(())
}
