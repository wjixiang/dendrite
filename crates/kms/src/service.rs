use tokio::sync::RwLock;
use uuid::Uuid;

use crate::Storage;
use crate::language::Language;
use crate::storage::types::{Entity, Index, Knowledge, KnowledgeType, Nomenclature, TargetType};
use crate::view::{IndexView, LocalView, SUBTREE_TITLES_LIMIT};

use crate::Diagnostic;
use crate::diagnostics;
use crate::document::{self, Document, DocumentChunk, ChunkHit, DEFAULT_CHUNK_SIZE, DEFAULT_CHUNK_OVERLAP};

use crate::storage::repo::{DocumentRepo, EntityRepo, IndexRepo, KnowledgeRepo};

/// Returns the current UTC time as an ISO-8601 string
/// (`YYYY-MM-DDTHH:MM:SSZ`). Uses `std::time::SystemTime` to avoid
/// adding a `chrono` dependency.
fn iso_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = d.as_secs();

    // Algorithm: convert epoch seconds to (year, month, day, hour, min, sec).
    let mut days = (total_secs / 86400) as i64;
    let secs_of_day = (total_secs % 86400) as u32;
    let hh = secs_of_day / 3600;
    let mm = (secs_of_day % 3600) / 60;
    let ss = secs_of_day % 60;

    // Shift epoch from 1970-01-01 to 0000-03-01 (algorithm simplification).
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EntityFilter {
    EmptyDefinition,
    NoNomenclature,
    All,
}

struct Inner {
    pointer: RwLock<Uuid>,
}

#[derive(Clone)]
pub struct KmsService {
    inner: std::sync::Arc<Inner>,
    storage: Storage,
}

impl KmsService {
    pub async fn new(db_path: &str) -> Result<Self, String> {
        let storage = Storage::new(db_path).await?;

        let root_id = ensure_root_index(&storage).await?;

        let inner = std::sync::Arc::new(Inner {
            pointer: RwLock::new(root_id),
        });

        Ok(KmsService { inner, storage })
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.storage.pool()
    }

    pub async fn get_pointer(&self) -> Uuid {
        *self.inner.pointer.read().await
    }

    async fn set_pointer(&self, id: Uuid) {
        *self.inner.pointer.write().await = id;
    }

