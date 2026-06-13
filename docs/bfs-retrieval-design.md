# BFS Multi-Agent Retrieval — Design

> Status: design draft, awaiting implementation
> Owner: KMS / `agent-bfs`
> Date: 2026-06-12

This document specifies a **breadth-first, multi-agent retrieval strategy** for the Dendrite KMS, complementing the existing single-agent, depth-first retrieval agent. It is the design source-of-truth for the new `agent-bfs` crate and the `kms_bfs_dispatch` tool.

---

## 1. Background and motivation

The current read-only retrieval agent (`crates/agent-knowledge`) explores the knowledge tree as a **single LLM agent** that issues multiple rounds of *parallel tool calls* within one conversation. The README's prompt calls this pattern *fan-out / fan-in*: round 1 fans out across candidate subtrees, round 2 fans in on the relevant knowledge titles, round 3 synthesizes.

That pattern is *depth-first by behavior*: a single agent dives into one or a few subtrees and ends as soon as it has enough. There is no mechanism for:

- **Bounded, layered expansion.** A single agent cannot be told "explore at most 4 children per layer for 3 layers" — its budget is implicit in token usage.
- **Bounded parallelism across subtrees.** A single LLM call can fan out tools, but all calls land in *one* agent's context and share one model call's latency.
- **Deterministic termination.** Termination is "the agent decided it had enough" — there is no quantitative signal.

A *parallel* role (`kms_parallel_dispatch`) exists for **write-side** multi-agent orchestration, but its design is single-shot (split by domain, run sub-agents, merge) and is not layered.

This design introduces a **read-side BFS orchestrator** that:

1. Maintains an explicit frontier, visited set, and evidence bag.
2. Expands the frontier **layer by layer**, one parallel sub-agent per frontier node.
3. Terminates on a quantitative **evidence-stability** signal (with depth/budget safety caps).
4. Returns a structured envelope; the orchestrator agent then writes a cited final answer.

The new role is a peer of the existing depth-first retrieval agent, not a replacement. The two coexist as two read-only strategies switchable in the TUI.

---

## 2. Goals and non-goals

### Goals

- Add **layered, bounded, deterministic** multi-agent retrieval to the read side.
- Reuse the **8 existing read-only tools** as the sub-agent's atomic capability surface — no new retrieval primitives.
- Provide a **structured, citable** evidence bag that the orchestrator agent can ground its answer in.
- Keep the **single-shot tool** model (matches `kms_parallel_dispatch`); no streaming / no async task IDs.
- Stay **stateless across queries** — BFS state lives only inside one `kms_bfs_dispatch` invocation.

### Non-goals (this round)

- **Graph BFS** across `[[entity-name]]` cross-links. The frontier is **pure group-children expansion** only.
- **Persistent BFS state**. No DB writes; nothing survives the call.
- **Streaming progress** to the TUI. The tool is synchronous-from-agent-perspective. Traces go to `tui.log`.
- **Sub-agent cross-talk**. Sub-agents are blind to each other; results aggregate only at the orchestrator.
- **Cross-query caching** of sub-agent results. Every BFS run is fresh.
- **Hybrid tree + entity-hop mode**. Deferred; a future `mode: tree | graph | hybrid` option on `kms_bfs_dispatch` may add it.
- **User-tunable parameters in the TUI**. Defaults are hard-coded; parameter UI is a later iteration.

---

## 3. Architectural overview

