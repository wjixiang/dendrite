use std::sync::Arc;
use serde_json::Value;
use uuid::Uuid;

use types::tools::{ToolBuilder, ToolResult};

/// Flatten nested markdown headings (##, ###, etc.) to bold-prefixed plain text
/// to prevent `internal_nested` diagnostic warnings.
/// e.g. "## 心脏结构" → "**心脏结构**"
fn flatten_nested_headings(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    for line in content.lines() {
        let trimmed = line.trim_start();
        let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
        if hash_count >= 2 {
            let text = trimmed[hash_count..].trim();
            result.push_str("**");
            result.push_str(text);
            result.push_str("**\n");
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }
    // Preserve original trailing newline behavior
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    result
}

pub fn registrations(svc: Arc<kms::KmsService>) -> Vec<crate::toolset::ToolRegistration> {
    vec![
        create_entity(svc.clone()),
        update_entity(svc.clone()),
        list_entities(svc.clone()),
        get_entity(svc.clone()),
        search_entity(svc.clone()),
        delete_entity(svc.clone()),
        add_nomenclature(svc.clone()),
        update_nomenclature(svc.clone()),
        delete_nomenclature(svc.clone()),
        get_entity_knowledge(svc.clone()),
        create_knowledge(svc.clone()),
        get_knowledge(svc.clone()),
        create_index(svc.clone()),
        navigate_index(svc.clone()),
        reorganize_children(svc.clone()),
        move_index(svc.clone()),
        link_orphans(svc.clone()),
        update_knowledge(svc.clone()),
        rename_knowledge(svc.clone()),
        delete_knowledge(svc.clone()),
        delete_index(svc),
    ]
}

fn create_entity(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_create_entity",
        "Create a new entity in the knowledge graph. Each (lang, full) combination must be unique — do NOT send duplicate names within the same call (e.g. two entries with the same lang and full). Duplicates will be silently removed.",
    )
    .parameter("names", "array", "Array of nomenclatures: [{lang: 'ZH'|'EN', full: string, abbr?: string}]. Each (lang, full) pair must be unique.")
    .parameter("definition", "string", "Brief definition of the entity")
    .required("names")
    .required("definition")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let definition = input["definition"].as_str().ok_or("missing 'definition'")?;
                if definition.is_empty() {
                    return Err("'definition' must not be empty".into());
                }
                let names_arr = input["names"].as_array().ok_or("missing 'names'")?;
                if names_arr.is_empty() {
                    return Err("'names' must not be empty".into());
                }

                let mut nomenclatures = Vec::with_capacity(names_arr.len());
                for name_val in names_arr {
                    let lang = name_val["lang"].as_str().unwrap_or("ZH");
                    let full = name_val["full"].as_str().ok_or("missing 'full' in nomenclature")?;
                    let abbr = name_val["abbr"].as_str().map(|s| s.to_string());
                    nomenclatures.push(kms::Nomenclature {
                        id: Uuid::new_v4(),
                        lang: match lang {
                            "EN" => kms::Language::EN,
                            _ => kms::Language::ZH,
                        },
                        full: full.to_string(),
                        abbr,
                    });
                }

                let (entity, existed) = svc.create_entity(nomenclatures, definition).await?;

                Ok(ToolResult::success_json(
                    "create_entity",
                    serde_json::json!({
                        "name": entity.name.first().map(|n| n.full.as_str()).unwrap_or(""),
                        "definition": entity.definition,
                        "existed": existed
                    }),
                ))
            })
        })),
        vec![],
    )
}

fn update_entity(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_update_entity",
        "Update an entity's definition and/or nomenclatures. Use name_ref or id to locate the entity.",
    )
    .parameter("name_ref", "string", "Current nomenclature full name of the entity to update (use id if entity has no nomenclature)")
    .parameter("id", "string", "UUID of the entity to update (use when entity has no nomenclature)")
    .parameter("definition", "string", "New definition for the entity")
    .parameter("names", "array", "New nomenclature array: [{lang: 'ZH'|'EN', full: string, abbr?: string}]")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let name_ref = input["name_ref"].as_str();
                let id_str = input["id"].as_str();

                let entity = if let Some(id_str) = id_str {
                    let id = Uuid::parse_str(id_str).map_err(|_| "invalid 'id' UUID")?;
                    let definition = input["definition"].as_str();
                    let names = parse_names(&input["names"])?;
                    svc.update_entity_by_id(id, definition, names).await?
                } else {
                    let name_ref = name_ref.ok_or("missing 'name_ref' or 'id'")?;
                    let definition = input["definition"].as_str();
                    let names = parse_names(&input["names"])?;
                    svc.update_entity_by_ref(name_ref, definition, names).await?
                };

                let names_json: Vec<Value> = entity
                    .name
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "lang": format!("{:?}", n.lang),
                            "full": n.full,
                            "abbr": n.abbr
                        })
                    })
                    .collect();

                Ok(ToolResult::success_json(
                    "update_entity",
                    serde_json::json!({
                        "id": entity.id.to_string(),
                        "name": entity.name.first().map(|n| n.full.as_str()).unwrap_or(""),
                        "definition": entity.definition,
                        "names": names_json,
                    }),
                ))
            })
        })),
        vec![],
    )
}

