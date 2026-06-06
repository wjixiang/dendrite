#[cfg(feature = "kms")]
use std::sync::Arc;
#[cfg(feature = "kms")]
use crate::kms_tools;
#[cfg(feature = "kms")]
use crate::toolset::ToolRegistration;

#[cfg(feature = "kms")]
pub fn kms_registrations(svc: Arc<kms::KmsService>) -> Vec<ToolRegistration> {
    kms_tools::registrations(svc)
}
