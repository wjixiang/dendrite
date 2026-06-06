# Dendrite 智能体知识增量管理系统

Dendrite 是一个基于 AI Agent 的知识管理系统（KMS），采用树形索引结构管理知识，支持诊断驱动的自主修复。

## 特性

- **树形知识索引** - 层次化组织 Entity、Index、Knowledge
- **诊断驱动自愈** - 自动检测结构问题，驱动 Agent 自主修复
- **CLI/TUI 界面** - 交互式终端界面
- ** Anthropic Claude 集成** - 支持流式输出、工具调用、视觉识别

## 技术栈

| 组件 | 技术 |
|------|------|
| 语言 | Rust (Edition 2024) |
| 异步运行时 | Tokio |
| 数据库 | SQLite |
| LLM | Anthropic Claude API |
| TUI | ratatui + crossterm |

## 项目结构

```
dendrite/
├── crates/
│   ├── types/          # 共享类型定义 (LLM API)
│   ├── llm_api/        # Anthropic API SDK
│   ├── tools/          # 工具执行基础设施
│   ├── agent/          # Agent 核心循环
│   ├── kms/            # 知识管理系统
│   └── kms_tui/        # TUI 应用
├── Cargo.toml
└── README.md
```

## 快速开始

### 前置依赖

- Rust 1.75+
- SQLite

### 构建

```bash
cargo build --release
```

### 配置

设置环境变量：

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

### 运行

```bash
cargo run --release --bin kms_tui
```

## KMS 诊断系统

诊断子系统持续验证知识树的完整性，类似编译器的类型检查器。

### 核心类型

```rust
pub enum Severity {
    Error,       // 必须修复的严重问题
    Warning,     // 应当修复的结构问题
    Information, // 信息性提示
    Hint,        // 优化建议
}

pub struct Diagnostic {
    pub code: String,                         // 规则标识
    pub code_description: Option<CodeDescription>, // 诊断码 URI
    pub location: String,                      // 知识树路径
    pub severity: Severity,
    pub message: String,                       // 问题描述
    pub suggested_actions: Vec<String>,        // 推荐的修复步骤
}
```

### 规则清单

#### Entity 规则

| 规则 | 代码 | 严重级别 | 检测条件 |
|------|------|---------|---------|
| `NoNomenclature` | `entity.no_nomenclature` | Error | 实体名称向量为空 |
| `EmptyDefinition` | `entity.empty_definition` | Warning | 定义字段为空字符串 |
| `MissingZhNomenclature` | `entity.missing_zh_nomenclature` | Hint | 缺少中文命名 |

#### Index 规则

| 规则 | 代码 | 严重级别 | 检测条件 |
|------|------|---------|---------|
| `EmptyLeaf` | `index.empty_leaf` | Warning | Group 节点无子节点且无知识关联 |
| `ExcessiveChildren` | `index.excessive_children` | Warning | 子节点数量超过 6 |

#### Knowledge 规则

| 规则 | 代码 | 严重级别 | 检测条件 |
|------|------|---------|---------|
| `OrphanKnowledge` | `knowledge.orphan` | Warning | 知识条目未被任何索引引用 |
| `EmptyContent` | `knowledge.empty_content` | Hint | 内容为空字符串 |
| `NoEntities` | `knowledge.no_entities` | Warning | 关联实体列表为空 |
| `TitleMissingEntityPrefix` | `knowledge.title_missing_entity_prefix` | Warning | 标题未包含实体名前缀 |

### Agent 闭环集成

```
Agent 工具调用 (KMS 写操作)
        ↓
    诊断系统重跑
        ↓
   问题数量变化？
     ├── 是 → 诊断结果注入 Agent 记忆
     │         Agent 继续修复
     └── 否 → 继续正常流程
```

### 扩展指南

添加新规则只需两步：

1. 在 `crates/kms/src/diagnostics/` 对应文件中定义结构体并实现领域 Trait
2. 在 `run_xxx_diagnostics` 函数中将规则添加到 `rules` 向量中

诊断码 URI 格式：`kms://diagnostics/{domain}/{rule-name}`

## License

MIT