fn parse_names(val: &Value) -> Result<Option<Vec<kms::Nomenclature>>, Box<dyn std::error::Error + Send + Sync>> {
    if !val.is_array() {
        return Ok(None);
    }
    let names_arr = val.as_array().unwrap();
    let mut nomenclatures = Vec::with_capacity(names_arr.len());
    for name_val in names_arr {
        let lang = name_val["lang"].as_str().unwrap_or("ZH");
        let full = name_val["full"].as_str().ok_or("missing 'full' in nomenclature")?;
        let abbr = name_val["abbr"].as_str().map(|s| s.to_string());
        nomenclatures.push(kms::Nomenclature {
            id: Uuid::new_v4(),
            lang: match lang {
                "EN" => kms::Language::EN,
                _ => kms::Language::ZH,
            },
            full: full.to_string(),
            abbr,
        });
    }
    Ok(Some(nomenclatures))
}

fn list_entities(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_list_entities",
        "List entities, optionally filtered by condition. Used to find entities with empty definitions or no nomenclatures.",
    )
    .parameter("filter", "string", "Filter condition: 'empty_definition', 'no_nomenclature', or 'all' (default: 'all')")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let filter = match input["filter"].as_str() {
                    Some("empty_definition") => kms::EntityFilter::EmptyDefinition,
                    Some("no_nomenclature") => kms::EntityFilter::NoNomenclature,
                    _ => kms::EntityFilter::All,
                };

                let entities = svc.list_entities(filter).await?;

                let results: Vec<Value> = entities
                    .into_iter()
                    .map(|e| {
                        let names: Vec<Value> = e.name.iter().map(|n| {
                            serde_json::json!({
                                "id": n.id.to_string(),
                                "lang": format!("{:?}", n.lang),
                                "full": n.full,
                                "abbr": n.abbr
                            })
                        }).collect();
                        serde_json::json!({
                            "id": e.id.to_string(),
                            "names": names,
                            "definition": e.definition,
                        })
                    })
                    .collect();

                Ok(ToolResult::success_json(
                    "list_entities",
                    serde_json::json!({
                        "count": results.len(),
                        "entities": results,
                    }),
                ))
            })
        })),
        vec![],
    )
}

fn delete_entity(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_delete_entity",
        "Delete an entity and all its nomenclatures by UUID. Use kms_list_entities to find the ID of orphan or duplicate entities.",
    )
    .parameter("id", "string", "UUID of the entity to delete")
    .required("id")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let id_str = input["id"].as_str().ok_or("missing 'id'")?;
                let id = Uuid::parse_str(id_str).map_err(|_| "invalid 'id' UUID")?;
                svc.delete_entity(id).await?;
                Ok(ToolResult::success_json(
                    "delete_entity",
                    serde_json::json!({ "deleted": id_str }),
                ))
            })
        })),
        vec![],
    )
}

fn add_nomenclature(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_add_nomenclature",
        "Add a new nomenclature (name variant) to an existing entity. Use this when an entity needs an additional name in another language, an abbreviation, or an alias.",
    )
    .parameter("entity_id", "string", "UUID of the entity")
    .parameter("lang", "string", "Language of the nomenclature: 'ZH' or 'EN'")
    .parameter("full", "string", "Full name")
    .parameter("abbr", "string", "Abbreviation (optional)")
    .required("entity_id")
    .required("lang")
    .required("full")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let id_str = input["entity_id"].as_str().ok_or("missing 'entity_id'")?;
                let id = Uuid::parse_str(id_str).map_err(|_| "invalid 'entity_id' UUID")?;
                let lang = input["lang"].as_str().ok_or("missing 'lang'")?;
                let full = input["full"].as_str().ok_or("missing 'full'")?;
                let abbr = input["abbr"].as_str().map(|s| s.to_string());
                let lang = match lang {
                    "EN" => kms::Language::EN,
                    _ => kms::Language::ZH,
                };
                let entity = svc.add_nomenclature(id, lang, full.to_string(), abbr).await?;
                let names_json: Vec<Value> = entity
                    .name
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "id": n.id.to_string(),
                            "lang": format!("{:?}", n.lang),
                            "full": n.full,
                            "abbr": n.abbr
                        })
                    })
                    .collect();
                Ok(ToolResult::success_json(
                    "add_nomenclature",
                    serde_json::json!({
                        "entity_id": id_str,
                        "names": names_json
                    }),
                ))
            })
        })),
        vec![],
    )
}

