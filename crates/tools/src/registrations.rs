use std::sync::Arc;
use crate::kms_tools;
use crate::lifecycle_tools::{AbortTaskTool, AttemptCompleteTool};
use crate::toolset::ToolRegistration;

pub fn lifecycle_registrations() -> Vec<ToolRegistration> {
    vec![
        ToolRegistration::from(AttemptCompleteTool),
        ToolRegistration::from(AbortTaskTool),
    ]
}

pub fn kms_registrations(svc: Arc<kms::KmsService>) -> Vec<ToolRegistration> {
    kms_tools::registrations(svc)
}
