# Agent 优化方案

基于三轮测试（v1/v2/v3）的行为观察记录。

---

## P0: `kms_rename_knowledge` / `kms_update_knowledge` 的 `resolve` 歧义

- **现象**: `entity not found: {uuid}` 错误反复出现，Agent 放弃了 42 个 vague_title 修复
- **根因**: `resolve(name)` 搜索顺序为 entity → index → knowledge。当 index 与 knowledge 共享相同标题时（如"心血管系统 · 结构与功能概述"同时存在于两张表），`resolve` 返回 index 的 UUID。后续 `knowledge.get(index_uuid)` 触发 `StorageError::NotFound`
- **修复**: 为 `rename_knowledge` 和 `update_knowledge` 增加专用的 `resolve_knowledge` 方法，直接查找 knowledge 表，避免 index 歧义

## P1: 诊断每次全量扫描，膨胀 context

- **现象**: 每次 KMS 变更操作后，`diagnose()` 全量扫描全部 index(312+)、knowledge(214)、entity(308) 表，生成完整诊断报告注入 memory。Agent 进行大量重组时，诊断报告反复注入 context，造成 token 膨胀、compact 触发、信息丢失
- **修复方案A （推荐）**: 优化诊断系统为增量计算

## P2: `kms_create_knowledge` 的实体引用验证过严

- **现象**: Agent 创建知识时遗漏实体（如"糖皮质激素"），`create_knowledge_by_ref` 中 `resolve` 找不到实体直接报错，Agent 需要 search → create entity → 重新 create knowledge，浪费 3 个 API 轮次
- **修复**: `create_knowledge` 中对未找到的实体名自动创建空实体（definition 为空），并发出 WARN 提示。或者改为：找不到实体时跳过而不是报错，在返回结果中列出未解析的实体

## P3: 缺少 `kms_delete_index` 工具

- **现象**: Agent 无法删除多余的索引节点（如重复的分组索引、错误创建的空节点），只能通过 `reorganize_children` 或 workaround
- **修复**: 注册 `kms_delete_index` 工具，删除索引节点。底层 `IndexRepo::delete` 已存在

## P4: `kms_rename_knowledge` 的 UNIQUE 约束冲突

- **现象**: `rename_knowledge` 将引用索引的 title 更新为 new_title，但如果新标题已存在于索引表中，会触发 UNIQUE 约束
- **修复**: `rename_knowledge` 应在更新索引标题前检查冲突，如果冲突则报错并提示 Agent 先处理冲突索引

## P5: Agent 反复创建有嵌套标题的新知识

- **现象**: Agent 尝试修复 `internal_nested` 时，创建的新知识内容也包含多级标题（LLM 默认用 Markdown 标题组织），导致问题越修越多（70→73→75）
- **修复**: 在 `kms_create_knowledge` 工具层做后置检查——如果创建的知识触发了 `internal_nested` 规则，自动将内容中的嵌套标题拍平（将 `##`/`###` 替换为 `**粗体**` 前缀）