fn update_nomenclature(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_update_nomenclature",
        "Update an existing nomenclature's lang, full name, or abbreviation.",
    )
    .parameter("entity_id", "string", "UUID of the entity")
    .parameter("nomenclature_id", "string", "UUID of the nomenclature to update")
    .parameter("lang", "string", "New language: 'ZH' or 'EN'")
    .parameter("full", "string", "New full name")
    .parameter("abbr", "string", "New abbreviation (optional, pass empty string to clear)")
    .required("entity_id")
    .required("nomenclature_id")
    .required("lang")
    .required("full")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let entity_id = Uuid::parse_str(
                    input["entity_id"].as_str().ok_or("missing 'entity_id'")?,
                )
                .map_err(|_| "invalid 'entity_id' UUID")?;
                let nom_id = Uuid::parse_str(
                    input["nomenclature_id"].as_str().ok_or("missing 'nomenclature_id'")?,
                )
                .map_err(|_| "invalid 'nomenclature_id' UUID")?;
                let lang = input["lang"].as_str().ok_or("missing 'lang'")?;
                let full = input["full"].as_str().ok_or("missing 'full'")?;
                let abbr = input["abbr"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string());
                let lang = match lang {
                    "EN" => kms::Language::EN,
                    _ => kms::Language::ZH,
                };
                let entity =
                    svc.update_nomenclature(entity_id, nom_id, lang, full.to_string(), abbr)
                        .await?;
                let names_json: Vec<Value> = entity
                    .name
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "id": n.id.to_string(),
                            "lang": format!("{:?}", n.lang),
                            "full": n.full,
                            "abbr": n.abbr
                        })
                    })
                    .collect();
                Ok(ToolResult::success_json(
                    "update_nomenclature",
                    serde_json::json!({
                        "entity_id": entity_id.to_string(),
                        "names": names_json
                    }),
                ))
            })
        })),
        vec![],
    )
}

fn delete_nomenclature(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_delete_nomenclature",
        "Delete a nomenclature from an entity. The entity must retain at least one nomenclature.",
    )
    .parameter("entity_id", "string", "UUID of the entity")
    .parameter("nomenclature_id", "string", "UUID of the nomenclature to delete")
    .required("entity_id")
    .required("nomenclature_id")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let entity_id = Uuid::parse_str(
                    input["entity_id"].as_str().ok_or("missing 'entity_id'")?,
                )
                .map_err(|_| "invalid 'entity_id' UUID")?;
                let nom_id = Uuid::parse_str(
                    input["nomenclature_id"].as_str().ok_or("missing 'nomenclature_id'")?,
                )
                .map_err(|_| "invalid 'nomenclature_id' UUID")?;
                let entity = svc.delete_nomenclature(entity_id, nom_id).await?;
                let names_json: Vec<Value> = entity
                    .name
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "id": n.id.to_string(),
                            "lang": format!("{:?}", n.lang),
                            "full": n.full,
                            "abbr": n.abbr
                        })
                    })
                    .collect();
                Ok(ToolResult::success_json(
                    "delete_nomenclature",
                    serde_json::json!({
                        "entity_id": entity_id.to_string(),
                        "names": names_json
                    }),
                ))
            })
        })),
        vec![],
    )
}

fn get_entity(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new("kms_get_entity", "Get an entity by its nomenclature name.")
        .parameter("name", "string", "Nomenclature full name of the entity")
        .required("name")
        .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let name = input["name"].as_str().ok_or("missing 'name'")?;
                let entity_id = svc.resolve(name).await?;
                let entity = svc.get_entity(entity_id).await?;

                let names: Vec<Value> = entity
                    .name
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "lang": format!("{:?}", n.lang),
                            "full": n.full,
                            "abbr": n.abbr
                        })
                    })
                    .collect();

                Ok(ToolResult::success_json(
                    "get_entity",
                    serde_json::json!({
                        "names": names,
                        "definition": entity.definition
                    }),
                ))
            })
        })),
        vec![],
    )
}

