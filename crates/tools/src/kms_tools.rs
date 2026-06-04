use std::sync::Arc;
use serde_json::Value;
use uuid::Uuid;

use types::tools::{ToolBuilder, ToolResult};

pub fn registrations(svc: Arc<kms::KmsService>) -> Vec<crate::toolset::ToolRegistration> {
    vec![
        create_entity(svc.clone()),
        get_entity(svc.clone()),
        search_entity(svc.clone()),
        create_knowledge(svc.clone()),
        create_index(svc.clone()),
        navigate_index(svc.clone()),
        reorganize_children(svc.clone()),
        move_index(svc.clone()),
        link_orphans(svc.clone()),
        update_knowledge(svc.clone()),
        rename_knowledge(svc.clone()),
        delete_knowledge(svc),
    ]
}

fn create_entity(svc: Arc<kms::KmsService>) -> crate::toolset::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_create_entity",
        "Create a new entity in the knowledge graph. Entities represent things/concepts.",
    )
    .parameter("names", "array", "Array of nomenclatures: [{lang: 'ZH'|'EN', full: string, abbr?: string}]")
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
                let names_arr = input["names"].as_array().ok_or("missing 'names'")?;

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

                let entity = svc.create_entity(nomenclatures, definition).await?;

                Ok(ToolResult::success_json(
                    "create_entity",
                    serde_json::json!({
                        "name": entity.name.first().map(|n| n.full.as_str()).unwrap_or(""),
                        "definition": entity.definition
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
