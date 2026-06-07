//! `kms_parallel_dispatch` — fan-out a user task across multiple
//! sub-agents that each build a staging sub-tree in parallel, then
//! fold the staging areas back into the main tree.
//!
//! This is the core orchestration entry point that turns the
//! single-root KMS into a "single main root, multi sub-root" system.

use std::sync::Arc;
use std::time::Instant;

use agentik_sdk::model::model_pool::ModelPool;
use agentik_types::messages::ContentBlock;
use agentik_types::tools::{ToolBuilder, ToolResult};
use futures::stream::{FuturesUnordered, StreamExt};
use serde_json::Value;
use uuid::Uuid;

use crate::parallel_progress::{ParallelProgress, ParallelProgressTx};

#[derive(serde::Deserialize)]
struct SubTask {
    /// Title of the staging Group node to create under Root.
    staging_title: String,
    /// The body of the sub-task to give the sub-agent as its initial
    /// user message.
    content: String,
    /// Optional title of the target parent in the main tree where the
    /// staging area should be merged after the sub-agent finishes.
    /// When omitted, the staging area is left in place (caller can
    /// `kms_merge_subtree` later).
    #[serde(default)]
    target_parent: Option<String>,
}

pub fn registration(
    svc: Arc<kms::KmsService>,
    pool: Arc<ModelPool>,
    sub_context_factory: Arc<
        dyn Fn(Arc<kms::KmsService>) -> Arc<dyn agentik_core::context::AgentContext> + Send + Sync,
    >,
    progress_tx: ParallelProgressTx,
) -> agentik_core::tools::ToolRegistration {
    let definition = ToolBuilder::new(
        "kms_parallel_dispatch",
        "Fan out a large knowledge-building task into multiple parallel sub-agents. \
         Each sub-task is given a dedicated staging Group node (a sub-root) under the \
         system root. Sub-agents run concurrently with isolated pointers and write only \
         inside their own staging area. When all sub-agents finish, each staging area \
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
            let progress_tx = progress_tx.clone();
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
                    &progress_tx,
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
        dyn Fn(Arc<kms::KmsService>) -> Arc<dyn agentik_core::context::AgentContext> + Send + Sync,
    >,
    progress_tx: &ParallelProgressTx,
    subtasks: Vec<SubTask>,
) -> Result<ToolResult, Box<dyn std::error::Error + Send + Sync>> {
    let dispatch_started = Instant::now();
    let _ = progress_tx.send(ParallelProgress::DispatchStarted {
        total: subtasks.len(),
    });

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
    for (index, sub_task) in subtasks.into_iter().enumerate() {
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
        let _ = progress_tx.send(ParallelProgress::StagingCreated {
            index,
            total,
            title: sub_task.staging_title.clone(),
        });
        plan.push(Plan {
            sub_task,
            staging_id: staging.id,
        });
    }

    // Phase 2: build + spawn one sub-agent per staging area.
    // Uses `FuturesUnordered` for true concurrency: sub-agents no
    // longer block each other on LLM calls. Per-sub-agent progress
    // is delivered via a private sub-event channel that a forwarder
    // task wraps and republishes to the main `progress_tx` with the
    // sub-agent's title attached.
    enum JoinResult {
        Ok(Plan),
        JoinError(Plan, String),
    }
    let mut results: Vec<JoinResult> = Vec::with_capacity(plan.len());

    // Build a list of (index, plan) to drive the unordered pool.
    let indexed: Vec<(usize, Plan)> = plan.into_iter().enumerate().collect();

    let mut unordered: FuturesUnordered<tokio::task::JoinHandle<JoinResult>> =
        FuturesUnordered::new();
    for (index, p) in indexed {
        let sub_svc = Arc::new(svc.with_pointer(p.staging_id));
        let ctx = sub_context_factory(sub_svc);
        let content = p.sub_task.content.clone();
        let staging_title = p.sub_task.staging_title.clone();
        let pool = pool.clone();
        let progress_tx = progress_tx.clone();

        // Private event channel: sub-agent's `event_tx` is the sender
        // side; a forwarder task drains the receiver side and
        // republishes events on the main `progress_tx` with the
        // sub-agent's title attached. The forwarder exits naturally
        // when the sender is dropped (i.e. when the sub-agent ends).
        let (sub_event_tx, sub_event_rx) = tokio::sync::mpsc::unbounded_channel::<
            agentik_types::AgentUiEvent,
        >();
        let title_for_forwarder = staging_title.clone();
        let progress_tx_for_forwarder = progress_tx.clone();
        tokio::spawn(async move {
            let mut rx = sub_event_rx;
            while let Some(event) = rx.recv().await {
                // Filter out orchestrator-level signals that would
                // confuse the TUI state machine if they came from a
                // sub-agent.
                match event {
                    agentik_types::AgentUiEvent::Done
                    | agentik_types::AgentUiEvent::Requesting => continue,
                    other => {
                        let _ = progress_tx_for_forwarder.send(
                            ParallelProgress::SubAgentEvent {
                                title: title_for_forwarder.clone(),
                                event: other,
                            },
                        );
                    }
                }
            }
        });

        let _ = progress_tx.send(ParallelProgress::SubAgentStarted {
            index,
            total,
            title: staging_title.clone(),
        });

        let handle = tokio::spawn(async move {
            let agent_started = Instant::now();
            let mut agent = match agentik_core::Agent::builder()
                .with_model_pool(pool)
                .with_context(ctx)
                .build()
                .await
            {
                Ok(a) => a,
                Err(e) => {
                    let duration_ms = agent_started.elapsed().as_millis() as u64;
                    let _ = progress_tx.send(ParallelProgress::SubAgentFailed {
                        index,
                        total,
                        title: staging_title.clone(),
                        error: format!("build agent: {e}"),
                        duration_ms,
                    });
                    return JoinResult::JoinError(
                        Plan {
                            sub_task: p.sub_task,
                            staging_id: p.staging_id,
                        },
                        format!("build agent '{staging_title}': {e}"),
                    );
                }
            };
            // Wire the sub-agent's event channel to the forwarder.
            agent.event_tx = Some(sub_event_tx);
            if let Err(e) = agent.inject_message(vec![ContentBlock::Text { text: content }]) {
                let duration_ms = agent_started.elapsed().as_millis() as u64;
                let _ = progress_tx.send(ParallelProgress::SubAgentFailed {
                    index,
                    total,
                    title: staging_title.clone(),
                    error: format!("inject: {e}"),
                    duration_ms,
                });
                return JoinResult::JoinError(
                    Plan {
                        sub_task: p.sub_task,
                        staging_id: p.staging_id,
                    },
                    format!("inject '{staging_title}': {e}"),
                );
            }
            if let Err(e) = agent.start().await {
                let duration_ms = agent_started.elapsed().as_millis() as u64;
                let _ = progress_tx.send(ParallelProgress::SubAgentFailed {
                    index,
                    total,
                    title: staging_title.clone(),
                    error: format!("run: {e}"),
                    duration_ms,
                });
                return JoinResult::JoinError(
                    Plan {
                        sub_task: p.sub_task,
                        staging_id: p.staging_id,
                    },
                    format!("run '{staging_title}': {e}"),
                );
            }
            let duration_ms = agent_started.elapsed().as_millis() as u64;
            let _ = progress_tx.send(ParallelProgress::SubAgentCompleted {
                index,
                total,
                title: staging_title.clone(),
                duration_ms,
            });
            JoinResult::Ok(p)
        });
        unordered.push(handle);
    }

    while let Some(joined) = unordered.next().await {
        match joined {
            Ok(result) => results.push(result),
            Err(join_err) => {
                // JoinError means a tokio::spawn panic. The agent
                // task itself already sends a SubAgentFailed event
                // before returning, so we just need to record a
                // synthetic failure so the merge phase skips it.
                results.push(JoinResult::JoinError(
                    Plan {
                        sub_task: SubTask {
                            staging_title: format!("<panic: {join_err}>"),
                            content: String::new(),
                            target_parent: None,
                        },
                        staging_id: Uuid::nil(),
                    },
                    format!("join panic: {join_err}"),
                ));
            }
        }
    }

    // Phase 3: collect failures and the plans that succeeded.
    let mut failures: Vec<String> = Vec::new();
    let mut ready: Vec<Plan> = Vec::new();
    for r in results {
        match r {
            JoinResult::Ok(p) => ready.push(p),
            JoinResult::JoinError(_p, e) => failures.push(e),
        }
    }

    // Phase 4: merge each staging area into its target parent (if specified).
    let mut merge_report: Vec<serde_json::Value> = Vec::new();
    let mut succeeded: usize = 0;
    for (idx, p) in ready.into_iter().enumerate() {
        let mut report = serde_json::json!({
            "staging": p.sub_task.staging_title,
        });
        if let Some(target) = p.sub_task.target_parent.as_deref() {
            let target_id = match svc.resolve_index(target).await {
                Ok(id) => id,
                Err(e) => {
                    let msg = format!("resolve '{target}': {e} (staging left in place)");
                    report["merge_error"] = serde_json::Value::String(msg);
                    merge_report.push(report);
                    continue;
                }
            };
            match svc.merge_subtree(p.staging_id, target_id).await {
                Ok(moved) => {
                    let _ = progress_tx.send(ParallelProgress::Merged {
                        index: idx,
                        total,
                        target: target.to_string(),
                        moved,
                    });
                    report["merged_into"] = serde_json::Value::String(target.to_string());
                    report["moved_children"] =
                        serde_json::Value::Number(serde_json::Number::from(moved));
                    succeeded += 1;
                }
                Err(e) => {
                    report["merge_error"] =
                        serde_json::Value::String(format!("{e} (staging left in place)"));
                }
            }
        } else {
            report["merged_into"] = serde_json::Value::String("(none — staging kept)".into());
            succeeded += 1;
        }
        merge_report.push(report);
    }

    let _ = progress_tx.send(ParallelProgress::DispatchFinished {
        total,
        succeeded,
        failed: failures.len(),
        elapsed_ms: dispatch_started.elapsed().as_millis() as u64,
    });

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
    use crate::parallel_progress::ParallelProgress;

    /// Verify that the `ParallelProgress` variant types we send match
    /// the contract the TUI state machine expects. This is a
    /// compile-time check plus a runtime count.
    #[test]
    fn dispatch_started_carries_total() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(ParallelProgress::DispatchStarted { total: 3 }).unwrap();
        match rx.try_recv().unwrap() {
            ParallelProgress::DispatchStarted { total } => assert_eq!(total, 3),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn sub_agent_event_carries_title() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(ParallelProgress::SubAgentEvent {
            title: "alpha".to_string(),
            event: agentik_types::AgentUiEvent::LlmResponse("hi".to_string()),
        })
        .unwrap();
        match rx.try_recv().unwrap() {
            ParallelProgress::SubAgentEvent { title, event } => {
                assert_eq!(title, "alpha");
                match event {
                    agentik_types::AgentUiEvent::LlmResponse(s) => assert_eq!(s, "hi"),
                    _ => panic!("wrong inner variant"),
                }
            }
            _ => panic!("wrong outer variant"),
        }
    }

    /// Sanity: a 5-slot channel buffer of small enum values should
    /// be well under any memory limit. This isn't a load test, just
    /// a contract check that the channel is usable for typical
    /// dispatch sizes.
    #[test]
    fn channel_drains_in_order() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        for i in 0..5 {
            tx.send(ParallelProgress::SubAgentStarted {
                index: i,
                total: 5,
                title: format!("t{i}"),
            })
            .unwrap();
        }
        for i in 0..5 {
            match rx.try_recv().unwrap() {
                ParallelProgress::SubAgentStarted { index, total, title } => {
                    assert_eq!(index, i);
                    assert_eq!(total, 5);
                    assert_eq!(title, format!("t{i}"));
                }
                _ => panic!("wrong variant"),
            }
        }
    }
}
