//! Storage layer for the corpus crate.
//!
//! [`CorpusStorage`] owns a single [`DocumentRepo`] instance
//! (a `Box<dyn DocumentRepo>` so the data backend is pluggable).
//! The actual backend choice happens in [`crate::service::CorpusService::open`]
//! via the [`crate::Backend`] enum.
//!
//! The SQLite implementation lives in [`sqlite`] (the only module
//! in the crate that references sqlx directly). It is fully private
//! — its types do not appear in the public API.

use std::sync::Arc;

use crate::error::CorpusError;
use crate::repo::DocumentRepo;

pub(crate) mod sqlite;

/// Owns the data backend (`Arc<dyn DocumentRepo>`) so the corpus
/// can be backed by SQLite today and any other backend tomorrow
/// without changing the public API.
#[derive(Clone)]
pub(crate) struct CorpusStorage {
    documents: Arc<dyn DocumentRepo>,
}

impl CorpusStorage {
    /// Wrap a ready-to-use `DocumentRepo` (used by [`crate::Backend::Custom`]
    /// and by tests that construct a backend directly).
    pub(crate) fn from_repo(documents: Arc<dyn DocumentRepo>) -> Self {
        Self { documents }
    }

    /// Build the SQLite-backed storage from a DB path. Used by
    /// [`crate::Backend::Sqlite`].
    pub(crate) async fn open_sqlite(path: &str) -> Result<Self, CorpusError> {
        let pool = sqlite::create_pool(path)
            .await
            .map_err(CorpusError::Backend)?;
        sqlite::run_migrations_on_pool(&pool)
            .await
            .map_err(CorpusError::Backend)?;
        Ok(Self {
            documents: Arc::new(sqlite::SqliteDocumentRepo::new(pool)),
        })
    }

    pub(crate) fn documents(&self) -> &Arc<dyn DocumentRepo> {
        &self.documents
    }
}
