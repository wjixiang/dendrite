pub mod database;
pub mod error;
pub mod repo;
pub mod types;

use sqlx::SqlitePool;

use crate::storage::database::sqlite::{SqliteEntityRepo, SqliteIndexRepo, SqliteKnowledgeRepo};
use crate::storage::repo::{EntityRepo, IndexRepo, KnowledgeRepo};

#[derive(Clone)]
pub struct Storage {
    pool: SqlitePool,
    pub entity: SqliteEntityRepo,
    pub knowledge: SqliteKnowledgeRepo,
    pub index: SqliteIndexRepo,
}

impl Storage {
    pub async fn new(db_path: &str) -> Result<Self, String> {
        let pool = create_pool(db_path).await?;
        Ok(Self::from_pool(pool))
    }

    pub fn from_pool(pool: SqlitePool) -> Self {
        Self {
            pool: pool.clone(),
            entity: SqliteEntityRepo::new(pool.clone()),
            knowledge: SqliteKnowledgeRepo::new(pool.clone()),
            index: SqliteIndexRepo::new(pool),
        }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

async fn create_pool(db_path: &str) -> Result<SqlitePool, String> {
    if let Some(parent) = std::path::Path::new(db_path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).ok();
        }
    }
    let url = if db_path.starts_with("sqlite://") {
        db_path.to_string()
    } else {
        format!("sqlite://{}?mode=rwc", db_path)
    };
    let pool = sqlx::SqlitePool::connect(&url)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::migrate!("migrations/sqlite")
        .run(&pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(pool)
}

async fn ensure_root_index(index_repo: &SqliteIndexRepo) -> Result<uuid::Uuid, String> {
    use crate::storage::repo::IndexRepo;
    use crate::storage::types::{Index, TargetType};

    let existing = sqlx::query_as::<sqlx::Sqlite, (String,)>(
        "SELECT id FROM indexes WHERE parent_id IS NULL LIMIT 1",
    )
    .fetch_optional(index_repo.pool())
    .await
    .map_err(|e| e.to_string())?;

    if let Some((id_str,)) = existing {
        return uuid::Uuid::parse_str(&id_str).map_err(|e| e.to_string());
    }

    let root_id = uuid::Uuid::new_v4();
    let entry = Index {
        id: root_id,
        title: Some("Root".to_string()),
        target: None,
        target_type: TargetType::Group,
        parent_id: None,
        position: 0,
    };

    index_repo.create(&entry).await.map_err(|e| e.to_string())?;
    Ok(root_id)
}
