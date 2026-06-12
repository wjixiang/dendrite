use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::Storage;
use crate::language::Language;
use crate::storage::types::{Entity, Index, Knowledge, KnowledgeType, Nomenclature, TargetType};
use serde::Serialize;

/// Outcome of a [`KmsService::move_children`] call. Returned alongside
/// the rendered location so the tool layer can echo a structured
/// result to the agent (the agent can then act on `group_created` to
/// know whether the destination group was a fresh container or an
/// already-existing one it just appended to).
#[derive(Debug, Clone, Serialize)]
pub struct MoveChildrenResult {
    /// Rendered location string pointing at the destination group.
    pub location: String,
    /// UUID of the destination group (whether newly created or reused).
    pub new_group_id: Uuid,
    /// `true` if a fresh group was created, `false` if an existing
    /// `Group`-typed child with the same title was reused.
    pub group_created: bool,
}
use crate::view::{IndexView, LocalView, SUBTREE_TITLES_LIMIT};

use crate::Diagnostic;
use crate::diagnostics;
use corpus::CorpusService;

use crate::storage::repo::{EntityRepo, IndexRepo, KnowledgeRepo};

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
    corpus: Arc<CorpusService>,
}

impl KmsService {
    pub async fn new(db_path: &str, corpus: Arc<CorpusService>) -> Result<Self, String> {
        let storage = Storage::new(db_path).await?;

        let root_id = ensure_root_index(&storage).await?;

        let inner = std::sync::Arc::new(Inner {
            pointer: RwLock::new(root_id),
        });

        Ok(KmsService { inner, storage, corpus })
    }

