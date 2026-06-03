use sqlx::{Pool, Sqlite};
use uuid::Uuid;

use crate::language::Language;
use crate::storage::{
    error::StorageError,
    repo::{EntityRepo, IndexRepo, KnowledgeRepo},
    types::{Entity, Index, Knowledge, KnowledgeType, Nomenclature, TargetType},
};

pub async fn init_sqlite() -> Result<Pool<Sqlite>, StorageError> {
    let pool = sqlx::sqlite::SqlitePool::connect("sqlite://data/deepmem.db?mode=rwc").await?;
    Ok(pool)
}

fn language_to_str(lang: &Language) -> &'static str {
    match lang {
        Language::EN => "EN",
        Language::ZH => "ZH",
    }
}

#[derive(sqlx::FromRow)]
struct EntityRow {
    id: String,
    definition: String,
}

#[derive(sqlx::FromRow)]
struct NomenclatureRow {
    id: String,
    #[allow(dead_code)]
    entity_id: String,
    lang: String,
    full: String,
    abbr: Option<String>,
}

#[derive(sqlx::FromRow)]
struct KnowledgeRow {
    id: String,
    title: String,
    knowledge_type: String,
    entities: String,
    content: Option<String>,
}

#[derive(Clone)]
pub struct SqliteEntityRepo {
    pool: Pool<Sqlite>,
}

impl SqliteEntityRepo {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }
}

impl EntityRepo for SqliteEntityRepo {
    async fn create(&self, entity: &Entity) -> Result<Uuid, StorageError> {
        sqlx::query::<Sqlite>("INSERT INTO entities (id, definition) VALUES (?, ?)")
            .bind(entity.id.to_string())
            .bind(&entity.definition)
            .execute(&self.pool)
            .await?;

        for nom in &entity.name {
            sqlx::query::<Sqlite>(
                "INSERT INTO nomenclatures (id, entity_id, lang, full, abbr) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(nom.id.to_string())
            .bind(entity.id.to_string())
            .bind(language_to_str(&nom.lang))
            .bind(&nom.full)
            .bind(&nom.abbr)
            .execute(&self.pool)
            .await?;
        }

        Ok(entity.id)
    }

    async fn get(&self, id: Uuid) -> Result<Entity, StorageError> {
        let id_str = id.to_string();

        let row =
            sqlx::query_as::<Sqlite, EntityRow>("SELECT id, definition FROM entities WHERE id = ?")
                .bind(&id_str)
                .fetch_optional(&self.pool)
                .await?
                .ok_or(StorageError::NotFound(id))?;

        let nom_rows = sqlx::query_as::<Sqlite, NomenclatureRow>(
            "SELECT id, entity_id, lang, full, abbr FROM nomenclatures WHERE entity_id = ?",
        )
        .bind(&id_str)
        .fetch_all(&self.pool)
        .await?;

        let name = nom_rows
            .into_iter()
            .map(|r| Nomenclature {
                id: Uuid::parse_str(&r.id).unwrap(),
                lang: match r.lang.as_str() {
                    "EN" => Language::EN,
                    "ZH" => Language::ZH,
                    _ => unreachable!("unknown language"),
                },
                full: r.full,
                abbr: r.abbr,
            })
            .collect();

        Ok(Entity {
            id,
            name,
            definition: row.definition,
        })
    }

    async fn search_by_name(&self, keyword: &str) -> Result<Vec<Entity>, StorageError> {
        let pattern = format!("{}%", keyword);
        let rows = sqlx::query_as::<Sqlite, EntityRow>(
            "SELECT DISTINCT e.id, e.definition FROM entities e JOIN nomenclatures n ON n.entity_id = e.id WHERE n.full LIKE ?",
        )
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await?;

        let mut entities = Vec::new();
        for row in rows {
            let id = Uuid::parse_str(&row.id).unwrap();
            let nom_rows = sqlx::query_as::<Sqlite, NomenclatureRow>(
                "SELECT id, entity_id, lang, full, abbr FROM nomenclatures WHERE entity_id = ?",
            )
            .bind(&row.id)
            .fetch_all(&self.pool)
            .await?;

            let name = nom_rows
                .into_iter()
                .map(|r| Nomenclature {
                    id: Uuid::parse_str(&r.id).unwrap(),
                    lang: match r.lang.as_str() {
                        "EN" => Language::EN,
                        "ZH" => Language::ZH,
                        _ => unreachable!("unknown language"),
                    },
                    full: r.full,
                    abbr: r.abbr,
                })
                .collect();

            entities.push(Entity {
                id,
                name,
                definition: row.definition,
            });
        }
        Ok(entities)
    }

    async fn find_by_exact_name(&self, name: &str) -> Result<Option<Entity>, StorageError> {
        let row = sqlx::query_as::<Sqlite, EntityRow>(
            "SELECT DISTINCT e.id, e.definition FROM entities e JOIN nomenclatures n ON n.entity_id = e.id WHERE n.full = ? LIMIT 1",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;

        let entity = match row {
            Some(r) => {
                let id = Uuid::parse_str(&r.id).unwrap();
                let nom_rows = sqlx::query_as::<Sqlite, NomenclatureRow>(
                    "SELECT id, entity_id, lang, full, abbr FROM nomenclatures WHERE entity_id = ?",
                )
                .bind(&r.id)
                .fetch_all(&self.pool)
                .await?;

                let name_list = nom_rows
                    .into_iter()
                    .map(|nr| Nomenclature {
                        id: Uuid::parse_str(&nr.id).unwrap(),
                        lang: match nr.lang.as_str() {
                            "EN" => Language::EN,
                            "ZH" => Language::ZH,
                            _ => unreachable!("unknown language"),
                        },
                        full: nr.full,
                        abbr: nr.abbr,
                    })
                    .collect();

                Some(Entity {
                    id,
                    name: name_list,
                    definition: r.definition,
                })
            }
            None => None,
        };
        Ok(entity)
    }

    async fn update(&self, entity: &Entity) -> Result<(), StorageError> {
        let result = sqlx::query::<Sqlite>("UPDATE entities SET definition = ? WHERE id = ?")
            .bind(&entity.definition)
            .bind(entity.id.to_string())
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(entity.id));
        }

        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<(), StorageError> {
        let result = sqlx::query("DELETE FROM entities WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(id));
        }

        Ok(())
    }
}

#[derive(Clone)]
pub struct SqliteKnowledgeRepo {
    pool: Pool<Sqlite>,
}

impl SqliteKnowledgeRepo {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }
}