fn search_entity(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new("kms_search_entity", "Search entities by nomenclature name (prefix match).")
        .parameter("keyword", "string", "Search keyword")
        .required("keyword")
        .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let keyword = input["keyword"].as_str().ok_or("missing 'keyword'")?;
                let entities = svc.search_entity(keyword).await?;

                let results: Vec<Value> = entities
                    .into_iter()
                    .map(|e| {
                        serde_json::json!({
                            "name": e.name.first().map(|n| n.full.as_str()).unwrap_or(""),
                            "definition": e.definition
                        })
                    })
                    .collect();

                Ok(ToolResult::success_json("search_entity", serde_json::Value::Array(results)))
            })
        })),
        vec![],
    )
}

fn get_entity_knowledge(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_get_entity_knowledge",
        "Get all knowledge entries that reference a given entity.",
    )
    .parameter("entity_name", "string", "Name of the entity to look up")
    .required("entity_name")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let entity_name = input["entity_name"].as_str().ok_or("missing 'entity_name'")?;
                let entity_id = svc.resolve(entity_name).await?;
                let knowledge_list = svc.get_entity_referencing_knowledge(entity_id).await?;

                let results: Vec<Value> = knowledge_list
                    .into_iter()
                    .map(|k| {
                        serde_json::json!({
                            "title": k.title,
                            "knowledge_type": format!("{:?}", k.knowledge_type),
                            "content": k.content,
                        })
                    })
                    .collect();

                Ok(ToolResult::success_json(
                    "get_entity_knowledge",
                    serde_json::json!({
                        "entity": entity_name,
                        "count": results.len(),
                        "knowledges": results,
                    }),
                ))
            })
        })),
        vec![],
    )
}

fn get_knowledge(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_get_knowledge",
        "Get the full content of a knowledge entry by its title.",
    )
    .parameter("title", "string", "Title of the knowledge entry to retrieve")
    .required("title")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let title = input["title"].as_str().ok_or("missing 'title'")?;
                let id = svc.resolve_knowledge(title).await?;
                let knowledge = svc.get_knowledge(id).await?;

                let entity_names: Vec<String> = {
                    let svc = svc.clone();
                    futures::future::join_all(
                        knowledge.entities.iter().map(|eid| {
                            let svc = svc.clone();
                            async move {
                                svc.get_entity(*eid)
                                    .await
                                    .ok()
                                    .and_then(|e| e.name.first().map(|n| n.full.clone()))
                            }
                        })
                    )
                    .await
                    .into_iter()
                    .filter_map(|n| n)
                    .collect::<Vec<_>>()
                };

                Ok(ToolResult::success_json(
                    "get_knowledge",
                    serde_json::json!({
                        "title": knowledge.title,
                        "knowledge_type": format!("{:?}", knowledge.knowledge_type),
                        "entities": entity_names,
                        "content": knowledge.content,
                    }),
                ))
            })
        })),
        vec![],
    )
}

/// Vague title suffixes that indicate the aspect is too generic.
/// Must be kept in sync with `kms::diagnostics::knowledge_rules::VAGUE_TITLE_KEYWORDS`.
const VAGUE_TITLE_SUFFIXES: &[&str] = &[
    "概述", "总结", "小结", "定义", "简介", "说明", "介绍", "基本概念", "疾病特征",
];

fn validate_knowledge_title(title: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Extract the suffix after " · " separator
    let suffix = title.split(" · ").nth(1).unwrap_or(title);
    for &keyword in VAGUE_TITLE_SUFFIXES {
        if suffix.contains(keyword) {
            return Err(format!(
                "标题 \"{title}\" 的切面描述包含模糊词汇 \"{keyword}\"。\
                 切面描述必须是具体的方面（如 \"药物治疗\"、\"诊断标准\"、\"发病机制\"），\
                 不能使用泛化术语。请选择一个更精确的切面名称后重试。"
            )
            .into());
        }
    }
    Ok(())
}