```
                          ┌────────────────────────────┐
   user query Q ────────► │  BFS Orchestrator Agent    │
                          │  (BfsContext, BFS prompt)  │
                          │                            │
                          │  Tools: 8 read-only +      │
                          │         kms_bfs_dispatch   │
                          └────────────┬───────────────┘
                                       │ call
                                       ▼
                          ┌────────────────────────────┐
                          │  kms_bfs_dispatch tool     │
                          │  → BfsRuntime (in agent-  │
                          │    bfs crate)              │
                          └────────────┬───────────────┘
                                       │ per-layer fan-out
                ┌──────────────────────┼──────────────────────┐
                ▼                      ▼                      ▼
       Sub-agent @ seed₁       Sub-agent @ seed₂    …   Sub-agent @ seed_k
       (BFS_SUB_PROMPT)        (BFS_SUB_PROMPT)          (k ≤ max_per_layer)
       Tools: 8 read-only      Tools: 8 read-only
       Returns:                Returns:
         - relevant_knowledge    - relevant_knowledge
         - next_seeds            - next_seeds
         - notes                 - notes
                │                      │                      │
                └──────────────────────┼──────────────────────┘
                                       │ aggregate
                                       ▼
                          ┌────────────────────────────┐
                          │  BfsRuntime: hash new      │
                          │  evidence; check stability │
                          │  promote next_seeds into   │
                          │  next frontier (dedup vs   │
                          │  visited, cap per layer)   │
                          └────────────┬───────────────┘
                                       │ envelope
                                       ▼
                          ┌────────────────────────────┐
                          │  Orchestrator agent        │
                          │  synthesizes final answer  │
                          │  with cited knowledge      │
                          │  titles from evidence bag  │
                          └────────────────────────────┘
```

The **tree** is the only topology: every expansion step is `parent → child group`. The **evidence bag** is the only inter-layer memory. The **BfsRuntime** is the only stateful component.

---

## 4. Crate reorganization

| Current | New | Notes |
|---|---|---|
| `crates/agent-knowledge` | `crates/agent-dfs` | Single-agent, depth-first read-only retrieval. Renamed; behavior unchanged. Prompt renamed `KNOWLEDGE_RETRIEVAL_PROMPT` → `DFS_RETRIEVAL_PROMPT`, prompt text lightly retitled to make the strategy explicit. |
| — | `crates/agent-bfs` | New crate; multi-agent BFS read-only retrieval. |

`agent-dfs` and `agent-bfs` are both **read-only** KMS strategies; they share the same 8-tool read-only registration set, and add exactly one new tool (`kms_bfs_dispatch`) to `agent-bfs` only.

`kms_tui` gains a fourth role switchable by `[Tab]`:

| Tab | Role | Crate |
|---|---|---|
| 1 | Compose | `agent-compose` (writes) |
| 2 | DFS (read) | `agent-dfs` (renamed from `agent-knowledge`) |
| 3 | **BFS (read)** | `agent-bfs` (new) |
| 4 | Parallel | `agent-compose::ParallelComposeContext` (writes) |

The role enum in `kms_tui/src/state.rs` and the corresponding agent-construction switch in `main.rs` / `components/agent.rs` are updated.

---

## 5. BFS orchestrator agent — `agent-bfs`

### 5.1 Context

```rust
pub struct BfsContext {
    kms: Arc<kms::KmsService>,
    state: RwLock<ContextSnapshot>,
}
```

Mirrors `KnowledgeContext` (the renamed `DfsContext`). `initialize()` injects a `local_view` of `/` into the snapshot at version 1. `write()` is a no-op — the BFS context is read-only. The version never changes after `initialize()`, so the view is never re-injected.

### 5.2 Tool set (11 tools)

**Read-only primitives (8 — identical to `agent-dfs`):**

- `kms_local`, `kms_subtree_knowledge`, `kms_search_subtree`
- `kms_search_entity`, `kms_get_entity`, `kms_get_entity_knowledge`, `kms_get_knowledge`
- `kms_navigate` (legacy, retained for compatibility)

**New (1):**

- `kms_bfs_dispatch` — the orchestration tool.

The BFS agent is the **only** role allowed to call `kms_bfs_dispatch`. It is NOT in the `readonly_registrations()` list (which is what `agent-dfs` consumes); it is registered explicitly in `agent-bfs`.

### 5.3 Prompt — `BFS_RETRIEVAL_PROMPT`

The prompt follows the same shape as `DFS_RETRIEVAL_PROMPT` but with BFS-specific guidance:

