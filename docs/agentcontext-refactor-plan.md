# AgentContext Trait 重构计划

> 目标：引入 `AgentContext` trait 解耦 agent 与 kms，使 agent crate 可独立编译，
> 同时通过 `agent-kms` companion crate 提供等价的 KMS 集成。

## 一、核心设计：`AgentContext` trait

```rust
// crates/agent/src/context.rs (新文件)

/// 上下文诊断结果（替代 kms::Diagnostic）
#[derive(Debug, Clone)]
pub struct ContextDiagnostic {
    pub code: String,
    pub location: String,
    pub severity: ContextSeverity,
    pub message: String,
    pub suggested_actions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

impl ContextSeverity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warning => "WARN",
            Self::Information => "INFO",
            Self::Hint => "HINT",
        }
    }
}

/// 状态快照，用于检测工具执行前后的状态变化
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSnapshot(Uuid);

/// Agent 上下文 trait：捕获 agent 核心循环与外部领域服务之间的所有交互。
///
/// 实现者提供：
/// - 启动状态（位置渲染、诊断结果）
/// - 变更检测（快照对比、工具分类）
/// - 变更后状态刷新（诊断、位置重新渲染）
/// - 系统提示词的领域段落
/// - 领域特定的工具注册
#[async_trait]
pub trait AgentContext: Send + Sync {
    /// 启动时提供位置信息（替代 kms.render_location()）
    async fn on_startup_location(&self) -> Result<Option<String>, String>;

    /// 启动时提供诊断结果（替代 kms.diagnose()）
    async fn on_startup_diagnostics(&self) -> Result<Vec<ContextDiagnostic>, String>;

    /// 工具执行前拍摄状态快照（替代 kms.get_pointer()）
    async fn take_snapshot(&self) -> Result<ContextSnapshot, String>;

    /// 判断工具名是否为变更工具（替代 starts_with("kms_") 检查）
    fn is_mutation_tool(&self, tool_name: &str) -> bool;

    /// 变更工具执行后重新诊断（替代 post-mutation diagnose）
    async fn on_mutation_diagnostics(&self) -> Result<Vec<ContextDiagnostic>, String>;

    /// 快照对比后返回新位置文本（替代 pointer drift + render_location）
    async fn on_snapshot_change(
        &self,
        before: &ContextSnapshot,
        after: &ContextSnapshot,
    ) -> Result<Option<String>, String>;

    /// 提供系统提示词的领域特定段落（替代 build_kms 的 130 行内容）
    fn system_prompt_section(&self) -> String;

    /// 提供领域特定的工具注册（替代 kms_registrations）
    fn tool_registrations(&self) -> Vec<ToolRegistration>;
}
```

## 二、实现步骤

### Step 1: `tools` crate feature-gate KMS 依赖

`tools` crate 必须先改，因为 `agent` 依赖 `tools`。

#### 1.1 `crates/tools/Cargo.toml`

```toml
[dependencies]
types = { version = "0.1.0", path = "../types" }
# kms 从这里移除 → 移到 optional dependencies

[dependencies.kms]
path = "../kms"
optional = true

[features]
kms = ["dep:kms"]
```

#### 1.2 `crates/tools/src/lib.rs`

```rust
pub mod lifecycle_tools;
pub mod error;
pub mod executor;
pub mod function;
pub mod toolset;
pub mod registry;

#[cfg(feature = "kms")]
pub mod kms_tools;

#[cfg(feature = "kms")]
pub mod registrations;

// lifecycle_registrations 始终可用（无 KMS 依赖）
pub use lifecycle_tools::lifecycle_registrations;
```

#### 1.3 `crates/tools/src/lifecycle_tools.rs`

在文件末尾添加（从 `registrations.rs` 搬入）：

```rust
use crate::toolset::ToolRegistration;

pub fn lifecycle_registrations() -> Vec<ToolRegistration> {
    vec![
        ToolRegistration::from(AttemptCompleteTool),
        ToolRegistration::from(AbortTaskTool),
    ]
}
```

