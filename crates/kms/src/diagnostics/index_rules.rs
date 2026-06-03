use crate::storage::types::{Index, TargetType};

use super::{CodeDescription, Diagnostic, Severity};

// ── Trait ────────────────────────────────────────────────────────

pub trait IndexDiagnosticRule: Send + Sync {
    fn check(
        &self,
        node: &Index,
        depth: usize,
        location: &str,
        children: &[Index],
    ) -> Option<Diagnostic>;
    fn name(&self) -> &str;
}

// ── Rules ───────────────────────────────────────────────────────

pub struct EmptyLeaf;

impl IndexDiagnosticRule for EmptyLeaf {
    fn check(
        &self,
        node: &Index,
        depth: usize,
        location: &str,
        children: &[Index],
    ) -> Option<Diagnostic> {
        if depth > 0 && node.target_type == TargetType::Group && children.is_empty() {
            Some(Diagnostic {
                code: self.name().to_string(),
                code_description: Some(CodeDescription {
                    href: "kms://diagnostics/index/empty-leaf".to_string(),
                }),
                location: location.to_string(),
                severity: Severity::Warning,
                message: "没有子节点也没有关联知识".to_string(),
                suggested_actions: vec![
                    "为该节点创建知识索引，或导航到该节点后添加子节点".to_string(),
                    "如不再需要，可考虑删除该空节点".to_string(),
                ],
            })
        } else {
            None
        }
    }

    fn name(&self) -> &str {
        "index.empty_leaf"
    }
}

pub struct DeepNesting;

impl IndexDiagnosticRule for DeepNesting {
    fn check(
        &self,
        _node: &Index,
        depth: usize,
        location: &str,
        _children: &[Index],
    ) -> Option<Diagnostic> {
        if depth > 4 {
            Some(Diagnostic {
                code: self.name().to_string(),
                code_description: Some(CodeDescription {
                    href: "kms://diagnostics/index/deep-nesting".to_string(),
                }),
                location: location.to_string(),
                severity: Severity::Information,
                message: format!("层级深度 {} 超过建议值 4", depth),
                suggested_actions: vec![
                    "将深层子节点重新组织到更高层级的分组中".to_string(),
                    "使用 kms_reorganize_children 合并或提升过深的分支".to_string(),
                ],
            })
        } else {
            None
        }
    }

    fn name(&self) -> &str {
        "index.deep_nesting"
    }
}

pub struct ExcessiveChildren;

impl IndexDiagnosticRule for ExcessiveChildren {
    fn check(
        &self,
        _node: &Index,
        _depth: usize,
        location: &str,
        children: &[Index],
    ) -> Option<Diagnostic> {
        if children.len() > 6 {
            Some(Diagnostic {
                code: self.name().to_string(),
                code_description: Some(CodeDescription {
                    href: "kms://diagnostics/index/excessive-children".to_string(),
                }),
                location: location.to_string(),
                severity: Severity::Warning,
                message: format!("有 {} 个子节点，建议重构整理", children.len()),
                suggested_actions: vec![
                    "使用 kms_reorganize_children 将子节点按主题分组".to_string(),
                ],
            })
        } else {
            None
        }
    }

    fn name(&self) -> &str {
        "index.excessive_children"
    }
}