```
You are a **read-only BFS retrieval orchestrator**. Your job is to
answer the user's question by dispatching one parallel search of the
knowledge tree, then synthesizing a final answer from the evidence
returned.

## Step 1 — pick seed paths (cheap pre-warm)

Before calling kms_bfs_dispatch, decide which subtree(s) are most
likely to contain answers. You have two free, fast tools for this:

- `kms_search_entity(keyword)` — prefix match on entity names.
- `kms_local(path)` — inspect a subtree's structural summary.

Pick 1–3 seed paths. If the question mentions specific entities, search
for them first. If the question is broad, start at `/` and pick the
top-level groups that look most relevant from the root local view.

## Step 2 — dispatch one BFS

Call `kms_bfs_dispatch` exactly once with:
  - `query`: the user's full question, verbatim
  - `seed_paths`: the paths you picked in Step 1
  - (optional) `max_depth`, `stable_k`, `max_per_layer`, `max_total_subagents`
    — leave at defaults unless you have a strong reason to override

The tool returns a structured envelope describing each BFS layer and
the final evidence bag. The BFS itself decides when to stop (evidence
stable for K consecutive layers, or frontier empty, or a safety cap hit).

## Step 3 — synthesize the final answer

From the evidence bag, write a clear, structured answer:

- Cite a specific `title` (and ideally a `path`) for every factual claim.
- If the evidence is thin, say so explicitly — do not invent.
- If multiple subtrees contributed, organize the answer by subtree or
  by aspect, not by retrieval order.

## Termination

When you have written the final answer, stop. Do NOT make another
tool call. The agent loop will end automatically when no tool call is
issued.
```

The prompt is intentionally short. The heavy lifting is in `kms_bfs_dispatch`.

---

## 6. New tool — `kms_bfs_dispatch`

### 6.1 Registration

Lives at `crates/dendrite-tools/src/kms_tools/kms_bfs_dispatch.rs`. Registration is added to `kms_tools::registrations()` (the master list); it is **not** in `readonly_registrations()`.

The tool factory takes:

```rust
pub fn registration(
    svc: Arc<kms::KmsService>,
    model_pool: Arc<ModelPool>,        // shared with orchestrator
    prompt_registry: Arc<PromptRegistry>,
) -> ToolRegistration
```

This matches the dependency surface that `kms_parallel_dispatch` already needs.

### 6.2 Tool signature

```json
{
  "name": "kms_bfs_dispatch",
  "description": "Breadth-first multi-agent retrieval over the index tree. ...",
  "parameters": {
    "type": "object",
    "properties": {
      "query":         { "type": "string" },
      "seed_paths":    { "type": "array", "items": { "type": "string" } },
      "max_depth":           { "type": "integer", "default": 4 },
      "stable_k":            { "type": "integer", "default": 2 },
      "max_per_layer":       { "type": "integer", "default": 4 },
      "max_total_subagents": { "type": "integer", "default": 16 }
    },
    "required": ["query", "seed_paths"]
  }
}
```

### 6.3 Return envelope

```rust
#[derive(Serialize)]
pub struct BfsResult {
    pub termination_reason: TerminationReason,
    pub layers: Vec<BfsLayerTrace>,
    pub evidence: Vec<EvidenceItem>,
    pub visited_count: usize,
    pub subagents_total: u32,
    pub config: BfsConfig,                // echo effective config
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminationReason {
    EvidenceStable,   // primary signal
    FrontierEmpty,
    DepthCap,         // safety cap
    BudgetCap,        // safety cap
    SeedResolution,   // seeds did not resolve to any group node
}

#[derive(Serialize)]
pub struct BfsLayerTrace {
    pub layer: u32,
    pub frontier: Vec<String>,             // group paths dispatched this layer
    pub expanded: Vec<ExpandedNode>,       // per-seed outcome
    pub new_evidence_count: usize,
    pub subagents_dispatched: u32,
    pub evidence_hash: u64,                // hash of titles-of-new-evidence
    pub stable_rounds: u32,                // consecutive layers with same hash
}

#[derive(Serialize)]
pub struct ExpandedNode {
    pub seed_path: String,
    pub resolved_node_id: Option<Uuid>,
    pub relevant_knowledge: Vec<EvidenceItem>,
    pub next_seeds: Vec<NextSeed>,
    pub notes: Option<String>,
    pub error: Option<String>,             // sub-agent failure surfaced
}

#[derive(Serialize, Clone)]
pub struct EvidenceItem {
    pub title: String,
    pub excerpt: String,                   // ≤ 280 chars
    pub path: String,                      // absolute group path containing it
    pub entities: Vec<String>,             // entity names mentioned
    pub layer_found: u32,
}

#[derive(Serialize)]
pub struct NextSeed {
    pub path: String,
    pub reason: String,
}
```