#### 1.4 `crates/tools/src/registrations.rs`

- 删除 `lifecycle_registrations()` 函数（已搬到 lifecycle_tools.rs）
- 整个文件用 `#[cfg(feature = "kms")]` 包裹，或仅包裹 `kms_registrations()`

---

### Step 2: 定义 trait + 类型

#### 2.1 新建 `crates/agent/src/context.rs`

内容如上方「一、核心设计」所示，另外包含从 `agent.rs` 搬来的 `format_diagnostics` 函数：

```rust
pub fn format_diagnostics(issues: &[ContextDiagnostic]) -> String {
    let mut lines = vec![format!("诊断发现 {} 个问题：", issues.len())];
    for d in issues {
        lines.push(format!(
            "[{}] {} — {} — {}",
            d.severity.label(),
            d.code,
            d.location,
            d.message
        ));
        for action in &d.suggested_actions {
            lines.push(format!("  → {}", action));
        }
    }
    lines.join("\n")
}
```

#### 2.2 `crates/agent/src/lib.rs`

```rust
pub mod context;
pub use context::{AgentContext, ContextDiagnostic, ContextSeverity, ContextSnapshot};
```

#### 2.3 `crates/agent/Cargo.toml`

```toml
# 移除此行：
# kms = { path = "../kms" }
# rusqlite 保留（agent 自身的 sqlite_storage 使用）
```

---

### Step 3: 重构 `agent.rs` 使用 trait

`crates/agent/src/agent.rs` 中所有 KMS 耦合点的逐行替换：

| 原代码 | 替换为 |
|--------|--------|
| `use kms::Diagnostic;` (L13) | `use crate::context::{AgentContext, ContextDiagnostic, ContextSnapshot, format_diagnostics};` |
| `READONLY_KMS_TOOLS` 常量 (L34-39) | **删除** |
| `fn format_diagnostics(issues: &[Diagnostic])` (L41-56) | **删除**（已搬到 context.rs） |
| `kms: Arc<kms::KmsService>` (L81) | `ctx: Arc<dyn AgentContext>` |
| `self.kms.render_location().await` (L172) | `self.ctx.on_startup_location().await` |
| `self.kms.diagnose().await` (L176) | `self.ctx.on_startup_diagnostics().await` |
| `self.kms.get_pointer().await` (L267) | `self.ctx.take_snapshot().await` |
| `tc.name.starts_with("kms_") && !READONLY_KMS_TOOLS.contains(...)` (L313-314) | `self.ctx.is_mutation_tool(&tc.name)` |
| `self.kms.diagnose().await` (L317) | `self.ctx.on_mutation_diagnostics().await` |
| `self.kms.get_pointer().await != pointer_before` (L333) | 改为再次 `take_snapshot`，然后 `on_snapshot_change(before, after)` |

具体替换代码片段：

**启动钩子（L172-182）：**
```rust
if let Ok(Some(location)) = self.ctx.on_startup_location().await {
    self.memory.remember(Message::user(location))?;
}

if let Ok(issues) = self.ctx.on_startup_diagnostics().await {
    self.last_diagnostic_count = issues.len();
    if !issues.is_empty() {
        self.memory.remember(Message::user(format_diagnostics(&issues)))?;
    }
}
```

**工具执行后（L267 + L312-337）：**
```rust
let snapshot_before = self.ctx.take_snapshot().await;

// ... 工具执行 ...

let has_mutation = toolcalls.iter().any(|tc| self.ctx.is_mutation_tool(&tc.name));
if has_mutation {
    if let Ok(issues) = self.ctx.on_mutation_diagnostics().await {
        if issues.len() != self.last_diagnostic_count {
            self.last_diagnostic_count = issues.len();
            if !issues.is_empty() {
                self.memory.remember(Message::user(format_diagnostics(&issues)))?;
            } else {
                self.memory.remember(Message::user(String::from("诊断刷新：所有问题已修复。")))?;
            }
        }
    }
}

// ...

let snapshot_after = self.ctx.take_snapshot().await;
if let Ok(Some(location)) = self.ctx.on_snapshot_change(&snapshot_before, &snapshot_after).await {
    self.memory.remember(Message::user(location))?;
}
```

