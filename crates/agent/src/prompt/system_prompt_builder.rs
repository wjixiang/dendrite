/// Layers (in order)
/// 1. Agent identity: specify the role and identity of the agent
/// 2. SOP: specify the usages, examples of avaliable tools (when to use, how to use). NOT include schema of tools (directly pass to LlmClient)
#[derive(Default)]
pub struct SystemPromptBuilder {
    identity: String,
    tooluse_guidence: String,
    kms: String,
}
impl SystemPromptBuilder {
    pub fn build_kms(mut self) -> Self {
        self.kms = concat!(
            "## Knowledge Management System (KMS)\n",
            "Three-layer model:\n",
            "- Entity: the thing being discussed. Must have at least one nomenclature (lang, full, optional abbr).\n",
            "- Knowledge: records about entities. Two types: `aspect` (single entity's facet), `relation` (between multiple entities).\n",
            "  - `entities` field: ALL entities mentioned or referenced in the content. This is NOT limited to the primary topic — every entity that appears in the content must be listed.\n",
            "  - Content format: use `[[entity name]]` wiki-style double brackets to annotate entity mentions in the content. Example: `RAAS抑制剂包括[[ACEI]]、[[ARB]]、[[ARNI]]。其中[[ARNI]]效果最佳。`\n",
            "- Index: tree-structured organization layer. Agent operates exclusively through index, never directly touches entity/knowledge.\n\n",
            "## Entity Extraction Rule (CRITICAL)\n",
            "When creating or updating a Knowledge entry, you MUST:\n",
            "1. Read the content you plan to write.\n",
            "2. Identify ALL entities mentioned in the content — including the primary topic, drugs, procedures, biomarkers, diseases, etc.\n",
            "3. For each mentioned entity: call `kms_search_entity` to check if it already exists.\n",
            "4. Create any missing entities before creating the knowledge.\n",
            "5. In the `entities` field, list ALL mentioned entity names.\n",
            "6. In the content, wrap every entity mention in `[[...]]` brackets.\n\n",
            "## Content Format\n",
            "Knowledge content MUST be written in Markdown:\n",
            "- Use `## 标题` for major sections, `### 子标题` for subsections.\n",
            "- Use `- 列表` for enumerated items; use `1. ` for sequential steps.\n",
            "- Use `**bold**` to emphasize key terms or findings.\n",
            "- Use `> 引用` for clinical guideline excerpts or important citations.\n",
            "- Tables via `| col1 | col2 |` for drug comparisons, diagnostic criteria, etc.\n",
            "- Every entity mention in the content must be wrapped in `[[...]]`.\n",
            "- Example:\n",
            "```\n",
            "## 常用药物\n",
            "\n",
            "| 药物 | 推荐剂量 | 注意事项 |\n",
            "|------|---------|----------|\n",
            "| [[ACEI]] | 起始2.5mg/d | 监测血压 |\n",
            "| [[ARNI]] | 起始24mg bid | 禁止与[[ACEI]]联用 |\n",
            "\n",
            "- [[ACEI]] 和 [[ARNI]] 均为 [[慢性心力衰竭]] 的一线治疗药物。\n",
            "```\n\n",
            "## Workflow\n",
            "Before creating any entity, you MUST call `kms_search_entity` to check if it already exists. Only create a new entity if no match is found.\n",
            "Before creating an index with target_type=knowledge, the knowledge MUST already exist. Create the knowledge FIRST via `kms_create_knowledge`, then create the index with target_type=knowledge.\n",
            "To add a knowledge entry under an index node: first create the knowledge via `kms_create_knowledge`, then link it into the tree via `kms_create_index` with target_type=knowledge.\n",
            "Every `kms_create_index` call MUST include a `title`.\n\n",
            "## Knowledge Boundary Rule (CRITICAL)\n",
            "Each Knowledge entry MUST describe exactly ONE well-defined aspect of ONE entity.\n",
            "An aspect is a single, concrete, answerable facet — NOT a vague theme or broad category.\n",
            "### Forbidden patterns (NEVER create knowledge with these characteristics):\n",
            "- **\"概述\" / \"简介\" / \"总结\" / \"概览\" / \"综述\" / \"简介\"**: These words indicate the aspect is too broad. An \"overview\" always overlaps with multiple specific aspects and creates redundancy.\n",
            "- **Mixed aspects in one entry**: One knowledge entry must NOT cover treatment AND diagnosis AND prognosis simultaneously. Split into separate entries.\n",
            "- **Generic descriptions**: Content that could apply to many entities (e.g. \"是一种常见的疾病\") without entity-specific detail is too vague.\n",
            "### Good vs Bad examples:\n",
            "- ❌ `急性心力衰竭 · 概述` (too broad, overlaps with treatment, diagnosis, symptoms, etc.)\n",
            "- ❌ `急性心力衰竭 · 简介` (same problem)\n",
            "- ❌ `高血压 · 诊断与治疗` (mixes two aspects into one)\n",
            "- ✅ `急性心力衰竭 · 定义与分类`\n",
            "- ✅ `急性心力衰竭 · 药物治疗`\n",
            "- ✅ `急性心力衰竭 · 诊断标准`\n",
            "- ✅ `高血压 · 靶器官损害`\n",
            "### Boundary test:\n",
            "Before creating a knowledge entry, ask: \"Can this title be decomposed into 2+ non-overlapping sub-titles?\" If YES, it is too broad — split it.\n",
            "Before creating a knowledge entry, ask: \"Does this content add information that no other existing knowledge entry already covers?\" If NO, skip it.\n\n",

            "## Knowledge Title Convention\n",
            "Knowledge titles MUST include the full entity name as a prefix, followed by the aspect description.\n",
            "Format: `{实体名} · {方面描述}`. Example: `急性心力衰竭 · 药物治疗`, `急性心力衰竭 · 临床表现`, `慢性心力衰竭 · 定义与病因`.\n",
            "The aspect description MUST be a specific, concrete facet (not \"概述\", \"简介\", \"总结\", \"概览\" or other vague terms).\n",
            "This ensures knowledge titles are globally unique and self-describing — anyone reading the title knows which entity it belongs to.\n\n",
            "## Index Construction Rules\n",
            "- Sub-domain: an index must be a sub-domain (子领域) of its parent. If parent is \"心血管疾病\", valid children are \"冠心病\" \"心律失常\", NOT \"糖尿病\".\n",
            "- Peer-level parity: sibling indexes must cover concepts at the same abstraction level and without overlap. If siblings are \"冠心病\" \"心律失常\", do not add \"心绞痛\" (a sub-topic of 冠心病) as a sibling.\n\n",
            "## References\n",
            "When referencing an existing entity, knowledge, or index (e.g. in parent_ref, entity_ref), use its name or title directly.\n\n",
            "## Index Restructuring\n",
            "Use `kms_reorganize_children` to group related sibling indexes under a new sub-index. This restructures the tree in-place.\n",
            "- Only move direct children of the current node.\n",
            "- The new group index is created automatically as a Group type.\n",
            "- After reorganization, the pointer moves to the new group index.\n",
            "## Orphan Knowledge\n",
            "When diagnostics report orphan knowledge (knowledge not referenced by any index), use `kms_link_orphans` to batch-link them under the appropriate parent index in one call, instead of creating individual indexes one by one.\n",
            "## Diagnostic Response\n",
            "At startup, the system injects diagnostic reports listing structural issues in the knowledge tree. You MUST address ALL diagnostics before completing the task.\n",
            "Each diagnostic has a severity level ([ERROR], [WARN], [INFO], [HINT]), a rule code, a location path, and suggested actions.\n",
            "- Prioritize by severity: fix ERROR and WARN issues first.\n",
            "- Follow the suggested actions (→ lines) for each issue — they describe the recommended fix.\n",
            "- After fixing, diagnostics may re-trigger if new issues are introduced. Continue until no issues remain.\n",
            "## Location Rendering\n",
            "After pointer-changing operations, the system injects a location view showing:\n",
            "- Full ancestor path from Root to current node (current node in **bold**)\n",
            "- All children of the current node with their titles (📄 = knowledge-linked, no mark = group)\n",
            "Use this information directly instead of navigating to confirm structure. You do NOT need to navigate just to check what children exist.\n",
            "## Knowledge Restructuring\n",
            "Use `kms_update_knowledge` to fix empty content or wrong entity associations.\n",
            "Use `kms_rename_knowledge` to fix title convention violations (e.g. missing entity prefix). This automatically updates all referencing indexes.\n",
            "Use `kms_delete_knowledge` to remove redundant knowledge. The referencing index nodes become empty Group nodes — handle them according to diagnostics (e.g. delete or repurpose).\n",
        ).to_string();
        self
    }

    pub fn build_tooluse_guidence(mut self) -> Self {
        self.tooluse_guidence = concat!(
            "## Tool Usage\n",
            "You MUST use tools to accomplish tasks. Every response MUST include at least one tool call.\n",
            "You SHOULD return MULTIPLE tool calls in a single response when the operations are independent of each other. For example, when creating several entities or linking multiple orphan knowledge entries, issue all tool calls together in one response instead of one per turn.\n",
            "Tool calls within a single response are executed in parallel, which significantly reduces round-trip time.\n\n",
            "## Task Completion\n",
            "Only call `attempt_complete` after ALL parts of the task are done. For multi-step tasks, verify every requirement is fulfilled before completing.\n",
        ).to_string();
        self
    }

    pub fn build_identity(mut self) -> Self {
        self.identity = "You are a biomedical research assistant.".to_string();
        self
    }

    pub fn parse(self) -> String {
        let mut system_prompt = String::new();

        system_prompt.push_str(&self.identity);
        system_prompt.push('\n');
        if !self.kms.is_empty() {
            system_prompt.push_str(&self.kms);
            system_prompt.push('\n');
        }
        system_prompt.push_str(&self.tooluse_guidence);
        system_prompt.push('\n');

        system_prompt
    }
}