The envelope is sized to fit comfortably in the orchestrator's tool-result budget. `excerpt` is hard-capped at 280 chars; if a knowledge entry's content is longer, the sub-agent truncates and the orchestrator can `kms_get_knowledge(title)` for the full text on demand.

---

## 7. BFS runtime — `agent-bfs::BfsRuntime`

```rust
pub struct BfsRuntime {
    config: BfsConfig,
    state: BfsState,
    // collaborators
    kms: Arc<kms::KmsService>,
    sub_runner: SubAgentRunner,
}

pub struct BfsState {
    pub frontier: VecDeque<SeedEntry>,
    pub visited: HashSet<Uuid>,           // node ids, not paths
    pub evidence: Vec<EvidenceItem>,
    pub evidence_by_layer: Vec<Vec<String>>, // titles per layer (for hashing)
    pub last_hash: Option<u64>,
    pub stable_rounds: u32,
    pub layer: u32,
    pub subagents_used: u32,
    pub traces: Vec<BfsLayerTrace>,
}

impl BfsRuntime {
    pub async fn run(
        &mut self,
        query: String,
        seed_paths: Vec<String>,
    ) -> Result<BfsResult, String> { /* see algorithm below */ }
}
```

### 7.1 Algorithm

Pseudocode (matches §1 of the high-level design):

```text
fn run(query, seed_paths):
    # resolve seeds → (node_id, path) pairs
    seeds = resolve_seeds(seed_paths)
    if seeds is empty:
        return BfsResult { termination: SeedResolution, evidence: [], ... }

    for (id, path) in seeds:
        if !visited.contains(id):
            frontier.push_back(SeedEntry { id, path })
            visited.insert(id)

    loop:
        if stable_rounds >= config.stable_k:
            return terminate(EvidenceStable)
        if frontier.is_empty():
            return terminate(FrontierEmpty)
        if layer >= config.max_depth:
            return terminate(DepthCap)
        if subagents_used >= config.max_total_subagents:
            return terminate(BudgetCap)

        # take up to max_per_layer for this round
        this_layer: Vec<SeedEntry> = drain up to max_per_layer from frontier

        # parallel sub-agent fan-out
        results = sub_runner.fan_out(&query, &this_layer).await

        new_evidence_titles: Vec<String> = []
        for r in results:
            visited.insert(r.seed.id)
            for ek in r.relevant_knowledge:
                evidence.push(ek.clone())
                new_evidence_titles.push(ek.title.clone())
            for ns in r.next_seeds:
                if !visited.contains(ns.id) and layer + 1 < max_depth:
                    frontier.push_back(ns)

        # hash new evidence titles, update stability counter
        new_hash = hash(&new_evidence_titles)
        if Some(new_hash) == last_hash and !new_evidence_titles.is_empty():
            stable_rounds += 1
        else:
            stable_rounds = 0
        last_hash = Some(new_hash)

        traces.push(BfsLayerTrace { layer, frontier, expanded, ... })
        layer += 1
        subagents_used += results.len()
```

Notes:

- **visited** stores node *IDs*, not paths. Two different paths that resolve to the same `Index` row (e.g. via shared reference, or alias) are deduped correctly.
- **new_evidence_titles** is computed before any next-layer work; the hash captures the *delta* this layer produced.
- If `new_evidence_titles` is empty AND frontier is empty, the loop hits the `frontier_empty` branch (no advance); the `evidence_stable` branch is suppressed by the `!is_empty()` guard so an empty layer does not falsely look "stable".
- `max_total_subagents` is checked at the top of the loop. If a layer would push past it, the layer is **not dispatched**; termination reason is `BudgetCap`. This avoids partial-layer artifacts.

### 7.2 Seed resolution

```rust
async fn resolve_seeds(paths: &[String]) -> Vec<(Uuid, String)>
```

For each path, call `KmsService::get_local_view_by_path(path)` and take `(node.id, node.title)`. If a path does not resolve (deleted node, typo), drop it silently — the orchestrator already has the local view and would have noticed; we don't error.

### 7.3 Safety caps and termination priority