**系统提示词构建（L362-365）：**
```rust
let system_prompt = system_prompt_builder::SystemPromptBuilder::default()
    .build_identity()
    .with_extra_section(self.ctx.system_prompt_section())
    .build_tooluse_guidence()
    .parse();
```

---

### Step 4: 重构 `system_prompt_builder.rs`

`crates/agent/src/prompt/system_prompt_builder.rs`：

- `kms: String` 字段重命名为 `extra_section: String`
- 删除 `build_kms()` 方法（130 行内容搬到 `agent-kms`）
- 新增：
  ```rust
  pub fn with_extra_section(mut self, section: String) -> Self {
      self.extra_section = section;
      self
  }
  ```
- `parse()` 中 `self.kms` → `self.extra_section`

---

### Step 5: 重构 `agent_builder.rs`

`crates/agent/src/agent_builder.rs` 完整改写：

```rust
use crate::context::AgentContext;

pub struct AgentBuilder {
    model_pool: Option<Arc<ModelPool>>,
    ctx: Option<Arc<dyn AgentContext>>,  // 替代 kms + kms_db_path
    config: AgentConfig,
    storage: Option<Arc<dyn AgentSnapshotStorage>>,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            model_pool: None,
            ctx: None,
            config: AgentConfig::default(),
            storage: None,
        }
    }

    // 替代 with_kms() + with_kms_path()
    pub fn with_context(mut self, ctx: Arc<dyn AgentContext>) -> Self {
        self.ctx = Some(ctx);
        self
    }

    pub async fn build(self) -> Result<Agent, AgentError> {
        let model_pool = self.model_pool
            .ok_or_else(|| AgentError::MissingConfig("model_pool".to_string()))?;
        let ctx = self.ctx
            .ok_or_else(|| AgentError::MissingConfig("context".to_string()))?;

        let mut toolset = Toolset::default();
        toolset.register_all(tools::lifecycle_registrations())?;  // 无需 features
        toolset.register_all(ctx.tool_registrations())?;

        Ok(Agent {
            id: Uuid::new_v4(),
            model_pool,
            memory: Memory::new(),
            toolset,
            lifecycle: AgentLifecycle::new(),
            config: self.config,
            storage: self.storage,
            token_budget: TokenBudget::default(),
            ctx,                        // 替代 kms
            last_diagnostic_count: 0,
            event_tx: None,
            current_model_name: None,
        })
    }
}
```

---

### Step 6: 创建 `agent-kms` crate

#### 6.1 `crates/agent-kms/Cargo.toml`

```toml
[package]
name = "agent-kms"
version = "0.1.0"
edition = "2024"

[dependencies]
agent = { path = "../agent" }
kms = { path = "../kms" }
tools = { path = "../tools", features = ["kms"] }
async-trait = { workspace = true }
uuid = { version = "1.12", features = ["v4"] }
```

#### 6.2 `crates/agent-kms/src/lib.rs`