impl KnowledgeRepo for SqliteKnowledgeRepo {
    async fn create(&self, knowledge: &Knowledge) -> Result<Uuid, StorageError> {
        let entities_str = knowledge
            .entities
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(",");

        sqlx::query::<Sqlite>(
            "INSERT INTO knowledges (id, title, knowledge_type, entities, content) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(knowledge.id.to_string())
        .bind(&knowledge.title)
        .bind(knowledge.knowledge_type.as_str())
        .bind(&entities_str)
        .bind(&knowledge.content)
        .execute(&self.pool)
        .await?;

        Ok(knowledge.id)
    }

    async fn get(&self, id: Uuid) -> Result<Knowledge, StorageError> {
        let id_str = id.to_string();

        let row = sqlx::query_as::<Sqlite, KnowledgeRow>(
            "SELECT id, title, knowledge_type, entities, content FROM knowledges WHERE id = ?",
        )
        .bind(&id_str)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StorageError::NotFound(id))?;

        let entities: Vec<Uuid> = row
            .entities
            .split(',')
            .filter(|s| !s.is_empty())
            .filter_map(|s| Uuid::parse_str(s).ok())
            .collect();

        Ok(Knowledge {
            id,
            title: row.title,
            knowledge_type: KnowledgeType::convert_from_str(&row.knowledge_type),
            entities,
            content: row.content,
        })
    }

    async fn find_by_title(&self, title: &str) -> Result<Option<Knowledge>, StorageError> {
        let row = sqlx::query_as::<Sqlite, KnowledgeRow>(
            "SELECT id, title, knowledge_type, entities, content FROM knowledges WHERE title = ? LIMIT 1",
        )
        .bind(title)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            let entities = r
                .entities
                .split(',')
                .filter_map(|s| s.parse::<Uuid>().ok())
                .collect();
            Knowledge {
                id: Uuid::parse_str(&r.id).unwrap(),
                title: r.title,
                knowledge_type: KnowledgeType::convert_from_str(&r.knowledge_type),
                entities,
                content: r.content,
            }
        }))
    }

    async fn update(&self, knowledge: &Knowledge) -> Result<(), StorageError> {
        let entities_str = knowledge
            .entities
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let result = sqlx::query::<Sqlite>(
            "UPDATE knowledges SET title = ?, knowledge_type = ?, entities = ?, content = ? WHERE id = ?",
        )
        .bind(&knowledge.title)
        .bind(knowledge.knowledge_type.as_str())
        .bind(&entities_str)
        .bind(&knowledge.content)
        .bind(knowledge.id.to_string())
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(knowledge.id));
        }

        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<(), StorageError> {
        let id_str = id.to_string();

        let result = sqlx::query::<Sqlite>("DELETE FROM knowledges WHERE id = ?")
            .bind(&id_str)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(id));
        }

        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct IndexRow {
    id: String,
    title: Option<String>,
    target: Option<String>,
    target_type: Option<String>,
    parent_id: Option<String>,
    position: i64,
}

