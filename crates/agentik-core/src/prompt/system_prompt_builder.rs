/// Layers (in order)
/// 1. Agent identity: specify the role and identity of the agent
/// 2. SOP: specify the usages, examples of avaliable tools (when to use, how to use). NOT include schema of tools (directly pass to LlmClient)
#[derive(Default)]
pub struct SystemPromptBuilder {
    identity: String,
    tooluse_guidence: String,
    extra_section: String,
}
impl SystemPromptBuilder {
    pub fn with_extra_section(mut self, section: String) -> Self {
        self.extra_section = section;
        self
    }

    pub fn build_tooluse_guidence(mut self) -> Self {
        self.tooluse_guidence = concat!(
            "## 工具使用\n",
            "必须使用工具完成任务。每个回复必须包含至少一个工具调用。\n",
            "当操作相互独立时，应该在单个回复中返回多个工具调用。例如，创建多个实体或链接多个孤立知识条目时，应在一个回复中一起发出所有工具调用，而不是每个调用一次。\n",
            "单个回复中的工具调用并行执行，这大大减少了往返时间。\n\n",
            "## 任务完成\n",
            "只有当任务的所有部分都完成后，才调用 `attempt_complete`。对于多步骤任务，验证每个需求都已满足后再完成。\n",
        ).to_string();
        self
    }

    pub fn build_identity(mut self) -> Self {
        self.identity = "你是一位生物医学研究助手。".to_string();
        self
    }

    pub fn parse(self) -> String {
        let mut system_prompt = String::new();

        system_prompt.push_str(&self.identity);
        system_prompt.push('\n');
        if !self.extra_section.is_empty() {
            system_prompt.push_str(&self.extra_section);
            system_prompt.push('\n');
        }
        system_prompt.push_str(&self.tooluse_guidence);
        system_prompt.push('\n');

        system_prompt
    }
}