Evaluated **in this order at the top of every loop iteration**:

1. `evidence_stable` (primary signal)
2. `frontier_empty`
3. `depth_cap`
4. `budget_cap`

If multiple fire on the same iteration, the first match wins. In practice, by construction only one fires — but ordering is documented for the test suite.

---

## 8. Sub-agent

### 8.1 Invocation

The `SubAgentRunner` constructs a fresh **agentik-core** agent loop per sub-agent, configured with:

- **System prompt**: `BFS_SUB_PROMPT` (see §8.2).
- **Tools**: the same 8 read-only tools (no `kms_bfs_dispatch`).
- **Model pool**: same `Arc<ModelPool>` as the orchestrator.
- **Initial user message**: `BFS_SUB_TASK_TEMPLATE { query, seed_path }`.
- **Hard call cap**: 8 tool calls. If the sub-agent exceeds it, the runner force-terminates with whatever it has and returns a `notes` warning.
- **No persistence**: sub-agent state is dropped on return.

The runner fans out N sub-agents via `futures::future::join_all` (or `tokio::spawn` if the agent loop is `Send`); results aggregate into a `Vec<SubAgentOutcome>`.

### 8.2 Prompt — `BFS_SUB_PROMPT`

```
You are a **read-only retrieval sub-agent**. You are exploring ONE
specific subtree of the knowledge tree on behalf of an orchestrator
that has already broken the user's question into sub-tasks.

## User's question (full, verbatim)

<query>

## Your scope

Subtree rooted at: <seed_path>

You MUST NOT call any tool that would explore outside this subtree.
If a tool would return cross-subtree data (e.g. kms_get_entity_knowledge
across many entities), restrict it conceptually to entities reachable
through the subtree.

## Allowed tools

- kms_local
- kms_subtree_knowledge
- kms_search_subtree
- kms_search_entity   (entities appearing in <seed_path>'s subtree)
- kms_get_entity      (entities appearing in <seed_path>'s subtree)
- kms_get_entity_knowledge
- kms_get_knowledge
- kms_navigate

## Your output (must be valid JSON)

{
  "relevant_knowledge": [
    {
      "title": "<exact knowledge title>",
      "excerpt": "<≤280 char excerpt, the most relevant sentence>",
      "path": "<absolute group path containing the knowledge>",
      "relevance_reason": "<one short sentence>"
    }
  ],
  "next_seeds": [
    {
      "path": "<absolute path of a child group that looks promising>",
      "reason": "<why this child might contain more relevant knowledge>"
    }
  ],
  "notes": "<free text; observed gaps, ambiguity, etc.>"
}

## When to stop

Stop when **any** of:
  (a) you have found enough relevant knowledge to contribute;
  (b) the subtree is exhausted (no more promising children);
  (c) you have made 8 tool calls.

When you stop, your FINAL message must be the JSON object above and
nothing else. Do NOT write a prose summary — the orchestrator will
synthesize the user's answer.
```

The "final message is JSON and nothing else" rule is enforced by the `SubAgentRunner` parsing the last assistant turn as JSON; if it fails, the runner returns a parse error in the `error` field of the expanded node and treats the sub-agent as having contributed no evidence (and no next_seeds).

### 8.3 Concurrency

Within a layer, all sub-agents run **truly in parallel** — independent model calls, no shared mutable state. Across layers, the runtime is sequential: a layer completes (all sub-agents joined), evidence is hashed, stability is checked, and only then the next layer starts. This is intentional — it gives the `evidence_stable` signal a well-defined per-layer semantics.

---

## 9. End-to-end example

> **User query**: *"Compare the architectures of Docker and Kubernetes."*

### Orchestrator round (BFS agent's LLM)

1. `kms_search_entity("Docker")` → finds entity `Docker`.
2. `kms_search_entity("Kubernetes")` → finds entity `Kubernetes`.
3. `kms_local("/容器技术/Docker")` → confirms it exists as a group.
4. `kms_local("/容器技术/Kubernetes")` → confirms it exists.
5. Calls `kms_bfs_dispatch(query="对比Docker和Kubernetes的架构设计", seed_paths=["/容器技术/Docker", "/容器技术/Kubernetes"])` with all defaults.

### BFS runtime