#[derive(Clone)]
pub struct SqliteIndexRepo {
    pool: Pool<Sqlite>,
}

fn row_to_index(row: IndexRow) -> Index {
    Index {
        id: Uuid::parse_str(&row.id).unwrap(),
        title: row.title,
        target: row.target.map(|t| Uuid::parse_str(&t).unwrap()),
        target_type: match row.target_type.as_deref() {
            Some("knowledge") => TargetType::Knowledge,
            _ => TargetType::Group,
        },
        parent_id: row.parent_id.map(|p| Uuid::parse_str(&p).unwrap()),
        position: row.position,
    }
}

impl SqliteIndexRepo {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }
}

impl IndexRepo for SqliteIndexRepo {
    async fn create(&self, entry: &Index) -> Result<Uuid, StorageError> {
        sqlx::query::<Sqlite>(
            "INSERT INTO indexes (id, title, target, target_type, parent_id, position) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(entry.id.to_string())
        .bind(&entry.title)
        .bind(entry.target.map(|t| t.to_string()))
        .bind(match entry.target_type {
            TargetType::Group => "group",
            TargetType::Knowledge => "knowledge",
        })
        .bind(entry.parent_id.map(|p| p.to_string()))
        .bind(entry.position)
        .execute(&self.pool)
        .await?;
        Ok(entry.id)
    }

    async fn get(&self, id: Uuid) -> Result<Index, StorageError> {
        let id_str = id.to_string();

        let row = sqlx::query_as::<Sqlite, IndexRow>(
            "SELECT id, title, target, target_type, parent_id, position FROM indexes WHERE id = ?",
        )
        .bind(&id_str)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StorageError::NotFound(id))?;

        Ok(row_to_index(row))
    }

