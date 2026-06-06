//! Conversion from KMS-internal diagnostics to runtime-agnostic
//! [`ContextDiagnostic`] values.

use agentik_core::context::{ContextDiagnostic, ContextSeverity};

/// Translate a batch of [`kms::Diagnostic`] values into the runtime
/// [`ContextDiagnostic`] representation.
pub(crate) fn convert_diagnostics(issues: Vec<kms::Diagnostic>) -> Vec<ContextDiagnostic> {
    issues
        .into_iter()
        .map(|d| ContextDiagnostic {
            code: d.code,
            location: d.location,
            severity: match d.severity {
                kms::Severity::Error => ContextSeverity::Error,
                kms::Severity::Warning => ContextSeverity::Warning,
                kms::Severity::Information => ContextSeverity::Information,
                kms::Severity::Hint => ContextSeverity::Hint,
            },
            message: d.message,
            suggested_actions: d.suggested_actions,
        })
        .collect()
}
