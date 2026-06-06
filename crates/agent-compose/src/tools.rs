//! KMS tool classification.

/// Tools that start with the `kms_` prefix but are read-only.
///
/// These do not mutate persistent state, so they must not trigger
/// post-mutation diagnostics.
pub(crate) const READONLY_KMS_TOOLS: &[&str] = &[
    "kms_search_entity",
    "kms_navigate",
    "kms_get_entity_knowledge",
];

/// Returns `true` when a tool is a KMS mutation tool (i.e. starts with the
/// `kms_` prefix and is *not* in [`READONLY_KMS_TOOLS`]).
pub(crate) fn is_mutation_tool(tool_name: &str) -> bool {
    tool_name.starts_with("kms_") && !READONLY_KMS_TOOLS.contains(&tool_name)
}