**Layer 0** (frontier = the two seed paths):

- Sub-agent A at `/容器技术/Docker`:
  - `kms_subtree_knowledge(...)` → enumerates titles.
  - `kms_search_subtree(..., "架构")` → finds 3 titles.
  - `kms_get_knowledge` × 3 in parallel.
  - Returns 3 evidence items; next_seeds = `["/容器技术/Docker/组件", "/容器技术/Docker/网络"]`.
- Sub-agent B at `/容器技术/Kubernetes`:
  - similar; returns 2 evidence items; next_seeds = `["/容器技术/Kubernetes/组件", "/容器技术/Kubernetes/网络"]`.
- new_evidence_titles = 5 titles; new_hash = H1; stable_rounds = 0.

**Layer 1** (frontier = 4 next_seeds, capped to max_per_layer=4 → all 4):

- 4 parallel sub-agents.
- Combined: +2 new evidence items, both already-mentioned titles (overlap with layer 0).
- new_evidence_titles = 2 titles; new_hash = H2; since H2 != H1, stable_rounds = 0.

**Layer 2** (frontier = seeds from layer 1; some overlap with visited → pruned):

- 2 sub-agents (e.g. networking sub-trees).
- +0 new evidence items.
- new_evidence_titles empty → fall to `frontier_empty` branch (no advance).

**Termination**: frontier empty after pruning. `termination_reason = "frontier_empty"`. (If layer 2 had produced new evidence and layers 1, 2, 3 all produced the same hash, the BFS would have stopped at "evidence_stable" instead.)

### Orchestrator final answer (BFS agent's last LLM turn, no tool call)

```
Two container technologies, two architecture profiles:

## Docker
- Client-Server架构: ...  [excerpt from "Docker · 架构"]
- 镜像分层: ...  [excerpt from "Docker · 核心概念"]
- 容器运行时: ...  [excerpt from "Docker · 组件"]
- Source: /容器技术/Docker (3 knowledge entries)

## Kubernetes
- 控制平面架构: ...  [excerpt from "Kubernetes · 架构"]
- Pod模型: ...  [excerpt from "Kubernetes · 核心概念"]
- 声明式配置: ...  [excerpt from "Kubernetes · 设计理念"]
- Source: /容器技术/Kubernetes (2 knowledge entries)

## 对比
Both technologies use container isolation, but Docker focuses on
single-host container management, while Kubernetes provides cluster-level
orchestration with self-healing, scaling, and service discovery. ...
```

Every claim cites a knowledge title from the evidence bag.

---

## 10. Edge cases and failure modes

