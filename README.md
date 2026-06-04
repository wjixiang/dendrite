# Dendrite 智能体知识增量管理系统

## KMS 诊断系统

诊断子系统持续验证知识树的完整性，类似编译器的类型检查器：在 Agent 构建和维护知识树的过程中自动检测结构问题，并驱动 Agent 自主修复。

### 架构总览

诊断系统位于 `crates/kms/src/diagnostics/`，采用**领域驱动的规则 Trait 体系**。三个数据领域（Entity、Index、Knowledge）各定义独立的诊断 Trait，共享统一的 `Diagnostic` 输出类型。

```
crates/kms/src/diagnostics/
├── mod.rs              # 核心类型 (Diagnostic, Severity) + 诊断运行器
├── entity_rules.rs     # Entity 领域规则 (3 条)
├── index_rules.rs      # Index 领域规则 (2 条)
└── knowledge_rules.rs   # Knowledge 领域规则 (4 条)
```

### 核心类型

```rust
pub enum Severity {
    Error,       // 必须修复的严重问题
    Warning,     // 应当修复的结构问题
    Information, // 信息性提示
    Hint,        // 优化建议
}

pub struct Diagnostic {
    pub code: String,                         // 规则标识，如 "entity.no_nomenclature"
    pub code_description: Option<CodeDescription>, // 诊断码 URI，如 "kms://diagnostics/entity/no-nomenclature"
    pub location: String,                      // 知识树路径，如 "Root > 循环系统 > 心律失常"
    pub severity: Severity,
    pub message: String,                       // 问题描述
    pub suggested_actions: Vec<String>,        // 推荐的修复步骤
}
```

### 规则 Trait 体系

每个领域定义自己的诊断规则 Trait，规则通过 `Box<dyn XxxDiagnosticRule>` 向量化实现组合：

| 领域 | Trait | 签名 |
|------|-------|------|
| Entity | `EntityDiagnosticRule` | `check(&self, entity: &Entity) -> Option<Diagnostic>` |
| Index | `IndexDiagnosticRule` | `check(&self, node: &Index, depth: usize, location: &str, children: &[Index]) -> Option<Diagnostic>` |
| Knowledge | `KnowledgeDiagnosticRule` | `check(&self, knowledge: &Knowledge) -> Option<Diagnostic>` |

所有 Trait 要求 `Send + Sync`。

### 规则清单

#### Entity 规则

| 规则 | 代码 | 严重级别 | 检测条件 | 修复建议 |
|------|------|---------|---------|---------|
| `NoNomenclature` | `entity.no_nomenclature` | Error | 实体名称向量为空 | 实体必须至少有一个命名 |
| `EmptyDefinition` | `entity.empty_definition` | Warning | 定义字段为空字符串 | 补充实体的定义 |
| `MissingZhNomenclature` | `entity.missing_zh_nomenclature` | Hint | 缺少中文 (ZH) 命名 | 添加中文命名 |

#### Index 规则

| 规则 | 代码 | 严重级别 | 检测条件 | 修复建议 |
|------|------|---------|---------|---------|
| `EmptyLeaf` | `index.empty_leaf` | Warning | 非根 Group 节点无子节点且无知识关联 | 创建知识索引或添加子节点 |
| `ExcessiveChildren` | `index.excessive_children` | Warning | 子节点数量超过 6 | 使用 `kms_reorganize_children` 按主题分组 |

#### Knowledge 规则

| 规则 | 代码 | 严重级别 | 检测条件 | 修复建议 |
|------|------|---------|---------|---------|
| `OrphanKnowledge` | `knowledge.orphan` | Warning | 知识条目未被任何索引节点引用 | 使用 `kms_link_orphans` 链接到适当的索引节点 |
| `EmptyContent` | `knowledge.empty_content` | Hint | 内容为空字符串 | 使用 `kms_update_knowledge` 补充内容 |
| `NoEntities` | `knowledge.no_entities` | Warning | 关联实体列表为空 | 在创建时指定 entities 字段 |
| `TitleMissingEntityPrefix` | `knowledge.title_missing_entity_prefix` | Warning | 标题未遵循 `"实体名 · 方面描述"` 格式 | 标题应包含实体名前缀 |

### 诊断执行流程

入口函数 `run_diagnostics(storage: &Storage) -> Result<Vec<Diagnostic>, String>` 按顺序执行三个阶段：

1. **索引树诊断** (`run_index_diagnostics`)
   - 迭代 DFS 遍历索引树（栈实现），构建 `Uuid → 路径` 映射表
   - 对每个节点运行 Index 规则，输出问题和路径映射（供后续阶段使用）

2. **知识条目诊断** (`run_knowledge_diagnostics`)
   - 遍历所有知识条目，构建 `knowledge_id → 索引路径` 的反向映射
   - 运行 Knowledge 规则；额外通过 SQL 查询 `orphan_knowledge_titles()` 检测孤儿知识

3. **实体诊断** (`run_entity_diagnostics`)
   - 遍历所有实体，运行 Entity 规则

三阶段结果合并为单一的 `Vec<Diagnostic>` 返回。

### 与 Agent 的闭环集成

诊断系统与 Agent 循环深度集成，形成**自动修复闭环**：

```
Agent 工具调用 (KMS 写操作)
        ↓
    诊断系统重跑
        ↓
  问题数量变化？
    ├── 是 → 诊断结果注入 Agent 记忆上下文
    │         Agent 根据建议继续修复
    │             ↓
    │         下一轮工具调用 → 循环
    └── 否 → 继续正常流程
```

#### 启动注入

Agent 启动时运行一次完整诊断，将问题格式化为结构化文本注入记忆：

```
诊断发现 N 个问题：
[WARN] index.empty_leaf — Root > 循环系统 > 心律失常 — 没有子节点也没有关联知识
  → 为该节点创建知识索引，或导航到该节点后添加子节点
```

#### 变更触发

Agent 每轮工具调用后，自动检测是否包含 KMS 写操作（排除 `kms_search_entity` 和 `kms_navigate` 两个只读工具）。若检测到写操作，重跑诊断并比较问题数量：
- 问题数量变化且不为零 → 注入新的诊断报告
- 问题数量变为零 → 注入 "诊断刷新：所有问题已修复。"

#### 系统提示词

系统 Prompt 明确要求 LLM **在完成任务前必须解决所有诊断问题**，并按严重级别优先处理：ERROR > WARN > INFO > HINT。

### TUI 集成

`kms-tui` 在启动时调用 `diagnose()` 并将结果渲染到诊断面板。使用 `style_diagnostic_line` 函数根据严重级别进行颜色编码。若诊断系统本身执行失败，会构造一个 `Severity::Error` 的回退诊断显示错误信息。

### 扩展指南

添加新规则只需两步：

1. 在对应的规则文件中定义结构体并实现领域 Trait（如 `EntityDiagnosticRule`）
2. 在 `mod.rs` 的对应 `run_xxx_diagnostics` 函数中，将规则添加到 `rules` 向量中

诊断码 URI 遵循格式 `kms://diagnostics/{domain}/{rule-name}`。