    async fn find_by_title(&self, title: &str) -> Result<Option<Index>, StorageError> {
        let row = sqlx::query_as::<Sqlite, IndexRow>(
            "SELECT id, title, target, target_type, parent_id, position FROM indexes WHERE title = ? LIMIT 1",
        )
        .bind(title)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(row_to_index))
    }

    async fn find_root(&self) -> Result<Index, StorageError> {
        let row = sqlx::query_as::<Sqlite, IndexRow>(
            "SELECT id, title, target, target_type, parent_id, position FROM indexes WHERE parent_id IS NULL LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(Uuid::nil()))?;

        Ok(row_to_index(row))
    }

    async fn update(&self, entry: &Index) -> Result<(), StorageError> {
        let result = sqlx::query::<Sqlite>(
            "UPDATE indexes SET title = ?, target = ?, target_type = ?, parent_id = ?, position = ? WHERE id = ?",
        )
        .bind(&entry.title)
        .bind(entry.target.map(|t| t.to_string()))
        .bind(match entry.target_type {
            TargetType::Group => "group",
            TargetType::Knowledge => "knowledge",
        })
        .bind(entry.parent_id.map(|p| p.to_string()))
        .bind(entry.position)
        .bind(entry.id.to_string())
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(entry.id));
        }

        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<(), StorageError> {
        let id_str = id.to_string();

        let result = sqlx::query::<Sqlite>("DELETE FROM indexes WHERE id = ?")
            .bind(&id_str)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(id));
        }

        Ok(())
    }

    async fn children_of(
        &self,
        parent_id: Option<Uuid>,
    ) -> Result<Vec<Index>, StorageError> {
        let rows = match parent_id {
            Some(pid) => {
                sqlx::query_as::<Sqlite, IndexRow>(
                    "SELECT id, title, target, target_type, parent_id, position FROM indexes WHERE parent_id = ? ORDER BY position",
                )
                .bind(pid.to_string())
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as::<Sqlite, IndexRow>(
                    "SELECT id, title, target, target_type, parent_id, position FROM indexes WHERE parent_id IS NULL ORDER BY position",
                )
                .fetch_all(&self.pool)
                .await?
            }
        };

        rows.into_iter().map(|row| Ok(row_to_index(row))).collect()
    }

    async fn subtree_knowledge_ids(&self, index_id: Uuid) -> Result<Vec<Uuid>, StorageError> {
        let rows = sqlx::query_as::<Sqlite, (String,)>(
            "WITH RECURSIVE subtree AS (
                SELECT id FROM indexes WHERE id = ?
                UNION ALL
                SELECT i.id FROM indexes i JOIN subtree s ON i.parent_id = s.id
            )
            SELECT idx.target FROM subtree
            JOIN indexes idx ON idx.id = subtree.id
            WHERE idx.target_type = 'knowledge' AND idx.target IS NOT NULL",
        )
        .bind(index_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|(id,)| {
                Uuid::parse_str(&id)
                    .map_err(|e| StorageError::Database(sqlx::Error::Configuration(e.into())))
            })
            .collect()
    }

    async fn reparent(
        &self,
        id: Uuid,
        new_parent_id: Uuid,
        position: i64,
    ) -> Result<(), StorageError> {
        let is_descendant: (i64,) = sqlx::query_as(
            "WITH RECURSIVE descendants AS (
                SELECT id FROM indexes WHERE id = ?
                UNION ALL
                SELECT i.id FROM indexes i JOIN descendants d ON i.parent_id = d.id
            )
            SELECT COUNT(*) FROM descendants WHERE id = ?",
        )
        .bind(id.to_string())
        .bind(new_parent_id.to_string())
        .fetch_one(&self.pool)
        .await?;

        if is_descendant.0 > 0 {
            return Err(StorageError::Database(sqlx::Error::Configuration(
                "cannot reparent: new_parent is a descendant of id".into(),
            )));
        }

        let result = sqlx::query("UPDATE indexes SET parent_id = ?, position = ? WHERE id = ?")
            .bind(new_parent_id.to_string())
            .bind(position)
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(id));
        }

        Ok(())
    }

    async fn reindex_positions(
        &self,
        parent_id: Option<Uuid>,
    ) -> Result<(), StorageError> {
        let children = self.children_of(parent_id).await?;
        for (i, child) in children.into_iter().enumerate() {
            let pos = i as i64;
            sqlx::query("UPDATE indexes SET position = ? WHERE id = ?")
                .bind(pos)
                .bind(child.id.to_string())
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    async fn orphan_knowledge_titles(&self) -> Result<Vec<String>, StorageError> {
        let rows = sqlx::query_as::<Sqlite, (String,)>(
            "SELECT k.title FROM knowledges k
             WHERE k.title NOT IN (
                 SELECT i2.title FROM indexes i2
                 WHERE i2.target_type = 'knowledge' AND i2.target IS NOT NULL
             )",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(t,)| t).collect())
    }

    async fn find_by_target(&self, target_id: Uuid) -> Result<Vec<Index>, StorageError> {
        let rows = sqlx::query_as::<Sqlite, IndexRow>(
            "SELECT id, title, target, target_type, parent_id, position FROM indexes WHERE target = ?",
        )
        .bind(target_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(|r| Ok(row_to_index(r))).collect()
    }

    async fn downgrade_to_group(&self, id: Uuid) -> Result<(), StorageError> {
        let result = sqlx::query::<Sqlite>(
            "UPDATE indexes SET target_type = 'group', target = NULL WHERE id = ?",
        )
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(id));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePool;

    use super::*;

    async fn setup_entity_repo() -> SqliteEntityRepo {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("migrations/sqlite")
            .run(&pool)
            .await
            .unwrap();
        SqliteEntityRepo::new(pool)
    }

    async fn setup_knowledge_repo() -> SqliteKnowledgeRepo {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("migrations/sqlite")
            .run(&pool)
            .await
            .unwrap();
        SqliteKnowledgeRepo::new(pool)
    }

    async fn setup_index_repo() -> SqliteIndexRepo {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("migrations/sqlite")
            .run(&pool)
            .await
            .unwrap();
        SqliteIndexRepo::new(pool)
    }

    fn make_entity(id: Uuid) -> Entity {
        Entity {
            id,
            name: vec![],
            definition: "test definition".into(),
        }
    }

    fn make_index_entity(id: Uuid) -> Entity {
        Entity {
            id,
            name: vec![],
            definition: "an index".into(),
        }
    }

    // --- Entity tests ---

    #[tokio::test]
    async fn test_create_and_get_entity() {
        let repo = setup_entity_repo().await;
        let id = Uuid::new_v4();
        let entity = make_entity(id);

        let returned_id = repo.create(&entity).await.unwrap();
        assert_eq!(returned_id, id);

        let got = repo.get(id).await.unwrap();
        assert_eq!(got.id, id);
        assert_eq!(got.definition, "test definition");
        assert!(got.name.is_empty());
    }

    #[tokio::test]
    async fn test_entity_not_found() {
        let repo = setup_entity_repo().await;
        let id = Uuid::new_v4();

        let err = repo.get(id).await.unwrap_err();
        match err {
            StorageError::NotFound(uid) => assert_eq!(uid, id),
            _ => panic!("expected NotFound, got {err:?}"),
        }
    }

    #[tokio::test]
    async fn test_update_entity() {
        let repo = setup_entity_repo().await;
        let id = Uuid::new_v4();
        repo.create(&make_entity(id)).await.unwrap();

        let updated = Entity {
            definition: "updated".into(),
            ..make_entity(id)
        };
        repo.update(&updated).await.unwrap();

        let got = repo.get(id).await.unwrap();
        assert_eq!(got.definition, "updated");
    }

    #[tokio::test]
    async fn test_update_entity_not_found() {
        let repo = setup_entity_repo().await;
        let id = Uuid::new_v4();

        let err = repo.update(&make_entity(id)).await.unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_delete_entity() {
        let repo = setup_entity_repo().await;
        let id = Uuid::new_v4();
        repo.create(&make_entity(id)).await.unwrap();

        repo.delete(id).await.unwrap();
        let err = repo.get(id).await.unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_delete_entity_not_found() {
        let repo = setup_entity_repo().await;
        let id = Uuid::new_v4();

        let err = repo.delete(id).await.unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_entity_with_nomenclatures() {
        let repo = setup_entity_repo().await;
        let id = Uuid::new_v4();

        sqlx::query::<Sqlite>("INSERT INTO entities (id, definition) VALUES (?, ?)")
            .bind(id.to_string())
            .bind("test")
            .execute(&repo.pool)
            .await
            .unwrap();

        sqlx::query::<Sqlite>(
            "INSERT INTO nomenclatures (id, entity_id, lang, full, abbr) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(id.to_string())
        .bind("EN")
        .bind("English Name")
        .bind("EN")
        .execute(&repo.pool)
        .await
        .unwrap();

        sqlx::query::<Sqlite>(
            "INSERT INTO nomenclatures (id, entity_id, lang, full, abbr) VALUES (?, ?, ?, ?, NULL)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(id.to_string())
        .bind("ZH")
        .bind("中文名称")
        .execute(&repo.pool)
        .await
        .unwrap();

        let got = repo.get(id).await.unwrap();
        assert_eq!(got.name.len(), 2);
        assert!(got.name.iter().any(|n| n.lang == Language::EN
            && n.full == "English Name"
            && n.abbr == Some("EN".into())));
        assert!(
            got.name
                .iter()
                .any(|n| n.lang == Language::ZH && n.full == "中文名称" && n.abbr.is_none())
        );
    }

    #[tokio::test]
    async fn test_index_entity_type() {
        let repo = setup_entity_repo().await;
        let id = Uuid::new_v4();
        repo.create(&make_index_entity(id)).await.unwrap();

        let got = repo.get(id).await.unwrap();
        assert_eq!(got.id, id);
    }

    // --- Knowledge tests ---

    fn make_aspect_knowledge(id: Uuid, target: Uuid, content: &str) -> Knowledge {
        Knowledge {
            id,
            title: "治疗".into(),
            knowledge_type: KnowledgeType::Aspect,
            entities: vec![target],
            content: Some(content.into()),
        }
    }

    fn make_relation_knowledge(id: Uuid, entities: Vec<Uuid>) -> Knowledge {
        Knowledge {
            id,
            title: "华法林 vs 利伐沙班".into(),
            knowledge_type: KnowledgeType::Relation,
            entities,
            content: Some("对比表格...".into()),
        }
    }

    #[tokio::test]
    async fn test_create_and_get_aspect_knowledge() {
        let repo = setup_knowledge_repo().await;
        let id = Uuid::new_v4();
        let target = Uuid::new_v4();
        let knowledge = make_aspect_knowledge(id, target, "华法林 3mg daily");

        let returned_id = repo.create(&knowledge).await.unwrap();
        assert_eq!(returned_id, id);

        let got = repo.get(id).await.unwrap();
        assert_eq!(got.id, id);
        assert_eq!(got.knowledge_type, KnowledgeType::Aspect);
        assert_eq!(got.entities, vec![target]);
    }

    #[tokio::test]
    async fn test_create_and_get_comparison_knowledge() {
        let repo = setup_knowledge_repo().await;
        let id = Uuid::new_v4();
        let e1 = Uuid::new_v4();
        let e2 = Uuid::new_v4();
        let knowledge = make_relation_knowledge(id, vec![e1, e2]);

        repo.create(&knowledge).await.unwrap();

        let got = repo.get(id).await.unwrap();
        assert_eq!(got.knowledge_type, KnowledgeType::Relation);
        assert_eq!(got.entities, vec![e1, e2]);
    }

    #[tokio::test]
    async fn test_knowledge_not_found() {
        let repo = setup_knowledge_repo().await;
        let err = repo.get(Uuid::new_v4()).await.unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_update_knowledge() {
        let repo = setup_knowledge_repo().await;
        let id = Uuid::new_v4();
        let target = Uuid::new_v4();
        repo.create(&make_aspect_knowledge(id, target, "original"))
            .await
            .unwrap();

        let updated = Knowledge {
            title: "updated title".into(),
            content: Some("updated content".into()),
            ..make_aspect_knowledge(id, target, "original")
        };
        repo.update(&updated).await.unwrap();

        let got = repo.get(id).await.unwrap();
        assert_eq!(got.title, "updated title");
        assert_eq!(got.content, Some("updated content".into()));
    }

    #[tokio::test]
    async fn test_delete_knowledge() {
        let repo = setup_knowledge_repo().await;
        let id = Uuid::new_v4();
        repo.create(&make_aspect_knowledge(id, Uuid::new_v4(), "test"))
            .await
            .unwrap();

        repo.delete(id).await.unwrap();
        let err = repo.get(id).await.unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    // --- Index tests ---

    fn make_group_entry(id: Uuid, title: &str, position: i64) -> Index {
        Index {
            id,
            title: Some(title.into()),
            target: None,
            target_type: TargetType::Group,
            parent_id: None,
            position,
        }
    }

    fn make_knowledge_entry(
        id: Uuid,
        parent_id: Uuid,
        target: Uuid,
        position: i64,
    ) -> Index {
        Index {
            id,
            title: None,
            target: Some(target),
            target_type: TargetType::Knowledge,
            parent_id: Some(parent_id),
            position,
        }
    }

    #[tokio::test]
    async fn test_create_and_get_knowledge_target() {
        let repo = setup_index_repo().await;
        let id = Uuid::new_v4();
        let parent = Uuid::new_v4();
        let target = Uuid::new_v4();

        repo.create(&make_group_entry(parent, "parent", 0))
            .await
            .unwrap();
        let entry = make_knowledge_entry(id, parent, target, 0);
        repo.create(&entry).await.unwrap();

        let got = repo.get(id).await.unwrap();
        assert_eq!(got.target, Some(target));
        assert_eq!(got.target_type, TargetType::Knowledge);
    }

    #[tokio::test]
    async fn test_index_not_found() {
        let repo = setup_index_repo().await;
        let err = repo.get(Uuid::new_v4()).await.unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_update_index() {
        let repo = setup_index_repo().await;
        let id = Uuid::new_v4();

        let entry = make_group_entry(id, "循环系统", 0);
        repo.create(&entry).await.unwrap();

        let updated = Index {
            title: Some("呼吸系统".into()),
            position: 1,
            ..entry
        };
        repo.update(&updated).await.unwrap();

        let got = repo.get(id).await.unwrap();
        assert_eq!(got.title, Some("呼吸系统".into()));
        assert_eq!(got.position, 1);
    }

    #[tokio::test]
    async fn test_update_index_not_found() {
        let repo = setup_index_repo().await;
        let entry = make_group_entry(Uuid::new_v4(), "test", 0);
        let err = repo.update(&entry).await.unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_delete_index() {
        let repo = setup_index_repo().await;
        let id = Uuid::new_v4();

        repo.create(&make_group_entry(id, "test", 0))
            .await
            .unwrap();
        repo.delete(id).await.unwrap();

        let err = repo.get(id).await.unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_delete_index_not_found() {
        let repo = setup_index_repo().await;
        let err = repo.delete(Uuid::new_v4()).await.unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_delete_index_cascades_children() {
        let repo = setup_index_repo().await;
        let parent_id = Uuid::new_v4();
        let child_id = Uuid::new_v4();

        repo.create(&make_group_entry(parent_id, "循环系统", 0))
            .await
            .unwrap();
        repo.create(&make_knowledge_entry(
            child_id,
            parent_id,
            Uuid::new_v4(),
            0,
        ))
        .await
        .unwrap();

        repo.delete(parent_id).await.unwrap();

        // child should be cascade deleted
        let err = repo.get(child_id).await.unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_children_of_root_level() {
        let repo = setup_index_repo().await;
        let g1 = Uuid::new_v4();
        let g2 = Uuid::new_v4();

        repo.create(&make_group_entry(g1, "循环系统", 0))
            .await
            .unwrap();
        repo.create(&make_group_entry(g2, "呼吸系统", 1))
            .await
            .unwrap();

        let children = repo.children_of(None).await.unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].title, Some("循环系统".into()));
        assert_eq!(children[1].title, Some("呼吸系统".into()));
    }

    #[tokio::test]
    async fn test_children_of_parent() {
        let repo = setup_index_repo().await;
        let parent = Uuid::new_v4();
        let t1 = Uuid::new_v4();
        let t2 = Uuid::new_v4();
        let c1 = Uuid::new_v4();
        let c2 = Uuid::new_v4();

        repo.create(&make_group_entry(parent, "parent", 0))
            .await
            .unwrap();
        repo.create(&make_knowledge_entry(c1, parent, t1, 0))
            .await
            .unwrap();
        repo.create(&make_knowledge_entry(c2, parent, t2, 1))
            .await
            .unwrap();

        let children = repo.children_of(Some(parent)).await.unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].target, Some(t1));
        assert_eq!(children[1].target, Some(t2));
    }

    #[tokio::test]
    async fn test_children_ordering_preserved() {
        let repo = setup_index_repo().await;
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();

        // insert out of order: position 2, 0, 1
        repo.create(&make_group_entry(a, "B", 2))
            .await
            .unwrap();
        repo.create(&make_group_entry(b, "A", 0))
            .await
            .unwrap();
        repo.create(&make_group_entry(c, "C", 1))
            .await
            .unwrap();

        let children = repo.children_of(None).await.unwrap();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].title, Some("A".into()));
        assert_eq!(children[1].title, Some("C".into()));
        assert_eq!(children[2].title, Some("B".into()));
    }

    #[tokio::test]
    async fn test_build_tree_hierarchy() {
        let repo = setup_index_repo().await;

        // root level groups (these ARE index entries, so they can be parents)
        let respiratory = Uuid::new_v4();
        let circulatory = Uuid::new_v4();
        repo.create(&make_group_entry(respiratory, "呼吸系统", 0))
            .await
            .unwrap();
        repo.create(&make_group_entry(circulatory, "循环系统", 1))
            .await
            .unwrap();

        // children of circulatory (parent = circulatory index entry id)
        let hypertension = Uuid::new_v4();
        let ar_group = Uuid::new_v4();
        let hp_entry = Uuid::new_v4();
        repo.create(&make_knowledge_entry(
            hp_entry,
            circulatory,
            hypertension,
            0,
        ))
        .await
        .unwrap();
        repo.create(&Index {
            id: ar_group,
            title: Some("心律失常".into()),
            target: None,
            target_type: TargetType::Group,
            parent_id: Some(circulatory),
            position: 1,
        })
        .await
        .unwrap();

        // children of arrhythmia group (parent = ar_group index entry id)
        let af = Uuid::new_v4();
        let af_entry = Uuid::new_v4();
        repo.create(&make_knowledge_entry(af_entry, ar_group, af, 0))
            .await
            .unwrap();

        // verify tree: root -> [呼吸系统, 循环系统]
        let root_children = repo.children_of(None).await.unwrap();
        assert_eq!(root_children.len(), 2);

        // verify: 循环系统 -> [高血压, 心律失常(group)]
        let circ_children = repo.children_of(Some(circulatory)).await.unwrap();
        assert_eq!(circ_children.len(), 2);
        assert_eq!(circ_children[0].target, Some(hypertension));
        assert_eq!(circ_children[1].title, Some("心律失常".into()));

        // verify: 心律失常(group) -> [房颤]
        let ar_children = repo.children_of(Some(ar_group)).await.unwrap();
        assert_eq!(ar_children.len(), 1);
        assert_eq!(ar_children[0].target, Some(af));
    }

    #[tokio::test]
    async fn test_reparent_basic() {
        let repo = setup_index_repo().await;
        let parent_a = Uuid::new_v4();
        let parent_b = Uuid::new_v4();
        let child = Uuid::new_v4();

        repo.create(&make_group_entry(parent_a, "parent_a", 0))
            .await
            .unwrap();
        repo.create(&make_group_entry(parent_b, "parent_b", 1))
            .await
            .unwrap();
        repo.create(&make_group_entry(child, "child", 0))
            .await
            .unwrap();

        // insert child under parent_a first
        let entry = repo.get(child).await.unwrap();
        repo.update(&Index {
            parent_id: Some(parent_a),
            ..entry
        })
        .await
        .unwrap();

        // reparent child to parent_b
        repo.reparent(child, parent_b, 0).await.unwrap();

        let b_children = repo.children_of(Some(parent_b)).await.unwrap();
        assert_eq!(b_children.len(), 1);
        assert_eq!(b_children[0].id, child);
        assert_eq!(b_children[0].position, 0);

        let a_children = repo.children_of(Some(parent_a)).await.unwrap();
        assert!(a_children.is_empty());
    }

    #[tokio::test]
    async fn test_reparent_cycle_detection() {
        let repo = setup_index_repo().await;
        let parent = Uuid::new_v4();
        let child = Uuid::new_v4();

        repo.create(&make_group_entry(parent, "parent", 0))
            .await
            .unwrap();
        repo.create(&make_group_entry(child, "child", 0))
            .await
            .unwrap();

        let entry = repo.get(child).await.unwrap();
        repo.update(&Index {
            parent_id: Some(parent),
            ..entry
        })
        .await
        .unwrap();

        // reparent parent to child would create a cycle
        let err = repo.reparent(parent, child, 0).await.unwrap_err();
        assert!(matches!(err, StorageError::Database(_)));
    }

    #[tokio::test]
    async fn test_reindex_positions() {
        let repo = setup_index_repo().await;
        let parent = Uuid::new_v4();

        repo.create(&make_group_entry(parent, "parent", 0))
            .await
            .unwrap();

        let c1 = Uuid::new_v4();
        let c2 = Uuid::new_v4();
        let c3 = Uuid::new_v4();

        repo.create(&Index {
            id: c1,
            title: Some("A".into()),
            target: None,
            target_type: TargetType::Group,
            parent_id: Some(parent),
            position: 5,
        })
        .await
        .unwrap();
        repo.create(&Index {
            id: c2,
            title: Some("B".into()),
            target: None,
            target_type: TargetType::Group,
            parent_id: Some(parent),
            position: 10,
        })
        .await
        .unwrap();
        repo.create(&Index {
            id: c3,
            title: Some("C".into()),
            target: None,
            target_type: TargetType::Group,
            parent_id: Some(parent),
            position: 15,
        })
        .await
        .unwrap();

        // reindex should make positions 0, 1, 2
        repo.reindex_positions(Some(parent)).await.unwrap();

        let children = repo.children_of(Some(parent)).await.unwrap();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].position, 0);
        assert_eq!(children[1].position, 1);
        assert_eq!(children[2].position, 2);
    }

    #[tokio::test]
    async fn test_orphan_knowledge_titles() {
        let repo = setup_index_repo().await;

        let indexed_kid = Uuid::new_v4();
        sqlx::query("INSERT INTO knowledges (id, title, knowledge_type, entities) VALUES (?, ?, 'aspect', '')")
            .bind(indexed_kid.to_string())
            .bind("indexed_knowledge")
            .execute(&repo.pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO knowledges (id, title, knowledge_type, entities) VALUES (?, ?, 'aspect', '')")
            .bind(Uuid::new_v4().to_string())
            .bind("orphan_knowledge")
            .execute(&repo.pool)
            .await
            .unwrap();

        let idx_id = Uuid::new_v4();
        sqlx::query("INSERT INTO indexes (id, title, target_type, parent_id, position, target) VALUES (?, ?, 'knowledge', NULL, 0, ?)")
            .bind(idx_id.to_string())
            .bind("indexed_knowledge")
            .bind(indexed_kid.to_string())
            .execute(&repo.pool)
            .await
            .unwrap();

        let orphans = repo.orphan_knowledge_titles().await.unwrap();
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0], "orphan_knowledge");
    }

    #[tokio::test]
    async fn test_find_by_target() {
        let repo = setup_index_repo().await;
        let target_id = Uuid::new_v4();
        let other_id = Uuid::new_v4();

        sqlx::query("INSERT INTO indexes (id, title, target_type, parent_id, position, target) VALUES (?, ?, 'knowledge', NULL, 0, ?)")
            .bind(Uuid::new_v4().to_string()).bind("idx_a").bind(target_id.to_string())
            .execute(&repo.pool).await.unwrap();

        sqlx::query("INSERT INTO indexes (id, title, target_type, parent_id, position, target) VALUES (?, ?, 'knowledge', NULL, 1, ?)")
            .bind(Uuid::new_v4().to_string()).bind("idx_b").bind(target_id.to_string())
            .execute(&repo.pool).await.unwrap();

        sqlx::query("INSERT INTO indexes (id, title, target_type, parent_id, position, target) VALUES (?, ?, 'knowledge', NULL, 2, ?)")
            .bind(Uuid::new_v4().to_string()).bind("idx_c").bind(other_id.to_string())
            .execute(&repo.pool).await.unwrap();

        let found = repo.find_by_target(target_id).await.unwrap();
        assert_eq!(found.len(), 2);
        let titles: Vec<&str> = found.iter().filter_map(|i| i.title.as_deref()).collect();
        assert!(titles.contains(&"idx_a"));
        assert!(titles.contains(&"idx_b"));

        let found_other = repo.find_by_target(other_id).await.unwrap();
        assert_eq!(found_other.len(), 1);
        assert_eq!(found_other[0].title.as_deref(), Some("idx_c"));
    }

    #[tokio::test]
    async fn test_downgrade_to_group() {
        let repo = setup_index_repo().await;
        let idx_id = Uuid::new_v4();
        let kid = Uuid::new_v4();

        sqlx::query("INSERT INTO indexes (id, title, target_type, parent_id, position, target) VALUES (?, ?, 'knowledge', NULL, 0, ?)")
            .bind(idx_id.to_string()).bind("knowledge_idx").bind(kid.to_string())
            .execute(&repo.pool).await.unwrap();

        let before = repo.get(idx_id).await.unwrap();
        assert_eq!(before.target_type, TargetType::Knowledge);
        assert!(before.target.is_some());

        repo.downgrade_to_group(idx_id).await.unwrap();

        let after = repo.get(idx_id).await.unwrap();
        assert_eq!(after.target_type, TargetType::Group);
        assert!(after.target.is_none());
    }
}
