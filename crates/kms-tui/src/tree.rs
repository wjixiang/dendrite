use kms::{Index, TargetType};

/// 以扁平列表形式结构存储树状索引，作为索引存储与TUI渲染的桥梁
pub struct TreeNode {
    pub id: String,
    pub title: String,
    pub target_type: TargetType,
    pub position: usize,
    pub indent: usize,
    // children: Vec<TreeNode>,
    pub expanded: bool,
}

impl TreeNode {
    pub fn from_index(value: Index, position: usize, indent: usize) -> Self {
        Self {
            id: value.id.to_string(),
            title: value.title.unwrap_or_default(),
            target_type: value.target_type,
            expanded: true,
            position,
            indent,
        }
    }
}