```rust
use std::sync::Arc;
use agent::context::{AgentContext, ContextDiagnostic, ContextSeverity, ContextSnapshot};
use tools::ToolRegistration;
use async_trait::async_trait;
use uuid::Uuid;

/// 只读 KMS 工具（从 agent.rs 搬入）
const READONLY_KMS_TOOLS: &[&str] = &[
    "kms_search_entity",
    "kms_navigate",
    "kms_get_entity_knowledge",
];

/// KMS 专用的 AgentContext 实现
pub struct KmsContext {
    kms: Arc<kms::KmsService>,
}

impl KmsContext {
    pub fn new(kms: Arc<kms::KmsService>) -> Self {
        Self { kms }
    }

    /// 便捷构造器：从数据库路径创建 KmsService + KmsContext
    pub async fn from_path(db_path: &str) -> Result<Self, String> {
        let svc = kms::KmsService::new(db_path).await?;
        Ok(Self::new(Arc::new(svc)))
    }
}

/// kms::Diagnostic → ContextDiagnostic 转换
fn convert_diagnostics(issues: Vec<kms::Diagnostic>) -> Vec<ContextDiagnostic> {
    issues.into_iter().map(|d| ContextDiagnostic {
        code: d.code,
        location: d.location,
        severity: match d.severity {
            kms::Severity::Error => ContextSeverity::Error,
            kms::Severity::Warning => ContextSeverity::Warning,
            kms::Severity::Information => ContextSeverity::Information,
            kms::Severity::Hint => ContextSeverity::Hint,
        },
        message: d.message,
        suggested_actions: d.suggested_actions,
    }).collect()
}

#[async_trait]
impl AgentContext for KmsContext {
    async fn on_startup_location(&self) -> Result<Option<String>, String> {
        let location = self.kms.render_location().await?;
        Ok(Some(location))
    }

    async fn on_startup_diagnostics(&self) -> Result<Vec<ContextDiagnostic>, String> {
        let issues = self.kms.diagnose().await?;
        Ok(convert_diagnostics(issues))
    }

    async fn take_snapshot(&self) -> Result<ContextSnapshot, String> {
        Ok(ContextSnapshot(self.kms.get_pointer().await))
    }

    fn is_mutation_tool(&self, tool_name: &str) -> bool {
        tool_name.starts_with("kms_") && !READONLY_KMS_TOOLS.contains(&tool_name)
    }

    async fn on_mutation_diagnostics(&self) -> Result<Vec<ContextDiagnostic>, String> {
        let issues = self.kms.diagnose().await?;
        Ok(convert_diagnostics(issues))
    }

    async fn on_snapshot_change(
        &self,
        before: &ContextSnapshot,
        after: &ContextSnapshot,
    ) -> Result<Option<String>, String> {
        if before != after {
            let location = self.kms.render_location().await?;
            Ok(Some(location))
        } else {
            Ok(None)
        }
    }

    fn system_prompt_section(&self) -> String {
        KMS_SYSTEM_PROMPT.to_string()
    }

    fn tool_registrations(&self) -> Vec<ToolRegistration> {
        tools::kms_registrations(self.kms.clone())
    }
}

/// KMS 系统提示词（从 system_prompt_builder.rs build_kms() 搬入，内容完全相同）
const KMS_SYSTEM_PROMPT: &str = concat!(
    "## 知识管理系统 (KMS)\n",
    "### 设计理念\n",
    "本系统的核心能力是通过**树状索引结构**实现知识的无限拓展，同时保证高度的知识整体性与组织性。\n",
    // ... 完整的 130 行内容，从当前 system_prompt_builder.rs L13-141 复制粘贴 ...
);
```

---

### Step 7: 更新消费者

#### 7.1 `Cargo.toml`（workspace root）

```toml
members = [
    "crates/types",
    "crates/llm_api",
    "crates/tools",
    "crates/agent",
    "crates/agent-kms",   # 新增
    "crates/kms",
    "crates/kms_tui",
]
```

#### 7.2 `crates/kms_tui/Cargo.toml`

```toml
# 新增
agent-kms = { path = "../agent-kms" }
```

#### 7.3 `crates/kms_tui/src/main.rs` (~L155)

```rust
use agent_kms::KmsContext;

let agent = agent::Agent::builder()
    .with_model_pool(Arc::new(pool))
    .with_context(Arc::new(KmsContext::new(svc.clone())))
    .build()
    .await
    .map_err(|e| e.to_string())?;
```

#### 7.4 `crates/kms_tui/src/input.rs` (~L468)

```rust
use agent_kms::KmsContext;

let new_agent = agent::Agent::builder()
    .with_model_pool(Arc::new(pool))
    .with_context(Arc::new(KmsContext::new(app.svc.clone())))
    .build()
    .await
    .map_err(|e| e.to_string())?;
```

#### 7.5 测试文件

