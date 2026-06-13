# Dendrite

> 中文版: [README.zh-CN.md](./README.zh-CN.md)

> An AI-agent-driven knowledge management system (KMS) built on a tree-shaped index.
> Personal project · Status: in progress (as of 2026-06)

Agents continuously build, retrieve, and repair structured knowledge on a SQLite knowledge tree, driven by diagnostic feedback. The design goal is to let knowledge **extend along the axons of an index tree like neurons**, rather than pile up in flat documents.

![demo](./docs/demo.png)

---

## Table of Contents

- [Design Philosophy](#design-philosophy)
- [Three-Layer Data Model](#three-layer-data-model)
- [Diagnostic System](#diagnostic-system)
- [Three Agent Roles](#three-agent-roles)
- [Agent ↔ KMS Loop](#agent--kms-loop)
- [KMS Toolset](#kms-toolset)
- [TUI](#tui)
- [Project Layout](#project-layout)
- [Quickstart](#quickstart)
- [Configuration](#configuration)
- [Extending](#extending)
- [Known Issues & Future Work](#known-issues--future-work)

---

## Design Philosophy

The core assumption: how usable a piece of knowledge is depends not on how detailed it's written, but on **whether its position in the index tree is correct**. So the system has two co-equal design pillars:

1. **The index tree comes before the knowledge itself.** Agents must inspect and repair the index structure *before* writing knowledge. Any "I can't find a good place for this" situation is treated as a signal that the index tree needs to grow — not a reason to bypass the signal.
2. **Diagnostic feedback drives self-healing.** After every KMS write, diagnostics are re-run automatically, and the **delta in issue counts** is injected into the agent's context, keeping the agent repairing structure inside the same interaction loop.

The underlying infrastructure (agent runtime, LLM SDK, type system) is **vendored as workspace crates** under `crates/agentik-core` and `crates/corpus`, so domain crates (`kms` / `kms_tui` / `dendrite-tools` / `agent-compose` / `agent-knowledge`) stay focused on the knowledge domain. This keeps the KMS itself thin, readable, and independently testable.

---

## Three-Layer Data Model

```
Index (index-tree skeleton)
  ├── Group node (internal, has children)
  └── Knowledge node (leaf, holds one knowledge)
        └── Knowledge references ≥ 1 Entity
```

| Layer | Role | Key constraints |
|-------|------|-----------------|
| **Entity** | the subject being discussed | must have ≥1 nomenclature (language + full name + optional abbreviation) |
| **Knowledge** | a record about an entity | `aspect` (single-entity facet) or `relation` (multi-entity relation); the `entities` field lists **all** mentioned entities; content uses `[[entity-name]]` wiki-style links |
| **Index** | tree-shaped organization | sibling nodes share abstraction level and don't overlap; internal nodes hold no knowledge directly |

**Typical organization:**

```
Programming languages
├── Python
│   ├── Python · Definition           ─┐
│   ├── Python · Type system          │  Knowledge nodes
│   ├── Python · Package management ─┘
│   └── ...
├── JavaScript
│   ├── React
│   │   ├── React · Core concepts
│   │   └── React · Ecosystem
│   └── Vue.js
│       └── ...
```

Knowledge granularity is strictly enforced: **one title = one facet**. Connectors that merge multiple facets are forbidden ("Setup and configuration", "Concepts and architecture", etc. all count as violations). This rule is enforced twice: in the system prompt **and** in diagnostic rules.

---

## Diagnostic System

> Acts like a compiler type-checker — re-runs automatically after every KMS write and feeds structural violations back to the agent.

### Core types

```rust
pub enum Severity { Error, Warning, Information, Hint }

pub struct Diagnostic {
    pub code: String,                              // rule identifier
    pub code_description: Option<CodeDescription>, // URI: kms://diagnostics/{domain}/{rule}
    pub location: String,                          // path in the knowledge tree
    pub severity: Severity,
    pub message: String,
    pub suggested_actions: Vec<String>,            // fix steps; reference KMS tool names directly
}
```

### Rule catalog

#### Entity rules (`crates/kms/src/diagnostics/entity_rules.rs`)

| Rule | Code | Severity | Detects |
|------|------|----------|---------|
| `NoNomenclature` | `entity.no_nomenclature` | Error | entity has empty nomenclature vector |
| `EmptyDefinition` | `entity.empty_definition` | Warning | definition field is empty |
| `MissingZhNomenclature` | `entity.missing_zh_nomenclature` | Hint | no Chinese (ZH) nomenclature |

#### Index rules (`crates/kms/src/diagnostics/index_rules.rs`)

| Rule | Code | Severity | Detects |
|------|------|----------|---------|
| `EmptyLeaf` | `index.empty_leaf` | Warning | Group node with no children and no attached knowledge (depth > 0) |
| `ExcessiveChildren` | `index.excessive_children` | Warning | more than 6 children |
| `InconsistentPrefixes` | `index.inconsistent_prefixes` | Hint | knowledge children's entity-name prefixes (before `·`) are scattered |

#### Knowledge rules (`crates/kms/src/diagnostics/knowledge_rules.rs`)

| Rule | Code | Severity | Detects |
|------|------|----------|---------|
| `NestedKnowledge` | `knowledge.internal_nested` | Warning | content contains multiple Markdown headings (should stay flat; structure belongs in the index) |
| `VagueTitle` | `knowledge.vague_title` | Warning | title suffix contains vague keywords ("overview", "definition", "intro") |
| `OrphanKnowledge` | `knowledge.orphan` | (placeholder) | currently disabled |
| `EmptyContent` | `knowledge.empty_content` | Hint | empty content |
| `NoEntities` | `knowledge.no_entities` | Warning | linked entity list is empty |
| `BoldAsHeading` | `knowledge.bold_as_heading` | Error | `**bold**` on its own line used as a heading |
| `TitleMissingEntityPrefix` | `knowledge.title_missing_entity_prefix` | Warning | title has no `·` separator (no entity-name prefix) |

Diagnostic results render live in the TUI `Diagnostics` panel; each one carries actionable `suggested_actions` telling the agent **which** `kms_*` tool to use to fix it.

---

## Three Agent Roles

`kms_tui` runs three independent agents concurrently, switchable by `[Tab]`:

| Role | Crate | Tools | Purpose |
|------|-------|-------|---------|
| **Compose** | `agent-compose` | 25 `kms_*` tools incl. writes | knowledge **build** specialist — handles ingestion, index adjustment, structural repair |
| **Retrieval** | `agent-knowledge` | 8 read-only `kms_*` tools | read-only **retrieval** specialist; forced to use parallel tool calls; ends automatically when no tools are called |
| **Parallel** | `agent-compose::ParallelComposeContext` | Compose tools + `kms_parallel_dispatch` | **orchestration** specialist — splits a big task across N sub-agents in parallel, then reports |

### Parallel Subtree orchestration mode

When user input is a large block of text (e.g. uploading an entire technical reference book), serial single-agent processing could take hours. Parallel mode's approach:

```
User pastes a big text
    ↓
Parallel agent splits along domain boundaries
("Programming languages", "Frontend frameworks", "Databases"...)
    ↓
Calls kms_parallel_dispatch(subtasks=[
  { staging_title: "Programming languages", content: "Chapter X..." },
  { staging_title: "Frontend frameworks",    content: "Chapter Y..." },
  ...
])
    ↓
The tool spawns one independent sub-agent per subtask
  - dedicated KmsService (pointer pinned to staging area)
  - shared ModelPool
  - sub-agents are blind to each other → naturally non-conflicting
    ↓
Once all sub-agents finish, the staging subtree is merged into the
target parent (or kept under Root for manual review)
    ↓
Parallel agent reports entity / knowledge counts per subtree to the user
```

**Key design points:**

- Sub-agent prompts inherit fully from `KMS_SYSTEM_PROMPT` — behavior is identical to a single agent.
- Progress is pushed to the right-side TUI sub-panel via a `ParallelProgressTx` channel in real time.
- Database write isolation comes from per-sub-agent `KmsService` instances + a staging subtree — **not** SQLite transactions.

---

## Agent ↔ KMS Loop

The core loop of the entire system:

```
Agent issues a mutation tool call (any kms_* write tool)
    ↓
agent-compose intercepts via is_mutation_tool
    ↓
KMS executes the write
    ↓
KMS re-runs all diagnostic rules
    ↓
Diagnostics and "delta vs previous run" are injected into agent context
  - new issues → prompt agent to keep fixing
  - no change → continue normally
    ↓
Agent continues
```

**Read-only tools do NOT trigger a re-diagnosis** (avoids meaningless overhead). The list:

```rust
// crates/agent-compose/src/tools.rs
pub(crate) const READONLY_KMS_TOOLS: &[&str] = &[
    "kms_search_entity",
    "kms_navigate",
    "kms_get_entity_knowledge",
];
```

**Diagnostic → fix-suggestion** chain: each diagnostic's `suggested_actions` field writes imperative guidance like *"use `kms_update_entity` to fill the entity's definition"*, so the agent can act directly without a second reasoning step.

---

## KMS Toolset

All `kms_*` tools live under `crates/dendrite-tools/src/kms_tools/`, one file per tool, exposed to the `agentik-core` runtime via `ToolRegistration`. **25+** in total.

### Entity tools

- `kms_create_entity` / `kms_update_entity` / `kms_delete_entity`
- `kms_get_entity` / `kms_search_entity` / `kms_list_entities`
- `kms_add_nomenclature` / `kms_update_nomenclature` / `kms_delete_nomenclature`

### Knowledge tools

- `kms_create_knowledge` / `kms_update_knowledge` / `kms_delete_knowledge`
- `kms_rename_knowledge` / `kms_get_knowledge` / `kms_get_entity_knowledge`
- `kms_link_orphans` — auto-attach unreferenced knowledge to a suitable location

### Index tools

- `kms_create_index` / `kms_delete_index` / `kms_move_index` (path-based: `index_path`, `new_parent_path`) / `kms_navigate`
- `kms_move_children` — move N children into a new grouping under a target path
- `kms_merge_subtree` — merge a staging subtree into a target parent
- `kms_local` — stateless local view; preferred by the retrieval agent
- `kms_subtree_knowledge` / `kms_search_subtree`

### Orchestration

- `kms_parallel_dispatch` — only exposed to the Parallel agent

### Read-only subset

(`readonly_registrations`, used by the retrieval agent):

```
kms_search_entity, kms_navigate, kms_get_entity, kms_get_entity_knowledge,
kms_get_knowledge, kms_local, kms_subtree_knowledge, kms_search_subtree
```

---

## TUI

Built on `ratatui` + `crossterm`. Multi-panel layout, vim-style shortcuts.

| Panel | Content |
|-------|---------|
| **Tree** (top-left) | knowledge-tree navigation; expand / collapse / jump |
| **Knowledge / Entity** (top-right) | content of the selected node or its linked entities |
| **Agent** (bottom half) | conversations / tool calls / reasoning of all three agents |
| **Diagnostics** (bottom-left) | live diagnostic results with fix suggestions |

The `Agent` panel embeds a **ParallelPanel**: while the Parallel agent is running, each sub-agent's status, completion percent, and any error is shown live. Sub-agent LLM responses are **inlined** into the main panel (scroll back to trace them).

The theme system (`crates/kms_tui/src/theme/`) supports custom palettes — all styles are injected through a `Theme` trait.

### Built-in `Settings` modal

- Manage multiple LLM providers concurrently (minimax / mimo / sensenova / custom OpenAI-compatible)
- Each provider has its own `base_url` + `api_key`
- Model pool selection: chain N models from N providers as the agent's fallback chain
- Persisted to `data/settings.json`

---

## Project Layout

```
dendrite/
├── crates/
│   ├── agentik-core/        # vendored agent runtime
│   ├── corpus/              # raw-corpus management; sqlx-backed chunk store
│   │
│   ├── kms/                 # KMS core: storage, views, diagnostics, service
│   │   └── src/
│   │       ├── storage/     # SQLite schema + repository
│   │       ├── diagnostics/ # entity / index / knowledge rules
│   │       ├── service.rs   # KmsService: write ops + diagnostic orchestration
│   │       ├── view.rs      # IndexView / LocalView (read-only views)
│   │       └── language.rs  # Language enum (ZH/EN/...)
│   │
│   ├── kms_tui/             # TUI app (bin: kms-tui)
│   │   └── src/
│   │       ├── main.rs              # entry: load config, build agent, run ratatui loop
│   │       ├── components/          # tree / agent / diagnostics / settings / help
│   │       ├── input/               # keyboard events, paste handling, keymap
│   │       ├── theme/               # themes / color schemes
│   │       ├── chat.rs              # ChatMessage rendering
│   │       └── parallel_panel.rs    # Parallel agent sub-panel
│   │
│   ├── dendrite-tools/      # KMS-domain tools beyond agentik-core
│   │   └── src/
│   │       ├── kms_tools/           # 25+ kms_* tools, one file each
│   │       └── parallel_progress.rs # progress-push channel
│   │
│   ├── agent-compose/       # knowledge-build agent context (write mode)
│   │   └── src/
│   │       ├── context.rs           # KmsContext (main agent)
│   │       ├── subtree_context.rs   # SubTreeComposeContext (sub-agent)
│   │       ├── parallel_context.rs  # ParallelComposeContext (orchestrator)
│   │       ├── prompt.rs            # KMS_SYSTEM_PROMPT (core prompt)
│   │       ├── subtree_prompt.rs    # sub-agent prompt variant
│   │       ├── parallel_prompt.rs   # orchestrator prompt
│   │       ├── diagnostics.rs       # KMS Diagnostic → runtime Diagnostic
│   │       └── tools.rs             # mutation-tool classification
│   │
│   └── agent-knowledge/     # read-only retrieval agent context
│       └── src/
│           ├── context.rs   # KnowledgeContext
│           └── prompt.rs    # KNOWLEDGE_RETRIEVAL_PROMPT
│
├── data/                    # runtime data
│   ├── kms_sqlite.db        # SQLite database
│   ├── settings.json        # TUI settings (provider / model pool)
│   └── tui.log              # TUI runtime log (override via KMS_LOG_PATH)
│
├── docs/
│   └── demo.png             # README screenshot
│
├── Cargo.toml               # workspace root
└── README.md
```

### Note on dependencies

The `agentik-*` runtime crates that used to be referenced via git URLs are now **vendored as workspace members** (see commits `15e228e` and `3e413f1`). All paths resolve through `[workspace.dependencies]` in the root `Cargo.toml`, so a `cargo build` no longer touches the network for those crates.

---

## Quickstart

### Prerequisites

- **Rust 1.85+** (Edition 2024; install stable via rustup)
- **SQLite** — schema is managed automatically by sqlx; no manual setup
- An API key for any one supported LLM provider (minimax / mimo / sensenova / custom OpenAI-compatible)

### Build

```bash
git clone https://github.com/wjixiang/dendrite.git
cd dendrite
cargo build --release
```

### Run

```bash
cargo run --release --bin kms-tui
```

**First launch:** the TUI enters "needs configuration" mode because `data/settings.json` doesn't exist yet:

1. Press `S` to open the settings modal
2. Add a provider (fill in `base_url` + `api_key`)
3. Pick one or more models from that provider into the model pool
4. Save — the agent is rebuilt automatically

After that, just chat — the agent will progressively build the knowledge tree.

### Database location

- Default: `data/kms_sqlite.db`
- Override: env var `KMS_DB_PATH=/path/to/kms.db`

### Log location

- Default: `data/tui.log`
- Resolution order: `KMS_LOG_PATH` > `$XDG_DATA_HOME/kms/tui.log` > `$HOME/.local/share/kms/tui.log` > `data/tui.log`
- Level: controlled by `RUST_LOG` (default DEBUG)

---

## Configuration

### Full `data/settings.json` example

```json
{
  "providers": [
    {
      "id": "prov-18b69253390fdd3c",
      "display_name": "mimo1",
      "provider_type": "mimo",
      "api_key": "tp-xxx",
      "base_url": "https://token-plan-cn.xiaomimimo.com/anthropic"
    },
    {
      "id": "prov-18b6928a3756c36d",
      "display_name": "sensenova1",
      "provider_type": "sensenova",
      "api_key": "sk-xxx",
      "base_url": ""
    }
  ],
  "pool": [
    { "provider_id": "prov-18b69253390fdd3c", "model": "mimo-v2.5-pro" }
  ]
}
```

- `providers`: configure multiple providers side-by-side; they don't interfere
- `pool`: the actual model chain used by the agent; **at least one entry** is required to build an agent
- `provider_type`: selects the HTTP protocol path the SDK takes (`minimax` / `mimo` / `sensenova` / `custom`)
- `base_url`: empty → use the provider_type's built-in default
- **No environment-variable provider config** (only `KMS_DB_PATH` / `KMS_LOG_PATH` / `RUST_LOG` are read) — all provider config goes through the TUI form, avoiding shell-history leaks.

---

## Extending

### Add a new KMS tool

1. Create `kms_xxx.rs` under `crates/dendrite-tools/src/kms_tools/`
2. Implement `ToolRegistration::registration(svc: Arc<KmsService>) -> ToolRegistration`
3. Register it in `kms_tools.rs` `registrations()` / `readonly_registrations()`

Read-only tools should go into `readonly_registrations()` — the retrieval agent uses it, and read-only tools skip post-mutation diagnostics.

### Add a new diagnostic rule

1. Add a struct in `crates/kms/src/diagnostics/{entity,index,knowledge}_rules.rs`
2. Implement the matching trait (`EntityDiagnosticRule` / `IndexDiagnosticRule` / `KnowledgeDiagnosticRule`)
3. Push it into the `rules` vector in `runner.rs`

```rust
pub trait EntityDiagnosticRule: Send + Sync {
    fn check(&self, entity: &Entity) -> Option<Diagnostic>;
    fn name(&self) -> &str;
}
```

Diagnostic-code URI convention: `kms://diagnostics/{domain}/{rule-name}`. `code_description.href` must follow this format — keeps the door open for LSP / docs integrations later.

### Tweak agent prompts

- Main agent (Compose): `crates/agent-compose/src/prompt.rs` → `KMS_SYSTEM_PROMPT`
- Sub-agent: `crates/agent-compose/src/subtree_prompt.rs`
- Orchestrator: `crates/agent-compose/src/parallel_prompt.rs`
- Retrieval agent: `crates/agent-knowledge/src/prompt.rs` → `KNOWLEDGE_RETRIEVAL_PROMPT`

Prompts are the **most important** carrier of domain knowledge. After any change, run a regression: feed known inputs and confirm the agent still obeys the rules.

### Add a new provider type

1. Add a new variant in the provider enum inside the SDK module
2. Add a matching option to the `ProviderType` dropdown in the TUI settings form
3. If it's OpenAI-compatible, just fill in `base_url`

---

## Known Issues & Future Work

- [ ] **Parallel subtree merge timing**: currently merges in one shot after all sub-agents finish. For very large inputs (>10 sub-tasks), total latency = max(subtask), and progress can't be streamed to the TUI incrementally.
- [ ] **Diagnostic-rule precision**:
  - `MissingZhNomenclature` should become "at least one major language" in multilingual settings
  - `OrphanKnowledge` is disabled — design decision pending: must every knowledge be referenced by at least one index?
- [ ] **Schema migration**: the sqlx schema is hard-coded in `storage/database.rs` with no migration history; needs proper migrations before production
- [ ] **Observability**: TUI diagnostics is the only feedback channel; consider exporting traces to OTLP for offline analysis of agent behavior
- [ ] **Prompt versioning**: prompts are string constants with no version/changelog — regressions can't be precisely traced to a specific prompt change
- [ ] **Multi-user / authorization**: single-process single-user; sharing the knowledge base needs row-level permissions

---

## License

MIT
