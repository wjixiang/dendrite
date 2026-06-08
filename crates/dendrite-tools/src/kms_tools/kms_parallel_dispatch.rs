//! `kms_parallel_dispatch` — fan-out a user task across multiple
//! sub-agents that each build a staging sub-tree in parallel, then
//! fold the staging areas back into the main tree.
//!
//! Sub-agents are managed by `agentik_core::ProcessManager`, which
//! handles lifecycle (spawn / start / stop), event forwarding via
//! broadcast, and exit detection.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use agentik_core::process::ProcessEvent;
use agentik_core::Agent;
use agentik_sdk::model::model_pool::ModelPool;
use agentik_types::messages::ContentBlock;
use agentik_types::tools::{ToolBuilder, ToolResult};
use serde_json::Value;
use uuid::Uuid;

#[derive(serde::Deserialize, Clone)]
struct SubTask {
    /// Title of the staging Group node to create under Root.
    staging_title: String,
    /// The body of the sub-task to give the sub-agent as its initial
    /// user message.
    content: String,
    /// Optional title of the target parent in the main tree where the
    /// staging area should be merged after the sub-agent finishes.
    #[serde(default)]
    target_parent: Option<String>,
}

pub fn registration(
    svc: Arc<kms::KmsService>,
    pool: Arc<ModelPool>,
    sub_context_factory: Arc<
        dyn Fn(Arc<kms::KmsService>, Arc<ModelPool>, String) -> crate::SubAgentConfig + Send + Sync,
    >,
    process_manager: Arc<agentik_core::process::ProcessManager>,
    agent_titles: Arc<std::sync::RwLock<HashMap<Uuid, String>>>,
) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_parallel_dispatch",
        "Fan out a large knowledge-building task into multiple parallel sub-agents. \
         Each sub-task is given a dedicated staging Group node (a sub-root) under the \
         system root. Sub-agents run concurrently using a stateless query model and \
         write only inside their own staging area. When all sub-agents finish, each staging area \
         is folded back into a designated target parent in the main tree (if specified).",
    )
    .parameter(
        "subtasks",
        "array",
        "Array of sub-tasks: [{staging_title, content, target_parent?}, ...]",
    )
    .required("subtasks")
    .build();

    agentik_core::tools::ToolRegistration::new(
        definition,
        Box::new(agentik_core::tools::SimpleTool::new(move |input: Value| {
            let svc = svc.clone();
            let pool = pool.clone();
            let sub_context_factory = sub_context_factory.clone();
            let process_manager = process_manager.clone();
            let agent_titles = agent_titles.clone();
            Box::pin(async move {
                let subtasks: Vec<SubTask> = serde_json::from_value(
                    input["subtasks"].clone(),
                )
                .map_err(|e| format!("invalid 'subtasks': {e}"))?;

                if subtasks.is_empty() {
                    return Err("'subtasks' must not be empty".into());
                }

                dispatch_parallel(
                    &svc,
                    &pool,
                    &sub_context_factory,
                    &process_manager,
                    &agent_titles,
                    subtasks,
                )
                .await
            })
        })),
        vec![],
    )
}