三个集成测试都需要改用 `KmsContext`：

- `crates/agent/tests/basic_agent.rs` (~L35)
- `crates/agent/tests/kms_agent_flow.rs` (~L23)
- `crates/agent/tests/import_textbook.rs` (~L253)

模式：
```rust
use agent_kms::KmsContext;

// with_kms_path → KmsContext::from_path
let ctx = Arc::new(KmsContext::from_path("data/kms_sqlite.db").await.unwrap());
let mut agent = Agent::builder()
    .with_model_pool(Arc::new(pool))
    .with_context(ctx)
    .build()
    .await
    .unwrap();

// with_kms(Arc::new(kms)) → KmsContext::new(Arc::new(kms))
let ctx = Arc::new(KmsContext::new(kms));
```

#### 7.6 `crates/agent/src/agent.rs` 内的单元测试

`get_test_agent()` (~L520) 直接创建了 `kms::KmsService`。两种处理方式：

**方案 A**：改签名接受 `Arc<dyn AgentContext>`：
```rust
async fn get_test_agent(mock_tool_call: Vec<ToolUse>, ctx: Arc<dyn AgentContext>) -> Agent {
    let model_pool = mock_model_pool("test_model", mock_tool_call);
    Agent::builder()
        .with_model_pool(Arc::new(model_pool))
        .with_context(ctx)
        .build()
        .await
        .expect("failed to create test agent")
}
```

**方案 B**：将整个 `#[cfg(test)] mod tests` 移至 `crates/agent-kms/tests/`

推荐方案 A，因为大部分测试逻辑（mock model pool、echo tool、lifecycle）不依赖 KMS。

---

## 三、验证清单

| 检查项 | 命令 |
|--------|------|
| `agent` crate 无 KMS 引用 | `grep -rn "kms::" crates/agent/ && echo "FAIL" \| \| echo "PASS"` |
| `agent` crate 编译 | `cargo check -p agent` |
| `agent-kms` crate 编译 | `cargo check -p agent-kms` |
| `tools` 无 feature 也可编译 | `cargo check -p tools` |
| `tools` 有 feature 也可编译 | `cargo check -p tools --features kms` |
| `kms_tui` 编译 | `cargo check -p kms_tui` |
| 全 workspace 编译 | `cargo check --workspace` |
| 全 workspace 测试 | `cargo test --workspace` |
| 无 KMS 时构建 Agent | 在测试中创建 `NoopContext` 实现，验证 Agent 可不依赖 KMS 运行 |

## 四、受影响文件一览

| 文件 | 操作 |
|------|------|
| `crates/tools/Cargo.toml` | 改：kms optional + features |
| `crates/tools/src/lib.rs` | 改：cfg-gate + re-export lifecycle_registrations |
| `crates/tools/src/registrations.rs` | 改：移出 lifecycle_registrations，整体 cfg gate |
| `crates/tools/src/lifecycle_tools.rs` | 改：加入 lifecycle_registrations() |
| `crates/agent/Cargo.toml` | 改：移除 kms 依赖 |
| `crates/agent/src/context.rs` | **新建**：trait + types + format_diagnostics |
| `crates/agent/src/lib.rs` | 改：export context module |
| `crates/agent/src/agent.rs` | 改：全部 KMS 调用 → ctx 方法 |
| `crates/agent/src/prompt/system_prompt_builder.rs` | 改：kms → extra_section |
| `crates/agent/src/agent_builder.rs` | 改：with_context 替换 with_kms |
| `crates/agent-kms/Cargo.toml` | **新建** |
| `crates/agent-kms/src/lib.rs` | **新建**：KmsContext 实现 |
| `Cargo.toml` (workspace) | 改：加 agent-kms member |
| `crates/kms_tui/Cargo.toml` | 改：加 agent-kms 依赖 |
| `crates/kms_tui/src/main.rs` | 改：用 KmsContext |
| `crates/kms_tui/src/input.rs` | 改：用 KmsContext |
| `crates/agent/tests/*.rs` | 改：用 KmsContext |
