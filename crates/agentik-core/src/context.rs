use async_trait::async_trait;
use uuid::Uuid;

use crate::toolset::ToolRegistration;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSnapshot(Uuid);

impl ContextSnapshot {
    pub fn new(uuid: Uuid) -> Self {
        Self(uuid)
    }

    pub fn inner(&self) -> Uuid {
        self.0
    }
}

#[async_trait]
pub trait AgentContext: Send + Sync {
    async fn on_startup_location(&self) -> Result<Option<String>, String>;

    async fn on_startup_diagnostics(&self) -> Result<Vec<ContextDiagnostic>, String>;

    async fn take_snapshot(&self) -> Result<ContextSnapshot, String>;

    fn is_mutation_tool(&self, tool_name: &str) -> bool;

    async fn on_mutation_diagnostics(&self) -> Result<Vec<ContextDiagnostic>, String>;

    async fn on_snapshot_change(
        &self,
        before: &ContextSnapshot,
        after: &ContextSnapshot,
    ) -> Result<Option<String>, String>;

    fn system_prompt_section(&self) -> String;

    fn tool_registrations(&self) -> Vec<ToolRegistration>;
}

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