| Case | Behavior |
|---|---|
| Seed path does not resolve to any group | Silently dropped at seed-resolution time. If all seeds fail, `termination_reason = "seed_resolution"` and evidence is empty. The orchestrator is expected to have inspected seeds via `kms_local` first, so this is rare. |
| Sub-agent returns malformed JSON | The sub-agent's `error` field is set; the sub-agent is treated as having contributed 0 evidence and 0 next_seeds. The BFS continues with the other sub-agents' contributions. If the error happens to the only sub-agent in a layer, that layer is empty; the next iteration evaluates termination as normal. |
| Sub-agent calls a tool that escapes its scope | The tools are read-only KMS tools, so there is no data-mutation risk. The sub-agent may return evidence from outside the subtree, which is logged in `notes` and still admitted to the evidence bag (the sub-agent's `relevance_reason` is the contract, not the path). |
| Sub-agent exceeds 8 tool calls | Force-terminated; warning returned in `notes`. |
| Frontier explodes (next_seeds wide) | Capped at `max_per_layer` per layer. Excess seeds are dropped silently. |
| Evidence loop (sub-agent keeps returning the same titles) | The hash-based stability check catches it in `stable_k` layers. |
| Two seeds resolve to the same node | First one consumed; the second is silently skipped via `visited`. |
| Empty KMS database | `kms_local('/')` returns an empty view; orchestrator's seed pick is empty; `kms_bfs_dispatch` returns `seed_resolution` with empty evidence; orchestrator reports "no knowledge found". |
| `kms_bfs_dispatch` called with empty `seed_paths` | Tool returns `seed_resolution` immediately, no sub-agents dispatched. |

---

## 11. Test plan

### 11.1 Unit tests (in `agent-bfs`)

- `BfsRuntime` synthetic fixtures:
  - linear tree (root → A → B → C), evidence only at C; verify 3-layer fan-out reaches C and terminates with `evidence_stable` or `frontier_empty` (whichever fires first).
  - wide tree (root → 6 children), evidence spread across all 6; verify `max_per_layer=4` cap prunes correctly.
  - empty KMS; verify `seed_resolution`.
  - identical evidence across 3 layers; verify `evidence_stable` after `stable_k=2`.
  - depth cap hit (deep chain, `max_depth=2`); verify termination.
  - budget cap hit; verify termination.
- `SubAgentRunner`:
  - happy path: synthetic tool responses → valid JSON → EvidenceItem list.
  - malformed JSON: returns parse error in `error`, evidence empty.
  - 8-call cap: 9th call never happens.

### 11.2 Integration tests

- `kms_bfs_dispatch` end-to-end on a real (in-memory) KMS populated with a small knowledge tree.
- `agent-bfs` full agent loop: orchestrator LLM mocked; verify one `kms_bfs_dispatch` call, verify the synthesized final answer cites a known title from the seeded evidence.
- `kms_tui`: switch to BFS role, send a query, verify the chat panel shows a single `kms_bfs_dispatch` tool call and a final answer.

### 11.3 Manual regression

- After renaming `agent-knowledge` → `agent-dfs`, verify the existing DFS retrieval agent still works on the same queries as before.
- A known broad query that previously struggled (e.g., "compare therapies of two diseases") should now produce a richer, more-cited answer under the BFS role.

---

## 12. Implementation phases

A suggested rollout order, with each phase independently testable.

### Phase 1 — rename and structural plumbing (no behavior change)

- Rename `crates/agent-knowledge` → `crates/agent-dfs`.
  - Update `Cargo.toml` `package.name`.
  - Update workspace `Cargo.toml` `members`.
  - Rename `KnowledgeContext` → `DfsContext`; `KNOWLEDGE_RETRIEVAL_PROMPT` → `DFS_RETRIEVAL_PROMPT`; lib re-exports.
  - Update all `use agent_knowledge::...` and `kms_tui` references.
- Verify: `cargo build`, `cargo test` pass; existing DFS agent still works in TUI.

### Phase 2 — `agent-bfs` crate skeleton

- Create `crates/agent-bfs` with `Cargo.toml`, `lib.rs`, empty `context.rs`, `prompt.rs`, `bfs_runtime.rs`, `sub_prompt.rs`, `sub_runner.rs`, `types.rs`.
- Add to workspace `members`.
- Verify: `cargo build` passes (crate compiles, no consumers yet).

### Phase 3 — `BfsRuntime` core (no LLM yet)

- Implement `BfsRuntime` and `SubAgentRunner` against a **stub** that returns canned `SubAgentOutcome` values.
- Unit tests from §11.1.
- Verify: `cargo test -p agent-bfs` passes with stub.

### Phase 4 — wire real sub-agents

- Replace the stub in `SubAgentRunner` with a real agentik-core agent loop using `BFS_SUB_PROMPT` and the 8 read-only tools.
- Verify: integration test from §11.2 first bullet passes.

### Phase 5 — `kms_bfs_dispatch` tool

- Implement the tool in `crates/dendrite-tools/src/kms_tools/kms_bfs_dispatch.rs`.
- Register in `kms_tools::registrations()`. Do **not** add to `readonly_registrations()`.
- Verify: tool can be invoked via the registry in a test.

### Phase 6 — `BfsContext` and `BFS_RETRIEVAL_PROMPT`

- Implement `BfsContext` mirroring `DfsContext`.
- Author `BFS_RETRIEVAL_PROMPT` (text in §5.3).
- Verify: BFS agent loop end-to-end test passes.

### Phase 7 — `kms_tui` integration

- Add BFS as a 4th role in `state.rs`, `main.rs`, and `components/agent.rs`.
- Verify: TUI switches to BFS role, accepts a query, runs the new pipeline, renders the answer.

### Phase 8 — manual regression and tuning

- Run the manual regression from §11.3.
- If any of the default caps (`max_depth=4`, `stable_k=2`, `max_per_layer=4`, `max_total_subagents=16`) feel off, tune them.

---

## 13. Configuration & defaults

| Parameter | Default | Where exposed | Tweak surface |
|---|---|---|---|
| `max_depth` | 4 | tool param | per-call override |
| `stable_k` | 2 | tool param | per-call override |
| `max_per_layer` | 4 | tool param | per-call override |
| `max_total_subagents` | 16 | tool param | per-call override |
| sub-agent tool-call cap | 8 | hard-coded in `SubAgentRunner` | source-level for now |
| sub-agent prompt | `BFS_SUB_PROMPT` | constant in `sub_prompt.rs` | source-level |
| orchestrator prompt | `BFS_RETRIEVAL_PROMPT` | constant in `prompt.rs` | source-level |
| model pool | shared with orchestrator | TUI settings | TUI settings modal (existing) |

Defaults are chosen so that the worst-case cost is bounded at `4 layers × 4 sub-agents/layer × 8 tool calls = 128 LLM tool calls per BFS` plus the orchestrator's own calls — a modest ceiling.

---

## 14. File & module layout (final)

```
crates/
├── agent-dfs/                          # renamed from agent-knowledge
│   ├── Cargo.toml                      # package.name = "agent-dfs"
│   └── src/
│       ├── lib.rs                      # pub use DfsContext, DFS_RETRIEVAL_PROMPT
│       ├── context.rs                  # DfsContext (was KnowledgeContext)
│       └── prompt.rs                   # DFS_RETRIEVAL_PROMPT
│
├── agent-bfs/                          # NEW
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                      # pub use BfsContext, BFS_RETRIEVAL_PROMPT, BfsRuntime
│       ├── context.rs                  # BfsContext (mirrors DfsContext, +1 tool)
│       ├── prompt.rs                   # BFS_RETRIEVAL_PROMPT
│       ├── bfs_runtime.rs              # BfsRuntime, BfsState, BfsConfig, run()
│       ├── sub_prompt.rs               # BFS_SUB_PROMPT
│       ├── sub_runner.rs               # SubAgentRunner, fan_out()
│       └── types.rs                    # BfsResult, BfsLayerTrace, EvidenceItem, NextSeed, ...
│
├── dendrite-tools/
│   └── src/kms_tools/
│       ├── kms_bfs_dispatch.rs         # NEW
│       └── kms_tools.rs                # registrations() updated; readonly unchanged
│
└── kms_tui/
    └── src/
        ├── main.rs                     # role construction switch (4 cases)
        ├── state.rs                    # role enum gains Bfs variant
        └── components/agent.rs         # prompt + tool selection by role
```

External dependencies added by `agent-bfs`:

- `agentik-core` (agent loop, tool types) — already vendored.
- `agentik-sdk` (tool builder types) — already vendored.
- `kms`, `corpus`, `dendrite-tools` — existing.
- `serde`, `serde_json`, `tokio`, `async-trait`, `futures` — workspace deps.
- `uuid` — workspace dep.

No new vendored crates required.

---

## 15. Open items

These are intentionally left for follow-up; not blocking this round.

- **Hybrid tree + entity-hop BFS.** A `mode` parameter on `kms_bfs_dispatch` could allow one cross-entity expansion step every N tree layers. Useful when `[[entity-name]]` links would unlock the answer; defer until user feedback indicates a need.
- **Streaming progress.** Mirror the `kms_parallel_dispatch` `ParallelProgressTx` channel so the TUI can show per-layer frontier size live. Adds complexity; not worth it while the runtime is sub-second for typical queries.
- **TUI parameter UI.** A small "BFS config" panel letting the user override `max_depth` / `stable_k` / `max_per_layer` / `max_total_subagents` per query.
- **Per-sub-agent model.** Allow the sub-agent to use a cheaper / faster model from the pool to keep costs down.
- **Cross-query caching.** Memoize sub-agent outputs by `(query, seed_path)` to avoid re-running the same exploration in subsequent queries. Risk: stale evidence; defer until cost data justifies it.
- **Sub-agent tool-call cap as a tool param.** Currently hard-coded at 8; could become a per-call override.
