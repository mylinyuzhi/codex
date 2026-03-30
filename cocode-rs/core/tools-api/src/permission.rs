//! Permission and skill invocation types.
//!
//! Provides the [`PermissionRequester`] trait for decoupling permission
//! approval from the executor, and [`InvokedSkill`] for tracking skill usage.

use async_trait::async_trait;
use cocode_protocol::ApprovalDecision;
use cocode_protocol::ApprovalRequest;
use std::path::PathBuf;
use std::time::Instant;

pub use cocode_policy::ApprovalStore;

/// Trait for requesting user permission approval.
///
/// This trait decouples the tools crate from the executor crate,
/// allowing `WorkerPermissionQueue` (in cocode-executor) to be used
/// without creating a circular dependency.
#[async_trait]
pub trait PermissionRequester: Send + Sync {
    /// Request permission for an operation.
    ///
    /// Returns the user's three-way decision: approve once, approve similar
    /// commands (with prefix pattern), or deny.
    async fn request_permission(
        &self,
        request: ApprovalRequest,
        worker_id: &str,
    ) -> ApprovalDecision;
}

/// Information about an invoked skill.
///
/// Tracks skills that have been invoked during the session for hook cleanup
/// and system reminder injection.
#[derive(Debug, Clone)]
pub struct InvokedSkill {
    /// The skill name.
    pub name: String,
    /// When the skill was invoked.
    pub started_at: Instant,
    /// The skill's prompt content (after argument substitution).
    pub prompt_content: String,
    /// Base directory of the skill (for relative path resolution).
    pub path: Option<PathBuf>,
}