async fn dispatch_parallel(
    svc: &Arc<kms::KmsService>,
    pool: &Arc<ModelPool>,
    sub_context_factory: &Arc<
        dyn Fn(Arc<kms::KmsService>, Arc<ModelPool>, String) -> crate::SubAgentConfig + Send + Sync,
    >,
    process_manager: &Arc<agentik_core::process::ProcessManager>,
    agent_titles: &Arc<std::sync::RwLock<HashMap<Uuid, String>>>,
    subtasks: Vec<SubTask>,
) -> Result<ToolResult, Box<dyn std::error::Error + Send + Sync>> {
    let root = svc
        .find_root()
        .await
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            format!("find_root: {e}").into()
        })?;

    // Phase 1: create one staging Group per sub-task.
    struct Plan {
        sub_task: SubTask,
        staging_id: Uuid,
    }
    let total = subtasks.len();
    let mut plan: Vec<Plan> = Vec::with_capacity(total);
    for sub_task in subtasks {
        let staging = svc
            .create_index(
                root.id,
                Some(sub_task.staging_title.clone()),
                None,
                Some(kms::TargetType::Group),
            )
            .await
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                format!("create staging '{}': {e}", sub_task.staging_title).into()
            })?;
        plan.push(Plan {
            sub_task,
            staging_id: staging.id,
        });
    }

    // Phase 2: spawn sub-agents via ProcessManager.
    #[derive(Clone)]
    struct Spawned {
        agent_id: Uuid,
        sub_task: SubTask,
        staging_id: Uuid,
    }
    let mut spawned: Vec<Spawned> = Vec::with_capacity(plan.len());
    for p in plan {
        // Sub-agents use a stateless query model: they receive a
        // one-shot `local_view` of the staging subtree and never rely
        // on a pinned global pointer. The shared `svc` is passed
        // directly — writes use `parent_ref` (title), reads use
        // absolute paths via `kms_view_local`.
        let sub_svc = svc.clone();
        // The absolute path of the staging Group we just created,
        // rooted at the system root (e.g. "/心血管疾病"). The factory
        // closure uses it to seed the sub-agent's `local_view`.
        let staging_path = format!("/{}", p.sub_task.staging_title);
        let mut config = sub_context_factory(sub_svc.clone(), pool.clone(), staging_path.clone());
        if let Some(init) = config.init.take() {
            init.await.map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                format!(
                    "initialize sub-agent '{}' context: {e}",
                    p.sub_task.staging_title
                )
                .into()
            })?;
        }

        let content = p.sub_task.content.clone();
        let staging_title = p.sub_task.staging_title.clone();

        let tools = crate::registrations(sub_svc.clone(), config.context.clone());
        let agent_id = process_manager
            .spawn(
                Agent::builder()
                    .with_model_pool(pool.clone())
                    .with_context(config.context)
                    .with_system_prompt_section(config.system_prompt)
                    .with_tools(tools),
            )
            .await
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                format!("spawn agent '{staging_title}': {e}").into()
            })?;

        // Register title so the TUI can display it.
        agent_titles
            .write()
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                format!("agent_titles lock: {e}").into()
            })?
            .insert(agent_id, staging_title.clone());

        process_manager
            .inject_message(
                &agent_id,
                vec![ContentBlock::Text { text: content }],
            )
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                format!("inject '{staging_title}': {e}").into()
            })?;

        process_manager
            .start(&agent_id)
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                format!("start '{staging_title}': {e}").into()
            })?;

        spawned.push(Spawned {
            agent_id,
            sub_task: p.sub_task,
            staging_id: p.staging_id,
        });
    }

    // Phase 3: wait for all sub-agents to exit.
    let mut rx = process_manager.events();
    let mut finished: HashSet<Uuid> = spawned.iter().map(|s| s.agent_id).collect();
    let expected: HashSet<Uuid> = finished.clone();
    let mut results: Vec<(Spawned, Result<(), String>)> = Vec::with_capacity(spawned.len());

    while !finished.is_empty() {
        match rx.recv().await {
            Ok(ProcessEvent::ProcessExited { agent_id, status }) => {
                if finished.remove(&agent_id) {
                    let sp = spawned
                        .iter()
                        .find(|s| s.agent_id == agent_id)
                        .cloned();
                    if let Some(sp) = sp {
                        let outcome = match status {
                            agentik_core::process::ProcessExitStatus::Completed => Ok(()),
                            other => Err(format!("{:?}", other)),
                        };
                        results.push((sp, outcome));
                    }
                }
            }
            Ok(_) => {
                // Agent events / StateChanged — not relevant here.
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                // Missed events; re-scan statuses for any we missed.
                for id in &expected {
                    if !finished.contains(id) {
                        continue;
                    }
                    // If the agent is no longer in the manager, treat as exited.
                    if process_manager.status(id).is_err() {
                        let sp = spawned.iter().find(|s| s.agent_id == *id).cloned();
                        if let Some(sp) = sp {
                            results.push((sp, Err("status lost (lagged)".into())));
                        }
                        finished.remove(id);
                    }
                }
            }
            Err(_) => break,
        }
    }

    // Phase 4: merge each staging area into its target parent (if specified).
    let mut merge_report: Vec<serde_json::Value> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for (sp, outcome) in &results {
        let mut report = serde_json::json!({
            "staging": sp.sub_task.staging_title,
        });
        if let Err(e) = outcome {
            failures.push(e.clone());
            report["error"] = serde_json::Value::String(e.clone());
            merge_report.push(report);
            continue;
        }
        if let Some(target) = sp.sub_task.target_parent.as_deref() {
            let target_id = match svc.resolve_index(target).await {
                Ok(id) => id,
                Err(e) => {
                    let msg = format!("resolve '{target}': {e} (staging left in place)");
                    report["merge_error"] = serde_json::Value::String(msg);
                    merge_report.push(report);
                    continue;
                }
            };
            match svc.merge_subtree(sp.staging_id, target_id).await {
                Ok(moved) => {
                    report["merged_into"] = serde_json::Value::String(target.to_string());
                    report["moved_children"] =
                        serde_json::Value::Number(serde_json::Number::from(moved));
                }
                Err(e) => {
                    report["merge_error"] =
                        serde_json::Value::String(format!("{e} (staging left in place)"));
                }
            }
        } else {
            report["merged_into"] = serde_json::Value::String("(none — staging kept)".into());
        }
        merge_report.push(report);
    }

    let mut body = serde_json::json!({
        "sub_agent_count": merge_report.len(),
        "failed_sub_agents": failures.len(),
        "merge_report": merge_report,
    });
    if !failures.is_empty() {
        body["errors"] = serde_json::Value::Array(
            failures
                .into_iter()
                .map(serde_json::Value::String)
                .collect(),
        );
    }

    Ok(ToolResult::success_json("parallel_dispatch", body))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the SubTask deserialization contract.
    #[test]
    fn subtask_deser_with_optional_target() {
        let json = serde_json::json!({
            "staging_title": "alpha",
            "content": "do stuff",
        });
        let t: SubTask = serde_json::from_value(json).unwrap();
        assert_eq!(t.staging_title, "alpha");
        assert_eq!(t.content, "do stuff");
        assert!(t.target_parent.is_none());
    }

    #[test]
    fn subtask_deser_with_target() {
        let json = serde_json::json!({
            "staging_title": "alpha",
            "content": "do stuff",
            "target_parent": "Biology",
        });
        let t: SubTask = serde_json::from_value(json).unwrap();
        assert_eq!(t.target_parent.as_deref(), Some("Biology"));
    }
}
