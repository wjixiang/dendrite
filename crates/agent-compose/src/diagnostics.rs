//! Conversion from KMS-internal diagnostics to JSON values
//! for injection into the context snapshot.

/// Translate a batch of [`kms::Diagnostic`] values into a JSON value
/// suitable for embedding in a [`ContextSnapshot`][`agentik_core::context::ContextSnapshot`].
pub(crate) fn convert_diagnostics_to_json(issues: Vec<kms::Diagnostic>) -> serde_json::Value {
    serde_json::json!(issues
        .into_iter()
        .map(|d| serde_json::json!({
            "code": d.code,
            "location": d.location,
            "severity": match d.severity {
                kms::Severity::Error => "ERROR",
                kms::Severity::Warning => "WARN",
                kms::Severity::Information => "INFO",
                kms::Severity::Hint => "HINT",
            },
            "message": d.message,
            "suggested_actions": d.suggested_actions,
        }))
        .collect::<Vec<_>>())
}