fn create_knowledge(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_create_knowledge",
        "Create a knowledge entry about an entity or entities. Knowledge can be an 'aspect' (about one entity) or 'relation' (between multiple entities).",
    )
    .parameter("title", "string", "Title of the knowledge entry")
    .parameter("knowledge_type", "string", "'aspect' or 'relation'")
    .parameter("entities", "array", "Array of all entity names mentioned in the content (wrapping each in [[...]])")
    .parameter("content", "string", "The knowledge content/notes — use [[entity name]] to mark every entity mention")
    .required("title")
    .required("knowledge_type")
    .required("entities")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let title = input["title"].as_str().ok_or("missing 'title'")?;
                validate_knowledge_title(title)?;
                let knowledge_type = match input["knowledge_type"].as_str() {
                    Some("relation") => kms::KnowledgeType::Relation,
                    _ => kms::KnowledgeType::Aspect,
                };
                let entity_refs: Vec<&str> = input["entities"]
                    .as_array()
                    .ok_or("missing 'entities'")?
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect();
                let content = input["content"].as_str().map(|s| s.to_string());

                // Auto-flatten nested headings (##, ###, …) to **bold** to prevent
                // internal_nested diagnostics — the index tree should carry hierarchy, not content.
                let content = content.map(|c| flatten_nested_headings(&c));

                let knowledge = svc
                    .create_knowledge_by_ref(title, knowledge_type, entity_refs, content)
                    .await?;

                Ok(ToolResult::success_json(
                    "create_knowledge",
                    serde_json::json!({ "title": knowledge.title }),
                ))
            })
        })),
        vec![],
    )
}

fn create_index(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_create_index",
        "Create an index entry under a parent index. Indexes organize entities and knowledge.",
    )
    .parameter("parent_ref", "string", "Title of parent index entry")
    .parameter("title", "string", "Title of this index entry")
    .parameter("target_ref", "string", "Name of knowledge to reference (optional)")
    .parameter("target_type", "string", "'knowledge' if linking to a knowledge entry (optional)")
    .required("parent_ref")
    .required("title")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let parent_ref = input["parent_ref"].as_str().ok_or("missing 'parent_ref'")?;
                let title = input["title"].as_str().ok_or("missing 'title'")?;
                let target_ref = input["target_ref"].as_str();
                let target_type = input["target_type"].as_str().map(|tt| match tt {
                    "knowledge" => kms::TargetType::Knowledge,
                    _ => kms::TargetType::Group,
                });

                svc.create_index_by_ref(parent_ref, Some(title.to_string()), target_ref, target_type)
                    .await?;

                Ok(ToolResult::success_json(
                    "create_index",
                    serde_json::json!({ "title": title }),
                ))
            })
        })),
        vec![],
    )
}

fn navigate_index(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_navigate",
        "Navigate the index pointer. Supports single segment, relative paths with '..', and absolute paths starting with '/'.\nExamples:\n- '心力衰竭' — descend into a child node\n- '..' — go to parent\n- '../心力衰竭' — go to parent then descend into '心力衰竭'\n- '/循环系统疾病/心力衰竭' — absolute path from root",
    )
    .parameter("target", "string", "Navigation target: child title, '..', relative path like '../心力衰竭', or absolute path like '/循环系统疾病/心力衰竭'")
    .required("target")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let target = input["target"].as_str().ok_or("missing 'target'")?;
                let location = svc.navigate(target).await?;
                Ok(ToolResult::success_json(
                    "navigate_index",
                    serde_json::json!({ "location": location }),
                ))
            })
        })),
        vec![],
    )
}

fn reorganize_children(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_reorganize_children",
        "Move specified child indexes under a newly created group index. Used to restructure the tree by grouping related siblings.",
    )
    .parameter("new_group_title", "string", "Title for the new grouping index")
    .parameter("child_titles", "array", "Titles of child indexes to move under the new group")
    .required("new_group_title")
    .required("child_titles")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let new_group_title = input["new_group_title"]
                    .as_str()
                    .ok_or("missing 'new_group_title'")?;
                let child_titles: Vec<String> = input["child_titles"]
                    .as_array()
                    .ok_or("missing 'child_titles'")?
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();

                if child_titles.is_empty() {
                    return Err("child_titles must not be empty".into());
                }

                let location = svc.reorganize_children(new_group_title, &child_titles).await?;

                Ok(ToolResult::success_json(
                    "reorganize_children",
                    serde_json::json!({ "location": location }),
                ))
            })
        })),
        vec![],
    )
}