    /// Return the system root index (the lone `Index` with
    /// `parent_id = NULL`). Used by the parallel-subtree orchestrator
    /// to know where to hang staging areas.
    pub async fn find_root(&self) -> Result<Index, String> {
        self.storage
            .index
            .find_root()
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn create_entity(
        &self,
        names: Vec<Nomenclature>,
        definition: &str,
    ) -> Result<(Entity, bool), String> {
        // Deduplication: if an entity with the same name already exists, return it.
        let lookup_name = names
            .iter()
            .find(|n| n.lang == Language::ZH)
            .or_else(|| names.first())
            .map(|n| n.full.as_str())
            .unwrap_or("");

        if !lookup_name.is_empty() {
            if let Some(existing) = self
                .storage
                .entity
                .find_by_exact_name(lookup_name)
                .await
                .map_err(|e| e.to_string())?
            {
                return Ok((existing, true)); // existed = true
            }
        }

        // Deduplicate input nomenclatures: keep only the first occurrence per (lang, full).
        let mut seen: Vec<(Language, String)> = Vec::new();
        let names: Vec<Nomenclature> = names
            .into_iter()
            .filter(|n| {
                let dup = seen.iter().any(|(l, f)| *l == n.lang && f == &n.full);
                if !dup {
                    seen.push((n.lang.clone(), n.full.clone()));
                }
                !dup
            })
            .collect();

        // Check each remaining nomenclature against existing DB records.
        // If any (lang, full) already exists in another entity, skip it.
        let mut db_safe_names = Vec::new();
        for n in names {
            let exists = self
                .storage
                .entity
                .find_by_exact_name(&n.full)
                .await
                .map_err(|e| e.to_string())?;
            if exists.is_none() {
                db_safe_names.push(n);
            }
        }
        let names = db_safe_names;

        if names.is_empty() {
            return Err("所有提供的命名已存在于数据库中，无法创建新实体".into());
        }

        let entity = Entity {
            id: Uuid::new_v4(),
            name: names,
            definition: definition.to_string(),
        };

        self.storage
            .entity
            .create(&entity)
            .await
            .map_err(|e| e.to_string())?;
        Ok((entity, false)) // existed = false
    }

    pub async fn get_entity(&self, id: Uuid) -> Result<Entity, String> {
        self.storage.entity.get(id).await.map_err(|e| e.to_string())
    }

    pub async fn delete_entity(&self, id: Uuid) -> Result<(), String> {
        self.storage.entity.delete(id).await.map_err(|e| e.to_string())
    }

    pub async fn add_nomenclature(
        &self,
        entity_id: Uuid,
        lang: Language,
        full: String,
        abbr: Option<String>,
    ) -> Result<Entity, String> {
        let entity = self.storage.entity.get(entity_id).await.map_err(|e| e.to_string())?;
        if entity.name.iter().any(|n| n.lang == lang && n.full == full) {
            return Err(format!("命名 ({:?}, {}) 已存在于该实体中", lang, full));
        }
        let nom = Nomenclature {
            id: Uuid::new_v4(),
            lang,
            full: full.clone(),
            abbr,
        };
        self.storage
            .entity
            .add_nomenclature(entity_id, &nom)
            .await
            .map_err(|e| e.to_string())?;
        self.storage.entity.get(entity_id).await.map_err(|e| e.to_string())
    }

    pub async fn update_nomenclature(
        &self,
        entity_id: Uuid,
        nomenclature_id: Uuid,
        lang: Language,
        full: String,
        abbr: Option<String>,
    ) -> Result<Entity, String> {
        let entity = self.storage.entity.get(entity_id).await.map_err(|e| e.to_string())?;
        if !entity.name.iter().any(|n| n.id == nomenclature_id) {
            return Err("该 nomenclature 不属于此实体".into());
        }
        if entity
            .name
            .iter()
            .any(|n| n.id != nomenclature_id && n.lang == lang && n.full == full)
        {
            return Err(format!(
                "命名 ({:?}, {}) 已存在于该实体的另一条记录中",
                lang, full
            ));
        }
        let nom = Nomenclature {
            id: nomenclature_id,
            lang,
            full,
            abbr,
        };
        self.storage
            .entity
            .update_nomenclature(entity_id, &nom)
            .await
            .map_err(|e| e.to_string())?;
        self.storage.entity.get(entity_id).await.map_err(|e| e.to_string())
    }

    pub async fn delete_nomenclature(
        &self,
        entity_id: Uuid,
        nomenclature_id: Uuid,
    ) -> Result<Entity, String> {
        let entity = self.storage.entity.get(entity_id).await.map_err(|e| e.to_string())?;
        if entity.name.len() <= 1 {
            return Err("实体至少需要保留一条命名".into());
        }
        if !entity.name.iter().any(|n| n.id == nomenclature_id) {
            return Err("该 nomenclature 不属于此实体".into());
        }
        self.storage
            .entity
            .delete_nomenclature(nomenclature_id)
            .await
            .map_err(|e| e.to_string())?;
        self.storage.entity.get(entity_id).await.map_err(|e| e.to_string())
    }

    pub async fn search_entity(&self, keyword: &str) -> Result<Vec<Entity>, String> {
        self.storage
            .entity
            .search_by_name(keyword)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn list_entities(&self, filter: EntityFilter) -> Result<Vec<Entity>, String> {
        let all = self.storage.entity.list_all().await.map_err(|e| e.to_string())?;
        match filter {
            EntityFilter::All => Ok(all),
            EntityFilter::EmptyDefinition => {
                Ok(all.into_iter().filter(|e| e.definition.is_empty()).collect())
            }
            EntityFilter::NoNomenclature => {
                Ok(all.into_iter().filter(|e| e.name.is_empty()).collect())
            }
        }
    }

    pub async fn resolve(&self, name: &str) -> Result<Uuid, String> {
        if let Some(entity) = self
            .storage
            .entity
            .find_by_exact_name(name)
            .await
            .map_err(|e| e.to_string())?
        {
            return Ok(entity.id);
        }
        if let Some(idx) = self
            .storage
            .index
            .find_by_title(name)
            .await
            .map_err(|e| e.to_string())?
        {
            return Ok(idx.id);
        }
        if let Some(knowledge) = self
            .storage
            .knowledge
            .find_by_title(name)
            .await
            .map_err(|e| e.to_string())?
        {
            return Ok(knowledge.id);
        }
        Err(format!("cannot resolve: {}", name))
    }

    pub async fn resolve_index(&self, name: &str) -> Result<Uuid, String> {
        if let Some(idx) = self
            .storage
            .index
            .find_by_title(name)
            .await
            .map_err(|e| e.to_string())?
        {
            return Ok(idx.id);
        }
        Err(format!("index not found: {}", name))
    }

    /// Resolve a name to a Knowledge UUID directly, avoiding ambiguity with Index entries
    /// that share the same title. Searches only the knowledge table.
    pub async fn resolve_knowledge(&self, title: &str) -> Result<Uuid, String> {
        if let Some(knowledge) = self
            .storage
            .knowledge
            .find_by_title(title)
            .await
            .map_err(|e| e.to_string())?
        {
            return Ok(knowledge.id);
        }
        Err(format!("knowledge not found: {}", title))
    }

    pub async fn update_entity_by_ref(
        &self,
        name_ref: &str,
        new_definition: Option<&str>,
        new_names: Option<Vec<Nomenclature>>,
    ) -> Result<Entity, String> {
        let id = self.resolve(name_ref).await?;
        self.update_entity(id, new_definition, new_names).await
    }

    pub async fn update_entity_by_id(
        &self,
        id: Uuid,
        new_definition: Option<&str>,
        new_names: Option<Vec<Nomenclature>>,
    ) -> Result<Entity, String> {
        self.update_entity(id, new_definition, new_names).await
    }

    async fn update_entity(
        &self,
        id: Uuid,
        new_definition: Option<&str>,
        new_names: Option<Vec<Nomenclature>>,
    ) -> Result<Entity, String> {
        let mut entity = self.storage.entity.get(id).await.map_err(|e| e.to_string())?;

        if let Some(definition) = new_definition {
            entity.definition = definition.to_string();
        }
        if let Some(names) = new_names {
            entity.name = names;
        }

        self.storage.entity.update(&entity).await.map_err(|e| e.to_string())?;
        Ok(entity)
    }

    pub async fn create_knowledge(
        &self,
        title: &str,
        knowledge_type: KnowledgeType,
        entities: Vec<Uuid>,
        content: Option<String>,
    ) -> Result<Knowledge, String> {
        let knowledge = Knowledge {
            id: Uuid::new_v4(),
            title: title.to_string(),
            knowledge_type,
            entities,
            content,
            source_document_id: None,
            source_chunk_idx: None,
        };

        self.storage
            .knowledge
            .create(&knowledge)
            .await
            .map_err(|e| e.to_string())?;
        Ok(knowledge)
    }

    pub async fn get_knowledge(&self, id: Uuid) -> Result<Knowledge, String> {
        self.storage
            .knowledge
            .get(id)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn create_index_root(&self, title: &str) -> Result<Index, String> {
        let id = Uuid::new_v4();
        let entry = Index {
            id,
            title: Some(title.to_string()),
            target: None,
            target_type: TargetType::Group,
            parent_id: None,
            position: 0,
        };

        self.storage
            .index
            .create(&entry)
            .await
            .map_err(|e| e.to_string())?;
        Ok(entry)
    }

    pub async fn create_index(
        &self,
        parent_id: Uuid,
        title: Option<String>,
        target: Option<Uuid>,
        target_type: Option<TargetType>,
    ) -> Result<Index, String> {
        let parent = self
            .storage
            .index
            .get(parent_id)
            .await
            .map_err(|e| e.to_string())?;

        let siblings = self
            .storage
            .index
            .children_of(Some(parent_id))
            .await
            .map_err(|e| e.to_string())?;

        let id = Uuid::new_v4();
        let entry = Index {
            id,
            title,
            target,
            target_type: target_type.unwrap_or(TargetType::Group),
            parent_id: Some(parent_id),
            position: siblings.len() as i64,
        };

        self.storage
            .index
            .create(&entry)
            .await
            .map_err(|e| e.to_string())?;
        Ok(entry)
    }

    pub async fn create_index_by_ref(
        &self,
        parent_ref: &str,
        title: Option<String>,
        target_ref: Option<&str>,
        target_type: Option<TargetType>,
    ) -> Result<Index, String> {
        let parent_id = self.resolve_index(parent_ref).await?;
        let target = match target_type {
            Some(TargetType::Knowledge) => match target_ref {
                Some(r) => Some(self.resolve_knowledge(r).await?),
                None => None,
            },
            _ => None,
        };
        let title = title.or_else(|| target_ref.map(|s| s.to_string()));
        self.create_index(parent_id, title, target, target_type)
            .await
    }

    pub async fn link_orphans(
        &self,
        parent_ref: &str,
        knowledge_titles: &[&str],
    ) -> Result<Vec<String>, String> {
        let parent_id = self.resolve_index(parent_ref).await?;
        let mut linked = Vec::new();

        for title in knowledge_titles {
            let target_id = match self.resolve_knowledge(title).await {
                Ok(id) => id,
                Err(_) => continue,
            };
            let idx = self
                .create_index(
                    parent_id,
                    Some(title.to_string()),
                    Some(target_id),
                    Some(TargetType::Knowledge),
                )
                .await?;
            linked.push(idx.title.unwrap_or_else(|| title.to_string()));
        }

        Ok(linked)
    }

    pub async fn update_knowledge_by_ref(
        &self,
        title_ref: &str,
        new_content: Option<&str>,
        new_entities: Option<Vec<&str>>,
    ) -> Result<Knowledge, String> {
        let id = self.resolve_knowledge(title_ref).await?;
        let mut knowledge = self
            .storage
            .knowledge
            .get(id)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(content) = new_content {
            knowledge.content = Some(content.to_string());
        }
        if let Some(entity_refs) = new_entities {
            let mut entities = Vec::with_capacity(entity_refs.len());
            for r in entity_refs {
                entities.push(self.resolve(r).await?);
            }
            knowledge.entities = entities;
        }

        self.storage
            .knowledge
            .update(&knowledge)
            .await
            .map_err(|e| e.to_string())?;
        Ok(knowledge)
    }

    pub async fn rename_knowledge(
        &self,
        old_title: &str,
        new_title: &str,
    ) -> Result<Knowledge, String> {
        let id = self.resolve_knowledge(old_title).await?;
        let mut knowledge = self
            .storage
            .knowledge
            .get(id)
            .await
            .map_err(|e| e.to_string())?;

        // Check for UNIQUE constraint conflicts before renaming
        if let Some(existing) = self
            .storage
            .knowledge
            .find_by_title(new_title)
            .await
            .map_err(|e| e.to_string())?
        {
            if existing.id != id {
                return Err(format!(
                    "knowledge title '{}' already exists (id: {}); rename it first or choose a different title",
                    new_title, existing.id
                ));
            }
        }
        if let Some(_) = self
            .storage
            .index
            .find_by_title(new_title)
            .await
            .map_err(|e| e.to_string())?
        {
            return Err(format!(
                "index with title '{}' already exists; delete or rename the conflicting index first, then retry",
                new_title
            ));
        }

        let referencing_indexes = self
            .storage
            .index
            .find_by_target(id)
            .await
            .map_err(|e| e.to_string())?;

        for idx in &referencing_indexes {
            let mut updated = idx.clone();
            updated.title = Some(new_title.to_string());
            self.storage
                .index
                .update(&updated)
                .await
                .map_err(|e| e.to_string())?;
        }

        knowledge.title = new_title.to_string();
        self.storage
            .knowledge
            .update(&knowledge)
            .await
            .map_err(|e| e.to_string())?;
        Ok(knowledge)
    }

    pub async fn delete_index(&self, title: &str) -> Result<(), String> {
        let idx = self
            .storage
            .index
            .find_by_title(title)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("index '{}' not found", title))?;

        if idx.parent_id.is_none() {
            return Err("cannot delete root index".into());
        }

        // Reparent children to the deleted node's parent
        let children = self
            .storage
            .index
            .children_of(Some(idx.id))
            .await
            .map_err(|e| e.to_string())?;
        for (i, child) in children.iter().enumerate() {
            self.storage
                .index
                .reparent(child.id, idx.parent_id.unwrap(), i as i64)
                .await
                .map_err(|e| e.to_string())?;
        }

        self.storage
            .index
            .delete(idx.id)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(parent_id) = idx.parent_id {
            self.storage
                .index
                .reindex_positions(Some(parent_id))
                .await
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    pub async fn delete_knowledge(&self, title: &str) -> Result<(), String> {
        let id = self.resolve_knowledge(title).await?;

        let referencing_indexes = self
            .storage
            .index
            .find_by_target(id)
            .await
            .map_err(|e| e.to_string())?;
        for idx in &referencing_indexes {
            self.storage
                .index
                .downgrade_to_group(idx.id)
                .await
                .map_err(|e| e.to_string())?;
        }

        self.storage
            .knowledge
            .delete(id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn create_knowledge_by_ref(
        &self,
        title: &str,
        knowledge_type: KnowledgeType,
        entity_refs: Vec<&str>,
        content: Option<String>,
    ) -> Result<Knowledge, String> {
        let mut entities = Vec::with_capacity(entity_refs.len());
        for r in entity_refs {
            let id = self.resolve(r).await.map_err(|_| {
                format!("entity '{}' not found, please create it first with kms_create_entity", r)
            })?;
            entities.push(id);
        }
        self.create_knowledge(title, knowledge_type, entities, content)
            .await
    }

    pub async fn get_index(&self, id: Uuid) -> Result<Index, String> {
        self.storage.index.get(id).await.map_err(|e| e.to_string())
    }

    pub async fn get_children(
        &self,
        parent_id: Option<Uuid>,
    ) -> Result<Vec<Index>, String> {
        self.storage
            .index
            .children_of(parent_id)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_entity_knowledge_under_index(
        &self,
        index_title: &str,
        entity_name: &str,
    ) -> Result<Vec<Knowledge>, String> {
        let index_id = self.resolve_index(index_title).await?;
        let entity_id = self.resolve(entity_name).await?;
        let knowledge_ids = self
            .storage
            .index
            .subtree_knowledge_ids(index_id)
            .await
            .map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        for kid in knowledge_ids {
            if let Ok(k) = self.get_knowledge(kid).await {
                if k.entities.contains(&entity_id) {
                    results.push(k);
                }
            }
        }
        Ok(results)
    }

    /// Returns all Knowledge entries that reference the given entity.
    pub async fn get_entity_referencing_knowledge(
        &self,
        entity_id: Uuid,
    ) -> Result<Vec<Knowledge>, String> {
        self.storage
            .knowledge
            .find_by_entity(entity_id)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn navigate(&self, path: &str) -> Result<String, String> {
        let current = self.get_pointer().await;

        if path == ".." {
            let node = self.get_index(current).await?;
            if let Some(parent_id) = node.parent_id {
                self.set_pointer(parent_id).await;
            }
            return self.render_location().await;
        }

        let (base_id, segments) = if path.starts_with('/') {
            let root = self
                .storage
                .index
                .find_root()
                .await
                .map_err(|e| e.to_string())?;
            (root.id, path[1..].split('/').collect::<Vec<_>>())
        } else if path.starts_with("../") {
            let node = self.get_index(current).await?;
            match node.parent_id {
                Some(pid) => (pid, path[3..].split('/').collect::<Vec<_>>()),
                None => return Err("already at root, cannot go to parent".into()),
            }
        } else if path.contains('/') {
            (current, path.split('/').collect::<Vec<_>>())
        } else {
            (current, vec![path])
        };

        let mut pointer = base_id;
        for seg in segments {
            let seg = seg.trim();
            if seg.is_empty() {
                continue;
            }
            if seg == ".." {
                let node = self.get_index(pointer).await?;
                match node.parent_id {
                    Some(pid) => pointer = pid,
                    None => return Err("already at root, cannot go to parent".into()),
                }
            } else {
                let children = self.get_children(Some(pointer)).await?;
                match children.iter().find(|c| c.title.as_deref() == Some(seg)) {
                    Some(child) => pointer = child.id,
                    None => {
                        return Err(format!(
                            "segment '{}' not found as child of current node",
                            seg
                        ))
                    }
                }
            }
        }

        self.set_pointer(pointer).await;
        self.render_location().await
    }

    pub async fn reorganize_children(
        &self,
        new_group_title: &str,
        child_titles: &[String],
    ) -> Result<String, String> {
        let current_id = self.get_pointer().await;
        let current_node = self.get_index(current_id).await?;

        let children = self
            .get_children(Some(current_id))
            .await?;

        let mut child_indices: Vec<Index> = Vec::new();
        for title in child_titles {
            let found = children
                .iter()
                .find(|c| c.title.as_deref() == Some(title.as_str()))
                .ok_or_else(|| format!("'{}' is not a child of current node", title))?;
            child_indices.push(found.clone());
        }

        let new_group = self
            .create_index(
                current_id,
                Some(new_group_title.to_string()),
                None,
                Some(TargetType::Group),
            )
            .await?;
        let new_group_id = new_group.id;

        for (i, child) in child_indices.iter().enumerate() {
            self.storage
                .index
                .reparent(child.id, new_group_id, i as i64)
                .await
                .map_err(|e| e.to_string())?;
        }

        self.storage
            .index
            .reindex_positions(Some(current_id))
            .await
            .map_err(|e| e.to_string())?;

        self.storage
            .index
            .reindex_positions(Some(new_group_id))
            .await
            .map_err(|e| e.to_string())?;

        self.set_pointer(new_group_id).await;

        self.render_location().await
    }

    pub async fn move_index(&self, index_title: &str, new_parent_title: &str) -> Result<String, String> {
        let idx = self
            .storage
            .index
            .find_by_title(index_title)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("index '{}' not found", index_title))?;

        if idx.parent_id.is_none() {
            return Err("cannot move the root index".into());
        }

        let new_parent = self
            .storage
            .index
            .find_by_title(new_parent_title)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("new parent '{}' not found", new_parent_title))?;

        if idx.parent_id == Some(new_parent.id) {
            return Err(format!("'{}' is already under '{}'", index_title, new_parent_title));
        }

        let old_parent_id = idx.parent_id;

        let target_children = self
            .storage
            .index
            .children_of(Some(new_parent.id))
            .await
            .map_err(|e| e.to_string())?;
        let new_position = target_children.len() as i64;

        self.storage
            .index
            .reparent(idx.id, new_parent.id, new_position)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(oid) = old_parent_id {
            self.storage
                .index
                .reindex_positions(Some(oid))
                .await
                .map_err(|e| e.to_string())?;
        }

        self.storage
            .index
            .reindex_positions(Some(new_parent.id))
            .await
            .map_err(|e| e.to_string())?;

        let location = self.render_location().await?;
        Ok(format!(
            "moved '{}' under '{}'\n{}",
            index_title, new_parent_title, location
        ))
    }

    pub async fn diagnose(&self) -> Result<Vec<Diagnostic>, String> {
        diagnostics::run_diagnostics(&self.storage).await
    }

    /// Create a sibling [`KmsService`] that shares the underlying
    /// [`Storage`] (and thus the SQLite connection pool) but owns an
    /// independent pointer. Sub-agents use this so that they can each
    /// carry their own navigation position without disturbing one
    /// another.
    pub fn with_pointer(&self, pointer: Uuid) -> KmsService {
        KmsService {
            inner: std::sync::Arc::new(Inner {
                pointer: tokio::sync::RwLock::new(pointer),
            }),
            storage: self.storage.clone(),
        }
    }

    /// Move every direct child of `sub_root_id` under
    /// `target_parent_id` (appended to the existing siblings) and
    /// delete the now-empty `sub_root_id` node. Positions on the
    /// target parent are reindexed afterwards.
    pub async fn merge_subtree(
        &self,
        sub_root_id: Uuid,
        target_parent_id: Uuid,
    ) -> Result<usize, String> {
        if sub_root_id == target_parent_id {
            return Err("sub_root and target_parent must differ".into());
        }

        let sub_root = self.get_index(sub_root_id).await?;
        if sub_root.parent_id.is_none() {
            return Err("cannot merge the system root".into());
        }

        let children = self
            .storage
            .index
            .children_of(Some(sub_root_id))
            .await
            .map_err(|e| e.to_string())?;

        let existing_count = self
            .storage
            .index
            .children_of(Some(target_parent_id))
            .await
            .map_err(|e| e.to_string())?
            .len();

        for (i, child) in children.iter().enumerate() {
            self.storage
                .index
                .reparent(child.id, target_parent_id, (existing_count + i) as i64)
                .await
                .map_err(|e| e.to_string())?;
        }

        if !children.is_empty() {
            self.storage
                .index
                .reindex_positions(Some(target_parent_id))
                .await
                .map_err(|e| e.to_string())?;
        }

        self.storage
            .index
            .delete(sub_root_id)
            .await
            .map_err(|e| e.to_string())?;

        Ok(children.len())
    }

    pub async fn render_location(&self) -> Result<String, String> {
        let current = self.get_pointer().await;

        let path = self.ancestor_path(current).await?;
        let current_id = path.last().map(|n| n.id).unwrap_or(current);
        let children = self.get_children(Some(current_id)).await?;

        let mut s = String::new();
        for (i, node) in path.iter().enumerate() {
            let title = node.title.as_deref().unwrap_or("(unnamed)");
            let is_root = node.parent_id.is_none();
            let is_current = node.id == current;
            let is_last = i == path.len() - 1;

            if i > 0 {
                s.push_str(if is_last {
                    "  └── "
                } else {
                    "  ├── "
                });
            } else {
                s.push_str("## ");
            }

            if is_current {
                s.push_str(&format!("**{}**", title));
            } else {
                s.push_str(title);
            }

            if is_root && i == 0 {
                s.push_str(" (system root, read-only)");
            }
            s.push('\n');
        }

        if path.is_empty() {
            s.push_str("## (pointer not initialized)\n");
        } else if children.is_empty() {
            s.push_str("      (empty)\n");
        } else {
            let last = children.len() - 1;
            for (i, c) in children.iter().enumerate() {
                let t = c.title.as_deref().unwrap_or("(unnamed)");
                let connector = if i == last {
                    "  └── "
                } else {
                    "  ├── "
                };
                let suffix = match c.target_type {
                    TargetType::Group => "",
                    TargetType::Knowledge => " [knowledge]",
                };
                s.push_str(&format!("{}{}{}{}\n", connector, t, suffix, ""));
            }
        }
        Ok(s)
    }

    pub async fn render_full_tree(&self) -> Result<String, String> {
        let root = self
            .storage
            .index
            .find_root()
            .await
            .map_err(|e| e.to_string())?;

        let mut s = String::new();
        let mut stack: Vec<(Index, String, String)> = vec![(root, String::new(), String::new())];

        while let Some((node, indent, connector)) = stack.pop() {
            let title = node.title.as_deref().unwrap_or("(unnamed)");
            let is_root = node.parent_id.is_none();

            if is_root {
                s.push_str(&format!("## {} (system root)\n", title));
            } else {
                let suffix = match node.target_type {
                    TargetType::Group => "",
                    TargetType::Knowledge => " [knowledge]",
                };
                s.push_str(&format!("{}{}{}{}\n", indent, connector, title, suffix));
            }

            let children = match self.get_children(Some(node.id)).await {
                Ok(c) => c,
                Err(_) => continue,
            };
            if children.is_empty() {
                continue;
            }

            let n = children.len();
            for i in (0..n).rev() {
                let child = &children[i];
                let is_last = i == n - 1;
                let c_connector = if is_last { "└── " } else { "├── " };
                let branch = if is_last { "   " } else { "│  " };
                let c_indent = if is_root {
                    branch.to_string()
                } else {
                    format!("{}{}", indent, branch)
                };
                stack.push((child.clone(), c_indent, c_connector.to_string()));
            }
        }

        Ok(s)
    }

    async fn ancestor_path(&self, target_id: Uuid) -> Result<Vec<Index>, String> {
        let mut path = Vec::new();
        let mut id = target_id;
        loop {
            let node = self.get_index(id).await?;
            let parent_id = node.parent_id;
            path.push(node);
            match parent_id {
                Some(pid) => id = pid,
                None => {
                    path.reverse();
                    return Ok(path);
                }
            }
        }
    }

    // -----------------------------------------------------------------
    //  Local-view (stateless) API
    //
    //  These methods are intentionally side-effect free: they never
    //  touch `self.inner.pointer` and can be invoked concurrently from
    //  multiple read-only agents without interfering with one another
    //  or with mutating agents that still use `navigate`.
    // -----------------------------------------------------------------

    /// Build a [`LocalView`] for the node identified by `node_id` without
    /// mutating the global pointer.
    pub async fn get_local_view(&self, node_id: Uuid) -> Result<LocalView, String> {
        // 1) ancestor path
        let path_rows = self
            .storage
            .index
            .ancestor_path_rows(node_id)
            .await
            .map_err(|e| e.to_string())?;

        // Reconstruct full Index rows from the ancestor rows. The path
        // rows include the requested node at depth 0.
        let path: Vec<Index> = path_rows
            .iter()
            .map(|r| {
                Ok::<Index, String>(Index {
                    id: Uuid::parse_str(&r.id).map_err(|e| e.to_string())?,
                    title: r.title.clone(),
                    target: match &r.target {
                        Some(t) => Some(Uuid::parse_str(t).map_err(|e| e.to_string())?),
                        None => None,
                    },
                    target_type: match r.target_type.as_deref() {
                        Some("knowledge") => TargetType::Knowledge,
                        _ => TargetType::Group,
                    },
                    parent_id: match &r.parent_id {
                        Some(p) => Some(Uuid::parse_str(p).map_err(|e| e.to_string())?),
                        None => None,
                    },
                    position: r.position,
                })
            })
            .collect::<Result<_, _>>()?;

        // The first row in the path (depth 0) is the requested node.
        let node = path
            .first()
            .cloned()
            .ok_or_else(|| format!("node {} not found", node_id))?;

        // 2) direct children
        let child_rows = self
            .storage
            .index
            .child_rows(node_id)
            .await
            .map_err(|e| e.to_string())?;
        let mut children: Vec<IndexView> = Vec::with_capacity(child_rows.len());
        for r in child_rows {
            let id = Uuid::parse_str(&r.id).map_err(|e| e.to_string())?;
            let title = r.title.unwrap_or_else(|| "(unnamed)".to_string());
            let target_type = match r.target_type.as_deref() {
                Some("knowledge") => TargetType::Knowledge,
                _ => TargetType::Group,
            };
            children.push(IndexView {
                id,
                title,
                target_type,
                position: r.position,
            });
        }

        // 3) subtree statistics
        let stats = self
            .storage
            .index
            .subtree_stats(node_id, SUBTREE_TITLES_LIMIT)
            .await
            .map_err(|e| e.to_string())?;

        // 4) sibling count (number of children of the node's parent,
        //    including the node itself).
        let sibling_count = self
            .storage
            .index
            .sibling_count(node_id)
            .await
            .map_err(|e| e.to_string())?;

        Ok(LocalView {
            node,
            path,
            children,
            sibling_count,
            subtree_summary: crate::view::SubtreeSummary {
                total_nodes: stats.total_nodes,
                knowledge_count: stats.knowledge_count,
                group_count: stats.group_count,
                max_depth: stats.max_depth,
                knowledge_titles: stats.knowledge_titles,
                truncated: stats.truncated,
            },
        })
    }

    /// Like [`get_local_view`](Self::get_local_view), but accepts a path
    /// string compatible with [`navigate`](Self::navigate). The path is
    /// resolved **without** mutating the global pointer; the original
    /// `target` parameter is preserved for back-compat with the
    /// stateful navigation API.
    pub async fn get_local_view_by_path(&self, path: &str) -> Result<LocalView, String> {
        let target_id = self.resolve_path_id(path).await?;
        self.get_local_view(target_id).await
    }

    /// Return **all** knowledge entries inside the subtree rooted at
    /// `node_id`. Use this when `LocalView::subtree_summary` is
    /// truncated and the agent needs the full list.
    pub async fn get_subtree_knowledge(&self, node_id: Uuid) -> Result<Vec<Knowledge>, String> {
        let ids = self
            .storage
            .index
            .subtree_knowledge_ids(node_id)
            .await
            .map_err(|e| e.to_string())?;

        let mut out = Vec::with_capacity(ids.len());
        for kid in ids {
            match self.get_knowledge(kid).await {
                Ok(k) => out.push(k),
                Err(_) => continue, // tolerate dangling references
            }
        }
        Ok(out)
    }

    /// Convenience wrapper: resolve a path string, then return the
    /// subtree's knowledge entries.
    pub async fn get_subtree_knowledge_by_path(
        &self,
        path: &str,
    ) -> Result<Vec<Knowledge>, String> {
        let target_id = self.resolve_path_id(path).await?;
        self.get_subtree_knowledge(target_id).await
    }

    /// Return knowledge entries inside the subtree rooted at `node_id`
    /// whose title contains `keyword` (case-insensitive substring).
    pub async fn search_knowledge_titles(
        &self,
        node_id: Uuid,
        keyword: &str,
    ) -> Result<Vec<Knowledge>, String> {
        let all = self.get_subtree_knowledge(node_id).await?;
        let kw = keyword.to_lowercase();
        Ok(all
            .into_iter()
            .filter(|k| k.title.to_lowercase().contains(&kw))
            .collect())
    }

    /// Internal helper: resolve a navigate-style path string to a node
    /// id without mutating the global pointer. Mirrors the resolution
    /// logic of [`navigate`](Self::navigate) but discards any stateful
    /// effect.
    async fn resolve_path_id(&self, path: &str) -> Result<Uuid, String> {
        if path.is_empty() {
            return Err("path is empty".into());
        }

        // Absolute path: start at the root.
        if let Some(stripped) = path.strip_prefix('/') {
            let root = self
                .storage
                .index
                .find_root()
                .await
                .map_err(|e| e.to_string())?;
            if stripped.is_empty() {
                return Ok(root.id);
            }
            let mut id = root.id;
            for seg in stripped.split('/') {
                let seg = seg.trim();
                if seg.is_empty() || seg == "." {
                    continue;
                }
                id = self.descend(id, seg).await?;
            }
            return Ok(id);
        }

        // `..` alone: error out (need a current pointer context).
        if path == ".." {
            return Err("'..' requires a current pointer; use an absolute path or a sub-segment of an existing path".into());
        }

        // Relative path starting with `../`: walk up N levels from a
        // notional current position. We resolve against the global
        // pointer for compatibility with the stateful `navigate`.
        if let Some(stripped) = path.strip_prefix("../") {
            let mut id = self.get_pointer().await;
            for _ in 0..path.matches("../").count() {
                let node = self.get_index(id).await?;
                id = node
                    .parent_id
                    .ok_or_else(|| "already at root, cannot go to parent".to_string())?;
            }
            for seg in stripped.split('/') {
                let seg = seg.trim();
                if seg.is_empty() {
                    continue;
                }
                if seg == ".." {
                    let node = self.get_index(id).await?;
                    id = node
                        .parent_id
                        .ok_or_else(|| "already at root, cannot go to parent".to_string())?;
                } else {
                    id = self.descend(id, seg).await?;
                }
            }
            return Ok(id);
        }

        // Multi-segment relative path: treat first segment as a child
        // of the global pointer; subsequent segments descend.
        if path.contains('/') {
            let mut id = self.get_pointer().await;
            for seg in path.split('/') {
                let seg = seg.trim();
                if seg.is_empty() {
                    continue;
                }
                id = self.descend(id, seg).await?;
            }
            return Ok(id);
        }

        // Single segment: descend from the global pointer.
        let id = self.get_pointer().await;
        self.descend(id, path.trim()).await
    }

    // -----------------------------------------------------------------
    //  Document buffer layer
    // -----------------------------------------------------------------

    /// Ingest a long text document: split into chunks and persist to
    /// the `documents` / `document_chunks` tables. Returns the
    /// [`Document`] metadata.
    pub async fn ingest_document(
        &self,
        title: &str,
        source: Option<&str>,
        content: &str,
    ) -> Result<Document, String> {
        let id = Uuid::new_v4();
        let chunks = document::chunk_text(id, content, DEFAULT_CHUNK_SIZE, DEFAULT_CHUNK_OVERLAP);
        let char_count = content.chars().count();
        let chunk_count = chunks.len();
        let created_at = iso_now();

        self.storage
            .document
            .create_document(id, title, source, char_count, chunk_count, &created_at)
            .await
            .map_err(|e| e.to_string())?;

        for chunk in &chunks {
            self.storage
                .document
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
        let rows = self
            .storage
            .document
            .list_documents()
            .await
            .map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(|r| {
                Ok(Document {
                    id: Uuid::parse_str(&r.id).map_err(|e| e.to_string())?,
                    title: r.title,
                    source: r.source,
                    char_count: r.char_count.max(0) as usize,
                    chunk_count: r.chunk_count.max(0) as usize,
                    created_at: r.created_at,
                })
            })
            .collect()
    }

    /// Get metadata for a single document.
    pub async fn get_document(&self, id: Uuid) -> Result<Document, String> {
        let row = self
            .storage
            .document
            .get_document(id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("document not found: {id}"))?;
        Ok(Document {
            id,
            title: row.title,
            source: row.source,
            char_count: row.char_count.max(0) as usize,
            chunk_count: row.chunk_count.max(0) as usize,
            created_at: row.created_at,
        })
    }

    /// Get a single chunk by (doc_id, chunk_index).
    pub async fn get_document_chunk(
        &self,
        id: Uuid,
        chunk_index: usize,
    ) -> Result<DocumentChunk, String> {
        let row = self
            .storage
            .document
            .get_chunk(id, chunk_index)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                format!("chunk {chunk_index} not found in document {id}")
            })?;
        Ok(DocumentChunk {
            document_id: id,
            index: row.chunk_index.max(0) as usize,
            content: row.content,
            char_start: row.char_start.max(0) as usize,
            char_end: row.char_end.max(0) as usize,
        })
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
        let rows = self
            .storage
            .document
            .get_chunks_window(id, start, end)
            .await
            .map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(|r| {
                Ok(DocumentChunk {
                    document_id: id,
                    index: r.chunk_index.max(0) as usize,
                    content: r.content,
                    char_start: r.char_start.max(0) as usize,
                    char_end: r.char_end.max(0) as usize,
                })
            })
            .collect()
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
            .document
            .search_keyword(id, keyword)
            .await
            .map_err(|e| e.to_string())?;
        let _doc = self.get_document(id).await?;
        Ok(hits
            .into_iter()
            .take(top_k)
            .map(|(idx, snippet)| {
                let chunk_start = idx.saturating_sub(1) * DEFAULT_CHUNK_SIZE;
                ChunkHit {
                    document_id: id,
                    index: idx,
                    snippet,
                    char_start: chunk_start,
                    char_end: chunk_start + DEFAULT_CHUNK_SIZE,
                }
            })
            .collect())
    }

    /// Delete a document and all its chunks. CASCADE will remove
    /// chunks automatically; knowledge rows that reference this
    /// document will have `source_document_id` set to NULL.
    pub async fn delete_document(&self, id: Uuid) -> Result<(), String> {
        self.storage
            .document
            .delete_document(id)
            .await
            .map_err(|e| e.to_string())
    }

    /// Like [`create_knowledge`](Self::create_knowledge) but accepts an
    /// optional `(source_document_id, source_chunk_idx)` pair for
    /// provenance tracking.
    pub async fn create_knowledge_with_source(
        &self,
        title: &str,
        knowledge_type: KnowledgeType,
        entities: Vec<Uuid>,
        content: Option<String>,
        source: Option<(Uuid, usize)>,
    ) -> Result<Knowledge, String> {
        let (source_document_id, source_chunk_idx) = match source {
            Some((doc_id, chunk_idx)) => (Some(doc_id), Some(chunk_idx as i64)),
            None => (None, None),
        };
        let knowledge = Knowledge {
            id: Uuid::new_v4(),
            title: title.to_string(),
            knowledge_type,
            entities,
            content,
            source_document_id,
            source_chunk_idx,
        };

        self.storage
            .knowledge
            .create(&knowledge)
            .await
            .map_err(|e| e.to_string())?;
        Ok(knowledge)
    }

    /// Find a direct child of `parent_id` whose title equals `title`.
    async fn descend(&self, parent_id: Uuid, title: &str) -> Result<Uuid, String> {
        let children = self.get_children(Some(parent_id)).await?;
        children
            .iter()
            .find(|c| c.title.as_deref() == Some(title))
            .map(|c| c.id)
            .ok_or_else(|| format!("segment '{}' not found as child of current node", title))
    }
}

async fn ensure_root_index(storage: &Storage) -> Result<Uuid, String> {
    use crate::storage::repo::IndexRepo;
    use crate::storage::types::{Index, TargetType};

    let existing = sqlx::query_as::<sqlx::Sqlite, (String,)>(
        "SELECT id FROM indexes WHERE parent_id IS NULL LIMIT 1",
    )
    .fetch_optional(storage.pool())
    .await
    .map_err(|e| e.to_string())?;

    match existing {
        Some((id_str,)) => Uuid::parse_str(&id_str).map_err(|e| e.to_string()),
        None => {
            let id = Uuid::new_v4();
            let entry = Index {
                id,
                title: Some("Root".to_string()),
                target: None,
                target_type: TargetType::Group,
                parent_id: None,
                position: 0,
            };
            storage
                .index
                .create(&entry)
                .await
                .map_err(|e| e.to_string())?;
            Ok(id)
        }
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;
    use std::sync::Arc;

    use super::*;
    use crate::language::Language;

    fn make_name(full: &str) -> Nomenclature {
        Nomenclature {
            id: Uuid::new_v4(),
            lang: Language::ZH,
            full: full.to_string(),
            abbr: None,
        }
    }

    async fn setup_service() -> KmsService {
        let pool = SqlitePoolOptions::new()
            .max_lifetime(std::time::Duration::from_secs(1))
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("migrations/sqlite")
            .run(&pool)
            .await
            .unwrap();
        KmsService {
            inner: Arc::new(Inner {
                pointer: RwLock::new(Uuid::nil()),
            }),
            storage: Storage::from_pool(pool),
        }
    }

    #[tokio::test]
    async fn test_entity_crud() {
        let svc = setup_service().await;
        let (e, existed) = svc
            .create_entity(vec![make_name("测试实体")], "定义")
            .await
            .unwrap();
        assert!(!existed);
        assert_eq!(e.definition, "定义");
        assert!(!e.name.is_empty());
        assert_eq!(e.name[0].full, "测试实体");

        let got = svc.get_entity(e.id).await.unwrap();
        assert_eq!(got.id, e.id);
    }

    #[tokio::test]
    async fn test_knowledge_crud() {
        let svc = setup_service().await;
        let (e, _existed) = svc
            .create_entity(vec![make_name("实体")], "定义")
            .await
            .unwrap();
        let k = svc
            .create_knowledge("标题", KnowledgeType::Aspect, vec![e.id], None)
            .await
            .unwrap();
        assert_eq!(k.title, "标题");
        assert_eq!(k.entities, vec![e.id]);

        let got = svc.get_knowledge(k.id).await.unwrap();
        assert_eq!(got.id, k.id);
    }

    #[tokio::test]
    async fn test_index_tree() {
        let svc = setup_service().await;
        let (_e, _existed) = svc
            .create_entity(vec![make_name("实体")], "定义")
            .await
            .unwrap();

        let root = svc.create_index_root("根节点").await.unwrap();
        assert!(root.parent_id.is_none());

        let child = svc
            .create_index(root.id, Some("子节点".into()), None, None)
            .await
            .unwrap();
        assert_eq!(child.parent_id, Some(root.id));
        assert!(child.target.is_none());

        let children = svc.get_children(Some(root.id)).await.unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, child.id);
    }

    #[tokio::test]
    async fn test_index_position_auto_increment() {
        let svc = setup_service().await;
        let root = svc.create_index_root("根").await.unwrap();
        let c1 = svc
            .create_index(root.id, Some("c1".into()), None, None)
            .await
            .unwrap();
        let c2 = svc
            .create_index(root.id, Some("c2".into()), None, None)
            .await
            .unwrap();
        let c3 = svc
            .create_index(root.id, Some("c3".into()), None, None)
            .await
            .unwrap();

        assert_eq!(c1.position, 0);
        assert_eq!(c2.position, 1);
        assert_eq!(c3.position, 2);
    }

    #[tokio::test]
    async fn test_get_index() {
        let svc = setup_service().await;
        let root = svc.create_index_root("根").await.unwrap();
        let child = svc
            .create_index(root.id, Some("子".into()), None, None)
            .await
            .unwrap();

        let got = svc.get_index(child.id).await.unwrap();
        assert_eq!(got.id, child.id);
    }

    // ---------- local-view tests ----------

    /// Build a small tree:
    ///
    /// ```text
    /// root
    /// ├── 父节点
    /// │   ├── 子节点A (knowledge)
    /// │   └── 子节点B
    /// └── 旁系
    /// ```
    async fn build_sample_tree(svc: &KmsService) -> SampleTree {
        let (e, _) = svc
            .create_entity(vec![make_name("测试实体")], "定义")
            .await
            .unwrap();
        let k = svc
            .create_knowledge("A · 病因", KnowledgeType::Aspect, vec![e.id], None)
            .await
            .unwrap();
        let root = svc.create_index_root("Root").await.unwrap();
        let parent = svc
            .create_index(root.id, Some("父节点".into()), None, Some(TargetType::Group))
            .await
            .unwrap();
        let a = svc
            .create_index(
                parent.id,
                Some("子节点A".into()),
                Some(k.id),
                Some(TargetType::Knowledge),
            )
            .await
            .unwrap();
        let b = svc
            .create_index(
                parent.id,
                Some("子节点B".into()),
                None,
                Some(TargetType::Group),
            )
            .await
            .unwrap();
        let sibling = svc
            .create_index(root.id, Some("旁系".into()), None, Some(TargetType::Group))
            .await
            .unwrap();
        SampleTree {
            _e: e,
            _k: k,
            root,
            parent,
            a,
            b,
            sibling,
        }
    }

    struct SampleTree {
        _e: Entity,
        _k: Knowledge,
        root: Index,
        parent: Index,
        a: Index,
        b: Index,
        sibling: Index,
    }

    #[tokio::test]
    async fn test_get_local_view_returns_node_and_children() {
        let svc = setup_service().await;
        let tree = build_sample_tree(&svc).await;

        let view = svc.get_local_view(tree.parent.id).await.unwrap();
        assert_eq!(view.node.id, tree.parent.id);
        assert_eq!(view.children.len(), 2);
        let titles: Vec<&str> = view.children.iter().map(|c| c.title.as_str()).collect();
        assert!(titles.contains(&"子节点A"));
        assert!(titles.contains(&"子节点B"));
        // path contains root + parent
        assert_eq!(view.path.len(), 2);
        assert_eq!(view.path[0].id, tree.parent.id);
        assert_eq!(view.path[1].id, tree.root.id);
    }

    #[tokio::test]
    async fn test_local_view_subtree_stats() {
        let svc = setup_service().await;
        let tree = build_sample_tree(&svc).await;

        let view = svc.get_local_view(tree.root.id).await.unwrap();
        // The root subtree contains: root, parent, A, B, sibling = 5 nodes
        assert_eq!(view.subtree_summary.total_nodes, 5);
        // Only A is a knowledge node
        assert_eq!(view.subtree_summary.knowledge_count, 1);
        assert_eq!(view.subtree_summary.group_count, 4);
        // Depth from root: root(0) -> parent(1) -> A/B(2) = 2
        assert_eq!(view.subtree_summary.max_depth, 2);
        assert!(view
            .subtree_summary
            .knowledge_titles
            .contains(&"A · 病因".to_string()));
        assert!(!view.subtree_summary.truncated);
    }

    #[tokio::test]
    async fn test_local_view_does_not_mutate_pointer() {
        let svc = setup_service().await;
        let tree = build_sample_tree(&svc).await;
        let initial = svc.get_pointer().await;

        // Call the new stateless methods and confirm the pointer is
        // unchanged.
        let _ = svc.get_local_view(tree.a.id).await.unwrap();
        let _ = svc.get_local_view(tree.b.id).await.unwrap();
        let _ = svc
            .get_local_view_by_path("/父节点/子节点A")
            .await
            .unwrap();
        let _ = svc.get_subtree_knowledge(tree.root.id).await.unwrap();
        let _ = svc
            .search_knowledge_titles(tree.root.id, "病因")
            .await
            .unwrap();

        let after = svc.get_pointer().await;
        assert_eq!(initial, after, "stateless methods must not move the pointer");
    }

    #[tokio::test]
    async fn test_get_local_view_by_path() {
        let svc = setup_service().await;
        let tree = build_sample_tree(&svc).await;

        let v1 = svc.get_local_view_by_path("/父节点").await.unwrap();
        assert_eq!(v1.node.id, tree.parent.id);

        let v2 = svc.get_local_view_by_path("/父节点/子节点A").await.unwrap();
        assert_eq!(v2.node.id, tree.a.id);
        assert_eq!(v2.children.len(), 0);

        // Relative (single segment) uses the global pointer.
        let _ = svc.set_pointer_for_test(tree.root.id).await;
        let v3 = svc.get_local_view_by_path("父节点").await.unwrap();
        assert_eq!(v3.node.id, tree.parent.id);
    }

    #[tokio::test]
    async fn test_get_subtree_knowledge() {
        let svc = setup_service().await;
        let tree = build_sample_tree(&svc).await;

        let subtree = svc.get_subtree_knowledge(tree.root.id).await.unwrap();
        assert_eq!(subtree.len(), 1);
        assert_eq!(subtree[0].title, "A · 病因");

        let subtree_a = svc.get_subtree_knowledge(tree.a.id).await.unwrap();
        assert_eq!(subtree_a.len(), 1);
    }

    #[tokio::test]
    async fn test_search_knowledge_titles() {
        let svc = setup_service().await;
        let tree = build_sample_tree(&svc).await;

        let hits = svc
            .search_knowledge_titles(tree.root.id, "病因")
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "A · 病因");

        let empty = svc
            .search_knowledge_titles(tree.root.id, "nonexistent")
            .await
            .unwrap();
        assert!(empty.is_empty());
    }

    // ---------- parallel-subtree tests ----------

    #[tokio::test]
    async fn test_with_pointer_shares_storage() {
        let svc = setup_service().await;
        let tree = build_sample_tree(&svc).await;

        let sub = svc.with_pointer(tree.a.id);
        // Reading a node still works against the shared storage.
        let got = sub.get_index(tree.a.id).await.unwrap();
        assert_eq!(got.id, tree.a.id);
    }

    #[tokio::test]
    async fn test_with_pointer_isolates_pointer() {
        let svc = setup_service().await;
        let tree = build_sample_tree(&svc).await;

        let _ = svc.set_pointer_for_test(tree.root.id).await;
        let sub = svc.with_pointer(tree.a.id);

        // Navigating on the sub-service must not move the parent pointer.
        sub.navigate("..").await.unwrap();
        assert_eq!(svc.get_pointer().await, tree.root.id);
    }

    #[tokio::test]
    async fn test_merge_subtree_reparents_children_and_deletes_staging() {
        let svc = setup_service().await;
        let tree = build_sample_tree(&svc).await;

        // Build a staging area: root -> [staging -> [x, y]]
        let staging = svc
            .create_index(
                tree.root.id,
                Some("staging-测试".into()),
                None,
                Some(TargetType::Group),
            )
            .await
            .unwrap();
        let x = svc
            .create_index(
                staging.id,
                Some("x".into()),
                None,
                Some(TargetType::Group),
            )
            .await
            .unwrap();
        let y = svc
            .create_index(
                staging.id,
                Some("y".into()),
                None,
                Some(TargetType::Group),
            )
            .await
            .unwrap();

        let moved = svc
            .merge_subtree(staging.id, tree.parent.id)
            .await
            .unwrap();
        assert_eq!(moved, 2);

        // staging node itself is gone.
        let err = svc.get_index(staging.id).await.unwrap_err();
        assert!(matches!(err, String) );

        // x and y now sit under tree.parent with consecutive positions.
        let new_children = svc.get_children(Some(tree.parent.id)).await.unwrap();
        let titles: Vec<String> = new_children
            .iter()
            .map(|c| c.title.clone().unwrap_or_default())
            .collect();
        assert!(titles.contains(&"x".to_string()));
        assert!(titles.contains(&"y".to_string()));
        // Positions for x and y should be >= previous sibling count.
        let x_pos = new_children.iter().find(|c| c.id == x.id).unwrap().position;
        let y_pos = new_children.iter().find(|c| c.id == y.id).unwrap().position;
        assert!(x_pos >= 2);
        assert!(y_pos >= 2);
        assert_ne!(x_pos, y_pos);
    }

    #[tokio::test]
    async fn test_merge_subtree_rejects_root() {
        let svc = setup_service().await;
        let tree = build_sample_tree(&svc).await;

        let err = svc
            .merge_subtree(tree.root.id, tree.parent.id)
            .await
            .unwrap_err();
        assert!(err.contains("root"));
    }

    #[tokio::test]
    async fn test_merge_subtree_rejects_self() {
        let svc = setup_service().await;
        let tree = build_sample_tree(&svc).await;

        let err = svc
            .merge_subtree(tree.parent.id, tree.parent.id)
            .await
            .unwrap_err();
        assert!(err.contains("differ"));
    }
}

impl KmsService {
    /// Test-only helper: set the global pointer to a known id.
    #[cfg(test)]
    pub(crate) async fn set_pointer_for_test(&self, id: Uuid) {
        *self.inner.pointer.write().await = id;
    }
}