    /// Borrow the corpus service handle held by this KMS.
    pub fn corpus(&self) -> &Arc<CorpusService> {
        &self.corpus
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

        // Enforce unique child titles within the same parent. The tree's
        // navigation rules already disambiguate paths by sequence, so a
        // duplicate title would silently shadow one of the siblings and
        // break every title-based lookup (`find_by_title`, `resolve_index`,
        // `navigate`, `move_index`, ...). Reject at creation time so the
        // tree stays self-consistent.
        if let Some(new_title) = title.as_deref() {
            if let Some(conflict) = siblings
                .iter()
                .find(|c| c.title.as_deref() == Some(new_title))
            {
                let parent_label = parent
                    .title
                    .as_deref()
                    .unwrap_or("(unnamed parent)");
                return Err(format!(
                    "duplicate child title '{new_title}' under parent '{parent_label}' \
                     (conflicts with existing index {})",
                    conflict.id
                ));
            }
        }

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

        // Refuse to delete a knowledge-linked index — that would silently
        // orphan the targeted Knowledge row (still present in `knowledges`,
        // no longer surfaced anywhere in the tree). The caller must either
        // delete the Knowledge itself via `kms_delete_knowledge` (which
        // downgrades the mount), or explicitly detach via
        // `kms_detach_knowledge` if they want to remount it elsewhere.
        if idx.target_type == TargetType::Knowledge {
            return Err(format!(
                "index '{title}' is a knowledge mount (target_type=knowledge); \
                 refusing to delete it because that would leave the Knowledge \
                 orphaned in the database. Use `kms_delete_knowledge` to remove \
                 the Knowledge itself, or `kms_detach_knowledge` to temporarily \
                 unmount it (you must re-link it before the session ends)."
            ));
        }

        // Refuse to delete a non-empty group — the caller must explicitly
        // move or delete the children first. The old behaviour reparented
        // children to the grandparent, which silently moved them out of
        // the structure the caller had organised; that lost intent every
        // time it ran. Force the cleanup to be explicit instead.
        let children = self
            .storage
            .index
            .children_of(Some(idx.id))
            .await
            .map_err(|e| e.to_string())?;
        if !children.is_empty() {
            let preview: Vec<String> = children
                .iter()
                .take(5)
                .map(|c| {
                    c.title
                        .clone()
                        .unwrap_or_else(|| "(unnamed)".to_string())
                })
                .collect();
            let suffix = if children.len() > preview.len() {
                format!(", …({} more)", children.len() - preview.len())
            } else {
                String::new()
            };
            return Err(format!(
                "index '{title}' is not empty ({n} child(ren): {preview}{suffix}). \
                 Refusing to delete a non-empty index because reparenting children \
                 would silently lose your structure. Move them first with \
                 `kms_move_children` / `kms_move_index`, delete them with \
                 `kms_delete_index` / `kms_delete_knowledge`, then retry.",
                n = children.len(),
                preview = preview.join(", "),
            ));
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

    /// Detach a knowledge-linked Index node (the "mount point" through
    /// which a Knowledge is reachable in the tree), deleting only the
    /// Index row and leaving the Knowledge as an explicit orphan.
    ///
    /// **Callers MUST re-link the orphan** before the session ends via
    /// `kms_link_orphans` (or `kms_create_index` with
    /// `target_type=knowledge`) — otherwise the Knowledge sits in the
    /// `knowledges` table with no surface in the tree.
    ///
    /// Refuses if:
    ///  * no Index with that title exists;
    ///  * the Index is not a knowledge mount (`target_type != Knowledge`);
    ///  * the Index has children (shouldn't normally happen for a
    ///    knowledge mount, but we refuse defensively).
    pub async fn detach_knowledge_index(&self, title: &str) -> Result<Uuid, String> {
        let idx = self
            .storage
            .index
            .find_by_title(title)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("index '{}' not found", title))?;

        if idx.parent_id.is_none() {
            return Err("cannot detach the root index".into());
        }

        if idx.target_type != TargetType::Knowledge {
            return Err(format!(
                "index '{title}' is a Group, not a knowledge mount; \
                 nothing to detach. Use `kms_delete_index` instead."
            ));
        }

        let knowledge_id = idx.target.ok_or_else(|| {
            format!(
                "index '{title}' is marked as knowledge-typed but has no target; \
                 this is a data inconsistency, investigate before retrying."
            )
        })?;

        // Defensive: a knowledge mount is supposed to be a leaf, but
        // guard against the rare case where a caller hand-built a
        // malformed tree.
        let children = self
            .storage
            .index
            .children_of(Some(idx.id))
            .await
            .map_err(|e| e.to_string())?;
        if !children.is_empty() {
            return Err(format!(
                "index '{title}' has {} child(ren); a knowledge mount must \
                 be a leaf. Move or delete the children first.",
                children.len()
            ));
        }

        let parent_id = idx.parent_id;

        self.storage
            .index
            .delete(idx.id)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(pid) = parent_id {
            self.storage
                .index
                .reindex_positions(Some(pid))
                .await
                .map_err(|e| e.to_string())?;
        }

        Ok(knowledge_id)
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

    /// Resolve a path string to a node UUID, mirroring the rules of
    /// [`Self::navigate`] (absolute `/...`, relative with `/` separators,
    /// and `..` segments for parent ascent) but **without** mutating the
    /// session pointer. Use this when an operation needs an explicit
    /// target index and the caller should stay where it is.
    pub async fn resolve_path(&self, path: &str) -> Result<Uuid, String> {
        let current = self.get_pointer().await;
        let trimmed = path.trim();
        if trimmed.is_empty() || trimmed == "." {
            return Ok(current);
        }

        if trimmed == ".." {
            let node = self.get_index(current).await?;
            return node
                .parent_id
                .ok_or_else(|| "already at root, cannot go to parent".to_string());
        }

        let (base_id, segments) = if trimmed.starts_with('/') {
            let root = self
                .storage
                .index
                .find_root()
                .await
                .map_err(|e| e.to_string())?;
            (root.id, trimmed[1..].split('/').collect::<Vec<_>>())
        } else if trimmed.starts_with("../") {
            let node = self.get_index(current).await?;
            match node.parent_id {
                Some(pid) => (pid, trimmed[3..].split('/').collect::<Vec<_>>()),
                None => return Err("already at root, cannot go to parent".into()),
            }
        } else if trimmed.contains('/') {
            (current, trimmed.split('/').collect::<Vec<_>>())
        } else {
            (current, vec![trimmed])
        };

        let mut pointer = base_id;
        for seg in segments {
            let seg = seg.trim();
            if seg.is_empty() {
                continue;
            }
            if seg == ".." {
                let node = self.get_index(pointer).await?;
                pointer = node
                    .parent_id
                    .ok_or_else(|| "already at root, cannot go to parent".to_string())?;
            } else {
                let children = self.get_children(Some(pointer)).await?;
                match children.iter().find(|c| c.title.as_deref() == Some(seg)) {
                    Some(child) => pointer = child.id,
                    None => {
                        return Err(format!(
                            "segment '{}' not found as child of node",
                            seg
                        ))
                    }
                }
            }
        }

        Ok(pointer)
    }

    pub async fn navigate(&self, path: &str) -> Result<String, String> {
        let pointer = self.resolve_path(path).await?;
        self.set_pointer(pointer).await;
        self.render_location().await
    }

    /// Move the named child indices from `source_path` into a newly
    /// created group index mounted under `remount_path`. The new group
    /// is created as a `TargetType::Group` and inherits the requested
    /// title.
    ///
    /// `source_path` and `remount_path` are resolved through
    /// [`Self::resolve_path`], so the caller can use any path form
    /// supported by navigation (absolute `/...`, relative with `/`,
    /// `..` segments). This removes the previous restriction that the
    /// children had to live under the current pointer node — both the
    /// source of the children and the mount point of the new group are
    /// now explicit. After the move the session pointer jumps to the
    /// newly created group.
    /// Look up a direct child of `parent_id` by exact title. Returns
    /// `Some(child_id)` if a titled child matches, `None` otherwise.
    /// Untitled children (`title == None`) are never considered a match
    /// — they have no name to compare against.
    pub async fn find_child_by_title(
        &self,
        parent_id: Uuid,
        title: &str,
    ) -> Result<Option<Uuid>, String> {
        let children = self
            .storage
            .index
            .children_of(Some(parent_id))
            .await
            .map_err(|e| e.to_string())?;
        Ok(children
            .iter()
            .find(|c| c.title.as_deref() == Some(title))
            .map(|c| c.id))
    }

    /// Move the named child indices from `source_path` into a group
    /// index mounted under `remount_path`. The group is identified by
    /// `new_group_title`: if a `Group`-typed child with that title
    /// already exists under `remount_path` it is **reused** (making the
    /// call idempotent for an LLM re-running the same regroup step);
    /// otherwise a fresh `Group`-typed index is created. Refusing to
    /// reuse a non-Group child (e.g. a Knowledge-linker) protects
    /// against accidentally stealing a leaf that already carries a
    /// knowledge entry. The whole subtree under the moved children
    /// follows them into the new group.
    ///
    /// `source_path` and `remount_path` are resolved through
    /// [`Self::resolve_path`], so the caller can use any path form
    /// supported by navigation (absolute `/...`, relative with `/`,
    /// `..` segments). This removes the previous restriction that the
    /// children had to live under the current pointer node — both the
    /// source of the children and the mount point of the new group are
    /// now explicit. After the move the session pointer jumps to the
    /// group.
    pub async fn move_children(
        &self,
        source_path: &str,
        remount_path: &str,
        new_group_title: &str,
        child_titles: &[String],
    ) -> Result<MoveChildrenResult, String> {
        if child_titles.is_empty() {
            return Err("child_titles must not be empty".into());
        }

        let source_id = self
            .resolve_path(source_path)
            .await
            .map_err(|e| addressing_hint("source_path", source_path, &e))?;
        let remount_id = self
            .resolve_path(remount_path)
            .await
            .map_err(|e| addressing_hint("remount_path", remount_path, &e))?;

        let children = self.get_children(Some(source_id)).await?;

        let mut child_indices: Vec<Index> = Vec::new();
        for title in child_titles {
            let found = children
                .iter()
                .find(|c| c.title.as_deref() == Some(title.as_str()))
                .ok_or_else(|| {
                    let mut msg = format!(
                        "'{}' is not a direct child of '{}'",
                        title, source_path
                    );
                    if looks_like_bare_title(source_path) {
                        msg.push_str(&format!(
                            " — note: source_path='{src}' looks like a bare title, \
                             not an absolute path. Pass an absolute path like \
                             '/parent/{src}' instead; bare titles are resolved \
                             against the implicit pointer and often land on the \
                             wrong node when titles repeat.",
                            src = source_path
                        ));
                    } else {
                        msg.push_str(
                            " (verify source_path resolves to the parent you \
                             intended — call kms_local on it to confirm its \
                             direct children).",
                        );
                    }
                    msg
                })?;
            child_indices.push(found.clone());
        }

        // Find-or-create the destination group. Reusing an existing
        // group is the idempotent path: the caller (often an LLM
        // re-running a regroup plan) does not have to first check
        // whether a group with this title is already in place.
        let (new_group_id, group_created) = match self
            .find_child_by_title(remount_id, new_group_title)
            .await?
        {
            Some(existing_id) => {
                let existing = self
                    .storage
                    .index
                    .get(existing_id)
                    .await
                    .map_err(|e| e.to_string())?;
                if existing.target_type != TargetType::Group {
                    return Err(format!(
                        "cannot reuse '{}' under '{}' as the destination group: \
                         it is already a {:?}-typed index (id {existing_id})",
                        new_group_title, remount_path, existing.target_type
                    ));
                }
                (existing_id, false)
            }
            None => {
                let created = self
                    .create_index(
                        remount_id,
                        Some(new_group_title.to_string()),
                        None,
                        Some(TargetType::Group),
                    )
                    .await?;
                (created.id, true)
            }
        };

        for (i, child) in child_indices.iter().enumerate() {
            self.storage
                .index
                .reparent(child.id, new_group_id, i as i64)
                .await
                .map_err(|e| e.to_string())?;
        }

        // Reindex the source only if the children actually came from a
        // different subtree than the new group's mount point — reindexing
        // the same node twice is harmless but skip the redundant work.
        if source_id != remount_id {
            self.storage
                .index
                .reindex_positions(Some(source_id))
                .await
                .map_err(|e| e.to_string())?;
        }

        self.storage
            .index
            .reindex_positions(Some(remount_id))
            .await
            .map_err(|e| e.to_string())?;

        self.storage
            .index
            .reindex_positions(Some(new_group_id))
            .await
            .map_err(|e| e.to_string())?;

        self.set_pointer(new_group_id).await;

        let location = self.render_location().await?;
        Ok(MoveChildrenResult {
            location,
            new_group_id,
            group_created,
        })
    }

    /// Move the index node resolved from `index_path` (together with its
    /// entire subtree) under the index node resolved from
    /// `new_parent_path`. Both arguments use the same path grammar as
    /// [`Self::navigate`] / [`Self::resolve_path`] — absolute `/...`,
    /// relative with `/`, or `..` segments — so the call site can
    /// disambiguate between same-titled nodes that live under different
    /// parents. The root index cannot be moved, descendants of the moved
    /// node are rejected as a new parent (cycle), and a no-op move
    /// (target equals current parent) returns an explicit error.
    pub async fn move_index(
        &self,
        index_path: &str,
        new_parent_path: &str,
    ) -> Result<String, String> {
        let idx_id = self
            .resolve_path(index_path)
            .await
            .map_err(|e| addressing_hint("index_path", index_path, &e))?;
        let new_parent_id = self
            .resolve_path(new_parent_path)
            .await
            .map_err(|e| addressing_hint("new_parent_path", new_parent_path, &e))?;

        let idx = self
            .storage
            .index
            .get(idx_id)
            .await
            .map_err(|e| e.to_string())?;

        if idx.parent_id.is_none() {
            return Err("cannot move the root index".into());
        }

        if idx.parent_id == Some(new_parent_id) {
            return Err(format!(
                "index at '{}' is already under '{}'",
                index_path, new_parent_path
            ));
        }

        let old_parent_id = idx.parent_id;

        let target_children = self
            .storage
            .index
            .children_of(Some(new_parent_id))
            .await
            .map_err(|e| e.to_string())?;
        let new_position = target_children.len() as i64;

        self.storage
            .index
            .reparent(idx_id, new_parent_id, new_position)
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
            .reindex_positions(Some(new_parent_id))
            .await
            .map_err(|e| e.to_string())?;

        // Reposition the agent on the moved node so the rendered
        // location is meaningful even when the caller never touched
        // the pointer (e.g. when running a path-only workflow). If the
        // agent was already sitting on a node inside the moved subtree
        // they now see the same subtree under its new ancestor.
        self.set_pointer(idx_id).await;
        let location = self.render_location().await?;

        let new_parent_label = self
            .storage
            .index
            .get(new_parent_id)
            .await
            .map(|n| n.title.unwrap_or_else(|| new_parent_id.to_string()))
            .unwrap_or_else(|_| new_parent_id.to_string());
        Ok(format!(
            "moved '{}' under '{}'\n{}",
            index_path, new_parent_label, location
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
            corpus: self.corpus.clone(),
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
    //  Knowledge (cross-corpus provenance)
    // -----------------------------------------------------------------

    /// Like [`create_knowledge`](Self::create_knowledge) but accepts an
    /// optional `(source_document_id, source_chunk_idx)` pair for
    /// provenance tracking. The `source_document_id` is validated
    /// against the corpus service.
    pub async fn create_knowledge_with_source(
        &self,
        title: &str,
        knowledge_type: KnowledgeType,
        entities: Vec<Uuid>,
        content: Option<String>,
        source: Option<(Uuid, usize)>,
    ) -> Result<Knowledge, String> {
        let (source_document_id, source_chunk_idx) = match source {
            Some((doc_id, chunk_idx)) => {
                if !self.corpus.document_exists(doc_id).await {
                    return Err(format!(
                        "source_document_id {doc_id} not found in corpus"
                    ));
                }
                (Some(doc_id), Some(chunk_idx as i64))
            }
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

/// Heuristic: does this string look like a bare title rather than a
/// path? Used to surface a "did you mean an absolute path?" hint when
/// the agent passes a single segment (no `/`, no `..`) to a `_path`
/// parameter. False positives are cheap (the hint is informational);
/// false negatives would silently swallow the misuse.
fn looks_like_bare_title(p: &str) -> bool {
    let t = p.trim();
    !t.is_empty()
        && !t.starts_with('/')
        && !t.contains('/')
        && t != ".."
        && t != "."
}

/// Wrap a `resolve_path` error from a path-typed parameter with a
/// pointer-vs-path hint when the value looks suspicious. Leaves clearly
/// path-shaped inputs alone so we don't add noise to genuine errors.
fn addressing_hint(param_name: &str, value: &str, err: &str) -> String {
    if looks_like_bare_title(value) {
        format!(
            "{err}\n\nhint: `{param}` expects an ABSOLUTE PATH (starting with `/`), not a bare title. \
             '{value}' was resolved against the implicit pointer and not found there. \
             Use an absolute path like '/parent/{value}', or call `kms_local` / \
             `kms_search_subtree('/', '{value}')` first to discover the correct full path.",
            err = err,
            param = param_name,
            value = value,
        )
    } else {
        format!("{err} (parameter `{param_name}` was {value:?})")
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
        // Use a per-test shared in-memory database so both services
        // see the same `documents` table. The random UUID suffix
        // isolates concurrent test cases from each other.
        let db_path = format!(
            "file:test-{}-{}?mode=memory&cache=shared",
            std::process::id(),
            Uuid::new_v4()
        );
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect(&db_path)
            .await
            .unwrap();
        // Run KMS migrations first (creates `knowledges`, `indexes`,
        // …). Then run corpus migrations (creates `documents`,
        // `document_chunks`) on the same shared DB. We bypass
        // `CorpusService::open`'s own migration step so the kms test
        // pool owns the migration ledger; we then build a corpus
        // service that shares the pool without re-running its
        // migrations.
        sqlx::migrate!("migrations/sqlite")
            .run(&pool)
            .await
            .unwrap();
        // The `__sqlite_backend` hook is a doc-hidden module that
        // exposes the SQLite repo + migration runner for tests that
        // share a pool. Production code goes through
        // `CorpusService::open(Backend::Sqlite { ... })`.
        corpus::__sqlite_backend::run_migrations_on_pool(&pool)
            .await
            .unwrap();
        let corpus = corpus::CorpusService::from_repo(Arc::new(
            corpus::__sqlite_backend::SqliteDocumentRepo::new(pool.clone()),
        ));
        KmsService {
            inner: Arc::new(Inner {
                pointer: RwLock::new(Uuid::nil()),
            }),
            storage: Storage::from_pool(pool),
            corpus,
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

    #[tokio::test]
    async fn test_move_children_creates_group_when_missing() {
        let svc = setup_service().await;
        let root = svc.create_index_root("root").await.unwrap();
        let a = svc
            .create_index(root.id, Some("a".into()), None, None)
            .await
            .unwrap();
        let b = svc
            .create_index(root.id, Some("b".into()), None, None)
            .await
            .unwrap();
        let _c = svc
            .create_index(root.id, Some("c".into()), None, None)
            .await
            .unwrap();

        let result = svc
            .move_children("/", "/", "新分组", &["a".into(), "b".into()])
            .await
            .expect("first call should create the group");
        assert!(result.group_created, "first call must create the group");
        assert!(result.location.contains("新分组"));

        // a, b should now sit under 新分组; c stays at root.
        let group = svc.get_index(result.new_group_id).await.unwrap();
        assert_eq!(group.parent_id, Some(root.id));
        assert_eq!(group.target_type, TargetType::Group);
        let group_kids = svc.get_children(Some(result.new_group_id)).await.unwrap();
        let titles: Vec<&str> = group_kids.iter().filter_map(|c| c.title.as_deref()).collect();
        assert_eq!(group_kids.len(), 2);
        assert!(titles.contains(&"a"));
        assert!(titles.contains(&"b"));
        assert!(titles.iter().all(|t| *t != "c"));

        // The moved children keep their original ids so the call is
        // transparent to anyone holding references to them.
        let group_kid_ids: Vec<Uuid> = group_kids.iter().map(|c| c.id).collect();
        assert!(group_kid_ids.contains(&a.id));
        assert!(group_kid_ids.contains(&b.id));
    }

    #[tokio::test]
    async fn test_move_children_reuses_existing_group() {
        let svc = setup_service().await;
        let root = svc.create_index_root("root").await.unwrap();
        svc.create_index(root.id, Some("a".into()), None, None)
            .await
            .unwrap();
        svc.create_index(root.id, Some("b".into()), None, None)
            .await
            .unwrap();
        let _c = svc
            .create_index(root.id, Some("c".into()), None, None)
            .await
            .unwrap();
        // Pre-create the destination group so the second move reuses it.
        let existing_group = svc
            .create_index(root.id, Some("新分组".into()), None, Some(TargetType::Group))
            .await
            .unwrap();
        // Seed the group with a child so the reused group doesn't end
        // up empty after the call.
        svc.create_index(existing_group.id, Some("seed".into()), None, None)
            .await
            .unwrap();

        // Move "a" and "b" into the pre-existing group.
        let result = svc
            .move_children("/", "/", "新分组", &["a".into(), "b".into()])
            .await
            .expect("second call should reuse the existing group");
        assert!(!result.group_created, "second call must NOT create a new group");
        assert_eq!(result.new_group_id, existing_group.id, "must reuse the same id");

        // The group now contains seed, a, b.
        let kids = svc.get_children(Some(existing_group.id)).await.unwrap();
        let titles: Vec<&str> = kids.iter().filter_map(|c| c.title.as_deref()).collect();
        assert_eq!(kids.len(), 3);
        assert!(titles.contains(&"seed"));
        assert!(titles.contains(&"a"));
        assert!(titles.contains(&"b"));
        // c stays at root.
        let root_kids = svc.get_children(Some(root.id)).await.unwrap();
        let root_titles: Vec<&str> = root_kids.iter().filter_map(|c| c.title.as_deref()).collect();
        assert_eq!(root_titles, vec!["c", "新分组"]);

        // Now the source no longer carries a/b, so the call is
        // trivially idempotent: the same group is reported back, and
        // the no-op move (no children to relocate) does not change
        // the tree.
        let second = svc
            .move_children("/", "/", "新分组", &[])
            .await
            .expect_err("empty child_titles should still be rejected");
        assert!(second.contains("must not be empty"));

        // The group still holds a, b, seed — no new copies, no failure.
        let kids2 = svc.get_children(Some(existing_group.id)).await.unwrap();
        assert_eq!(kids2.len(), 3);
    }

    #[tokio::test]
    async fn test_move_children_rejects_non_group_reuse() {
        let svc = setup_service().await;
        let root = svc.create_index_root("root").await.unwrap();
        // Create a Knowledge-linker under root that already occupies
        // the title the move wants to reuse.
        let knowledge = svc
            .create_knowledge("TestEntity · 病因", KnowledgeType::Aspect, vec![], None)
            .await
            .unwrap();
        svc.create_index(
            root.id,
            Some("新分组".into()),
            Some(knowledge.id),
            Some(TargetType::Knowledge),
        )
        .await
        .unwrap();
        svc.create_index(root.id, Some("a".into()), None, None)
            .await
            .unwrap();

        let err = svc
            .move_children("/", "/", "新分组", &["a".into()])
            .await
            .expect_err("should refuse to reuse a Knowledge-linker as a group");
        assert!(
            err.contains("Knowledge") || err.contains("新分组"),
            "error should explain the type conflict: {err}"
        );
    }

    #[tokio::test]
    async fn test_create_index_rejects_duplicate_title() {
        let svc = setup_service().await;
        let root = svc.create_index_root("根").await.unwrap();
        let first = svc
            .create_index(root.id, Some("重复名".into()), None, None)
            .await
            .unwrap();

        // Same title under the same parent must be rejected.
        let err = svc
            .create_index(root.id, Some("重复名".into()), None, None)
            .await
            .expect_err("duplicate title should be rejected");
        assert!(err.contains("重复名"), "error should name the duplicate: {err}");
        assert!(
            err.contains(&first.id.to_string()),
            "error should reference the conflicting index id: {err}"
        );

        // The same title under a different parent is allowed.
        let sibling = svc
            .create_index(root.id, Some("兄弟".into()), None, None)
            .await
            .unwrap();
        let allowed = svc
            .create_index(sibling.id, Some("重复名".into()), None, None)
            .await
            .expect("same title under a different parent should be allowed");
        assert_eq!(allowed.parent_id, Some(sibling.id));
        assert_ne!(allowed.id, first.id);

        // Untitled children are not subject to the uniqueness rule.
        let untitled_a = svc
            .create_index(root.id, None, None, None)
            .await
            .unwrap();
        let untitled_b = svc
            .create_index(root.id, None, None, None)
            .await
            .unwrap();
        assert_ne!(untitled_a.id, untitled_b.id);
    }

    #[tokio::test]
    async fn test_move_index_uses_paths() {
        // Build a tree (the create_index_root node is the actual root,
        // its title is irrelevant for path resolution — `/` points at it
        // directly):
        //   /
        //   ├── A
        //   │   └── A1
        //   └── B
        let svc = setup_service().await;
        let root = svc.create_index_root("root").await.unwrap();
        let a = svc
            .create_index(root.id, Some("A".into()), None, None)
            .await
            .unwrap();
        let _a1 = svc
            .create_index(a.id, Some("A1".into()), None, None)
            .await
            .unwrap();
        let b = svc
            .create_index(root.id, Some("B".into()), None, None)
            .await
            .unwrap();

        // Move /A under /B using absolute paths.
        let result = svc
            .move_index("/A", "/B")
            .await
            .expect("absolute-path move should succeed");
        // Result echoes the source path and the destination parent's
        // title (not the full path).
        assert!(result.contains("/A"), "result should echo source path: {result}");
        assert!(result.contains("under 'B'"), "result should echo destination title: {result}");

        // The subtree must follow: A1 is now a grandchild of B.
        let b_children = svc.get_children(Some(b.id)).await.unwrap();
        assert_eq!(b_children.len(), 1);
        assert_eq!(b_children[0].id, a.id);
        let a_children = svc.get_children(Some(a.id)).await.unwrap();
        assert_eq!(a_children.len(), 1);
        assert_eq!(a_children[0].id, _a1.id);

        // No-op move (already under the requested parent) is rejected.
        let err = svc
            .move_index("/B/A", "/B")
            .await
            .expect_err("no-op move should be rejected");
        assert!(
            err.contains("already under"),
            "error should explain the no-op: {err}"
        );

        // Cycle prevention: cannot move a node under one of its descendants.
        let err = svc
            .move_index("/B", "/B/A")
            .await
            .expect_err("move under a descendant should be rejected");
        assert!(
            err.contains("descendant") || err.contains("reparent"),
            "error should mention the cycle: {err}"
        );

        // Root cannot be moved. The root here is the node created by
        // `create_index_root` — its title is "root", so `/root` resolves
        // to the actual root index.
        let err = svc
            .move_index("/root", "/B")
            .await
            .expect_err("moving the root should be rejected");
        assert!(
            err.contains("root"),
            "error should mention the root constraint: {err}"
        );

        // Relative path resolution: sit on A1 and use `..` to reach A.
        svc.set_pointer(_a1.id).await;
        let c = svc
            .create_index(root.id, Some("C".into()), None, None)
            .await
            .unwrap();
        let result = svc
            .move_index("..", "/C")
            .await
            .expect("relative `..` from A1 should resolve to A");
        assert!(
            result.contains(".."),
            "result should echo the relative source: {result}"
        );
        assert!(
            result.contains("under 'C'"),
            "result should echo the destination title: {result}"
        );
        // A (with A1) is now under C.
        let c_kids = svc.get_children(Some(c.id)).await.unwrap();
        assert_eq!(c_kids.len(), 1);
        assert_eq!(c_kids[0].id, a.id);
    }

    #[tokio::test]
    async fn test_move_index_disambiguates_same_titled_siblings() {
        // Two leaves with the same title living under different parents
        // must be moved independently by their full path. Title-based
        // lookup would silently pick the wrong one.
        let svc = setup_service().await;
        let root = svc.create_index_root("root").await.unwrap();
        let p1 = svc
            .create_index(root.id, Some("P1".into()), None, None)
            .await
            .unwrap();
        let p2 = svc
            .create_index(root.id, Some("P2".into()), None, None)
            .await
            .unwrap();
        let dup1 = svc
            .create_index(p1.id, Some("dup".into()), None, None)
            .await
            .unwrap();
        let dup2 = svc
            .create_index(p2.id, Some("dup".into()), None, None)
            .await
            .unwrap();
        let new_home = svc
            .create_index(root.id, Some("home".into()), None, None)
            .await
            .unwrap();

        svc.move_index("/P1/dup", "/home")
            .await
            .expect("path-based move should reach the right duplicate");
        // dup1 should now sit under home; dup2 should still sit under p2.
        let home_children = svc.get_children(Some(new_home.id)).await.unwrap();
        assert_eq!(home_children.len(), 1);
        assert_eq!(home_children[0].id, dup1.id);
        let p2_children = svc.get_children(Some(p2.id)).await.unwrap();
        assert_eq!(p2_children.len(), 1);
        assert_eq!(p2_children[0].id, dup2.id);
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

    // ─── delete_index hardening ──────────────────────────────────────

    /// `delete_index` MUST refuse when the target is a knowledge mount
    /// (the old behaviour silently orphaned the underlying Knowledge).
    #[tokio::test]
    async fn delete_index_refuses_knowledge_mount() {
        let svc = setup_service().await;
        let root = svc.create_index_root("root").await.unwrap();

        let (e, _) = svc
            .create_entity(vec![make_name("心脏")], "器官")
            .await
            .unwrap();
        let k = svc
            .create_knowledge("心脏病", KnowledgeType::Aspect, vec![e.id], None)
            .await
            .unwrap();
        let _idx = svc
            .create_index(
                root.id,
                Some("心脏病".into()),
                Some(k.id),
                Some(TargetType::Knowledge),
            )
            .await
            .unwrap();

        let err = svc.delete_index("心脏病").await.unwrap_err();
        assert!(
            err.contains("knowledge mount") || err.contains("target_type=knowledge"),
            "error should explain why a knowledge mount cannot be deleted via delete_index: {err}"
        );
        assert!(
            err.contains("kms_delete_knowledge") && err.contains("kms_detach_knowledge"),
            "error should redirect the caller to the right tool: {err}"
        );

        // Both rows must still exist after the refusal.
        assert!(
            svc.storage
                .index
                .find_by_title("心脏病")
                .await
                .unwrap()
                .is_some(),
            "index must survive a refused delete"
        );
        assert!(
            svc.get_knowledge(k.id).await.is_ok(),
            "knowledge must survive a refused delete"
        );
    }

    /// `delete_index` MUST refuse when the target has children
    /// (the old behaviour silently reparented them to the grandparent).
    #[tokio::test]
    async fn delete_index_refuses_non_empty_group() {
        let svc = setup_service().await;
        let root = svc.create_index_root("root").await.unwrap();
        let parent = svc
            .create_index(root.id, Some("循环系统".into()), None, Some(TargetType::Group))
            .await
            .unwrap();
        let _c1 = svc
            .create_index(parent.id, Some("心".into()), None, Some(TargetType::Group))
            .await
            .unwrap();
        let _c2 = svc
            .create_index(parent.id, Some("血管".into()), None, Some(TargetType::Group))
            .await
            .unwrap();

        let err = svc.delete_index("循环系统").await.unwrap_err();
        assert!(
            err.contains("not empty"),
            "error should explain that the group is non-empty: {err}"
        );
        assert!(
            err.contains("心") || err.contains("血管"),
            "error should preview the surviving children so the caller knows what to clean up: {err}"
        );

        // All four index rows must still exist after the refusal.
        for title in ["循环系统", "心", "血管"] {
            assert!(
                svc.storage
                    .index
                    .find_by_title(title)
                    .await
                    .unwrap()
                    .is_some(),
                "index '{title}' must survive a refused delete"
            );
        }
    }

    /// After cleaning up the children, `delete_index` on the (now empty)
    /// group must succeed — the refusal is a guard, not a permanent
    /// lockout.
    #[tokio::test]
    async fn delete_index_works_on_empty_group_after_cleanup() {
        let svc = setup_service().await;
        let root = svc.create_index_root("root").await.unwrap();
        let parent = svc
            .create_index(root.id, Some("临时组".into()), None, Some(TargetType::Group))
            .await
            .unwrap();
        let _c1 = svc
            .create_index(parent.id, Some("子".into()), None, Some(TargetType::Group))
            .await
            .unwrap();

        // Delete the child first, then the parent. Both calls must
        // succeed because the parent is empty by the time we ask.
        svc.delete_index("子").await.unwrap();
        svc.delete_index("临时组").await.unwrap();

        assert!(
            svc.storage
                .index
                .find_by_title("临时组")
                .await
                .unwrap()
                .is_none(),
            "empty group should delete cleanly after cleanup"
        );
    }

    // ─── detach_knowledge_index ──────────────────────────────────────

    /// `detach_knowledge_index` removes the mount and leaves the
    /// Knowledge row as an orphan. Round-tripping through
    /// `link_orphans` restores the mount.
    #[tokio::test]
    async fn detach_knowledge_index_round_trip() {
        let svc = setup_service().await;
        let root = svc.create_index_root("root").await.unwrap();
        let bucket = svc
            .create_index(root.id, Some("挂载点".into()), None, Some(TargetType::Group))
            .await
            .unwrap();
        let (e, _) = svc
            .create_entity(vec![make_name("胃")], "器官")
            .await
            .unwrap();
        let k = svc
            .create_knowledge("胃炎", KnowledgeType::Aspect, vec![e.id], None)
            .await
            .unwrap();
        let idx = svc
            .create_index(
                bucket.id,
                Some("胃炎".into()),
                Some(k.id),
                Some(TargetType::Knowledge),
            )
            .await
            .unwrap();

        // Detach: the index row is gone, the knowledge survives, and
        // the returned id matches the knowledge we just orphaned.
        let orphan = svc.detach_knowledge_index("胃炎").await.unwrap();
        assert_eq!(orphan, k.id);
        assert!(
            svc.storage
                .index
                .find_by_title("胃炎")
                .await
                .unwrap()
                .is_none(),
            "index row should be gone after detach"
        );
        assert!(
            svc.get_knowledge(k.id).await.is_ok(),
            "knowledge row should still exist as orphan"
        );

        // Re-link: the orphan returns to the tree under a (possibly
        // different) parent. The new index points at the same knowledge.
        let _ = svc.link_orphans("挂载点", &["胃炎"]).await.unwrap();
        let remounted = svc
            .storage
            .index
            .find_by_title("胃炎")
            .await
            .unwrap()
            .expect("knowledge must be remountable via link_orphans");
        assert_eq!(remounted.target, Some(k.id));
        assert_eq!(remounted.target_type, TargetType::Knowledge);
        // The remount should be a *new* index id (the original was deleted).
        assert_ne!(remounted.id, idx.id);
    }

    /// `detach_knowledge_index` MUST refuse when invoked on a Group —
    /// detach is strictly for knowledge mounts.
    #[tokio::test]
    async fn detach_knowledge_index_refuses_group() {
        let svc = setup_service().await;
        let root = svc.create_index_root("root").await.unwrap();
        let _grp = svc
            .create_index(root.id, Some("组".into()), None, Some(TargetType::Group))
            .await
            .unwrap();

        let err = svc.detach_knowledge_index("组").await.unwrap_err();
        assert!(
            err.contains("Group") && err.contains("kms_delete_index"),
            "error should redirect group deletion to kms_delete_index: {err}"
        );
    }

    // ─── path-vs-title addressing hints for move tools ───────────────

    /// When `move_children` receives a bare title in `source_path`,
    /// the error MUST steer the caller toward an absolute path.
    #[tokio::test]
    async fn move_children_hints_when_source_path_is_bare_title() {
        let svc = setup_service().await;
        let root = svc.create_index_root("root").await.unwrap();
        let parent = svc
            .create_index(root.id, Some("循环系统".into()), None, Some(TargetType::Group))
            .await
            .unwrap();
        let _c1 = svc
            .create_index(parent.id, Some("c1".into()), None, Some(TargetType::Group))
            .await
            .unwrap();

        // Bare title — looks like a title to the LLM, but to resolve_path
        // it's a relative segment under the implicit pointer (which is
        // root here; no child named "循环系统" exists at root in this
        // shape — yes it does actually). The hint must fire whether or
        // not resolution accidentally succeeds: use a title that does
        // NOT resolve so we get the error path.
        let err = svc
            .move_children("不存在的标题", "/", "x", &["whatever".to_string()])
            .await
            .unwrap_err();
        assert!(
            err.contains("ABSOLUTE PATH"),
            "error should mention ABSOLUTE PATH: {err}"
        );
        assert!(
            err.contains("`source_path`"),
            "error should name the parameter: {err}"
        );
        assert!(
            err.contains("kms_local") || err.contains("kms_search_subtree"),
            "error should point to discovery tools: {err}"
        );
    }

    /// When `move_children` finds the source path but a `child_titles`
    /// entry isn't a direct child AND `source_path` looks like a bare
    /// title, the missing-child error includes the path-vs-title hint.
    #[tokio::test]
    async fn move_children_child_not_found_includes_hint_when_source_looks_like_title() {
        let svc = setup_service().await;
        let root = svc.create_index_root("root").await.unwrap();
        // Put a node named "循环系统" directly under root so the bare
        // title "循环系统" *resolves* (via the implicit pointer which
        // starts at root) — this is the silent-misaddressing path.
        let parent = svc
            .create_index(root.id, Some("循环系统".into()), None, Some(TargetType::Group))
            .await
            .unwrap();
        let _real_child = svc
            .create_index(parent.id, Some("心律失常".into()), None, Some(TargetType::Group))
            .await
            .unwrap();

        // Ask to move a child that does not exist — the failure must
        // call out the bare-title shape.
        let err = svc
            .move_children("循环系统", "/", "x", &["不存在".to_string()])
            .await
            .unwrap_err();
        assert!(
            err.contains("bare title"),
            "error should flag the bare-title shape of source_path: {err}"
        );
        assert!(
            err.contains("absolute path") || err.contains("/parent/"),
            "error should suggest an absolute path form: {err}"
        );
    }

    /// `move_index` mirrors `move_children`: a bare title in a `_path`
    /// parameter triggers the hint.
    #[tokio::test]
    async fn move_index_hints_when_path_is_bare_title() {
        let svc = setup_service().await;
        let root = svc.create_index_root("root").await.unwrap();
        let _a = svc
            .create_index(root.id, Some("A".into()), None, Some(TargetType::Group))
            .await
            .unwrap();

        let err = svc
            .move_index("不存在", "/")
            .await
            .unwrap_err();
        assert!(
            err.contains("ABSOLUTE PATH") && err.contains("`index_path`"),
            "error should hint that index_path needs an absolute path: {err}"
        );
    }

    /// Absolute paths must NOT receive the bare-title hint — the hint
    /// is only for the suspicious shape, not for genuine errors.
    #[tokio::test]
    async fn move_index_absolute_path_error_has_no_bare_title_hint() {
        let svc = setup_service().await;
        let _root = svc.create_index_root("root").await.unwrap();

        let err = svc
            .move_index("/不存在", "/")
            .await
            .unwrap_err();
        assert!(
            !err.contains("ABSOLUTE PATH"),
            "absolute-path errors must not be polluted with the bare-title hint: {err}"
        );
    }
}

impl KmsService {
    /// Test-only helper: set the global pointer to a known id.
    #[cfg(test)]
    pub(crate) async fn set_pointer_for_test(&self, id: Uuid) {
        *self.inner.pointer.write().await = id;
    }
}