fn move_index(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_move_index",
        "Move an index node (and its entire subtree) to a new parent. Use this to restructure the tree without creating duplicates.",
    )
    .parameter("index_title", "string", "Title of the index to move")
    .parameter("new_parent_title", "string", "Title of the new parent index to move under")
    .required("index_title")
    .required("new_parent_title")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let index_title = input["index_title"]
                    .as_str()
                    .ok_or("missing 'index_title'")?;
                let new_parent_title = input["new_parent_title"]
                    .as_str()
                    .ok_or("missing 'new_parent_title'")?;

                let result = svc.move_index(index_title, new_parent_title).await?;

                Ok(ToolResult::success("move_index", &result))
            })
        })),
        vec![],
    )
}

fn link_orphans(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_link_orphans",
        "Batch-link orphan knowledge entries under a parent index. Each knowledge title becomes a knowledge-type index child.",
    )
    .parameter("parent_ref", "string", "Title of the parent index node to link orphans under")
    .parameter("knowledge_titles", "array", "Array of orphan knowledge titles to link")
    .required("parent_ref")
    .required("knowledge_titles")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let parent_ref = input["parent_ref"].as_str().ok_or("missing 'parent_ref'")?;
                let knowledge_titles: Vec<&str> = input["knowledge_titles"]
                    .as_array()
                    .ok_or("missing 'knowledge_titles'")?
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect();

                if knowledge_titles.is_empty() {
                    return Err("knowledge_titles must not be empty".into());
                }

                let linked = svc.link_orphans(parent_ref, &knowledge_titles).await?;

                Ok(ToolResult::success_json(
                    "link_orphans",
                    serde_json::json!({
                        "linked": linked,
                        "count": linked.len(),
                    }),
                ))
            })
        })),
        vec![],
    )
}

fn update_knowledge(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_update_knowledge",
        "Update a knowledge entry's content and/or entities. Does NOT change the title.",
    )
    .parameter("title_ref", "string", "Current title of the knowledge to update")
    .parameter("content", "string", "New content — use [[entity name]] to mark entity mentions")
    .parameter("entities", "array", "New array of all entity names mentioned in the content")
    .required("title_ref")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let title_ref = input["title_ref"].as_str().ok_or("missing 'title_ref'")?;
                let content = input["content"].as_str();
                let entities: Option<Vec<&str>> = if input["entities"].is_array() {
                    Some(
                        input["entities"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter_map(|v| v.as_str())
                            .collect(),
                    )
                } else {
                    None
                };

                let knowledge = svc.update_knowledge_by_ref(title_ref, content, entities).await?;

                Ok(ToolResult::success_json(
                    "update_knowledge",
                    serde_json::json!({ "title": knowledge.title }),
                ))
            })
        })),
        vec![],
    )
}

fn rename_knowledge(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_rename_knowledge",
        "Rename a knowledge entry. All indexes referencing this knowledge are updated to the new title.",
    )
    .parameter("current_title", "string", "Current title of the knowledge to rename")
    .parameter("new_title", "string", "New title for the knowledge entry")
    .required("current_title")
    .required("new_title")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let current_title = input["current_title"].as_str().ok_or("missing 'current_title'")?;
                let new_title = input["new_title"].as_str().ok_or("missing 'new_title'")?;

                let knowledge = svc.rename_knowledge(current_title, new_title).await?;

                Ok(ToolResult::success_json(
                    "rename_knowledge",
                    serde_json::json!({ "old_title": current_title, "new_title": knowledge.title }),
                ))
            })
        })),
        vec![],
    )
}

fn delete_knowledge(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_delete_knowledge",
        "Delete a knowledge entry. Indexes referencing this knowledge are downgraded to empty Group nodes (may trigger empty_leaf diagnostics).",
    )
    .parameter("title", "string", "Title of the knowledge to delete")
    .required("title")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let title = input["title"].as_str().ok_or("missing 'title'")?;

                svc.delete_knowledge(title).await?;

                Ok(ToolResult::success_json(
                    "delete_knowledge",
                    serde_json::json!({ "deleted": title }),
                ))
            })
        })),
        vec![],
    )
}

fn delete_index(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_delete_index",
        "Delete an index node by its title. Cannot delete the root index. Children of the deleted node are reparented to the deleted node's parent.",
    )
    .parameter("title", "string", "Title of the index to delete")
    .required("title")
    .build();

    crate::toolset::ToolRegistration::new(
        definition,
        Box::new(crate::function::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            Box::pin(async move {
                let title = input["title"].as_str().ok_or("missing 'title'")?;

                svc.delete_index(title).await?;

                Ok(ToolResult::success_json(
                    "delete_index",
                    serde_json::json!({ "deleted": title }),
                ))
            })
        })),
        vec![],
    )
}
