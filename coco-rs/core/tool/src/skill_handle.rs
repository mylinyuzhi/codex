//! Skill-resolution callback trait.
//!
//! Skills were historically reached via `AgentHandle::resolve_skill`,
//! but skills and agents are distinct runtime concepts — the
//! swarm-oriented agent handle was never the right home for skill
//! expansion. Phase 7 of the agent-loop refactor plan moves skill
//! resolution behind this dedicated trait.
//!
//! # Callback pattern
//!
//! `coco-skills` owns the `SkillManager` + expansion helpers but
//! lives above `coco-tool` in the layer graph; the trait lives here
//! and is implemented by `app/query` (or another high-layer adapter)
//! to bridge the call. This is the same pattern `HookHandle`,
//! `AgentHandle`, `MailboxHandle`, etc. use.
//!
//! # Invocation shapes
//!
//! Skills can be **inline** (expand prompt text into new user
//! messages) or **forked** (spawn a subagent). The trait returns a
//! typed [`SkillInvocationResult`]; the runtime decides which shape
//! to run based on the skill definition. A stub implementation
//! ([`NoOpSkillHandle`]) is provided for test/subagent contexts
//! where skill expansion is not supported.

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;

/// Outcome of a skill invocation.
///
/// Inline skills return expanded messages that the runtime appends
/// to history (tagged with the parent tool_use_id). Forked skills
/// return the child agent's final text + metadata — the runtime
/// routes this through the same tool_result pipeline as `AgentTool`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SkillInvocationResult {
    /// Expand the skill's prompt inline as new user messages.
    Inline {
        /// Pre-rendered summary of what ran, shown to the user in
        /// the tool result. Keep short — the detailed content goes
        /// into `new_messages`.
        summary: String,
        /// Messages to append to the agent's history. These are
        /// tagged with the parent tool_use_id upstream so skill
        /// outputs group with the SkillTool call that produced
        /// them.
        new_messages: Vec<serde_json::Value>,
    },
    /// Fork a subagent to run the skill and aggregate its output.
    Forked {
        /// Branded agent identifier so status queries / transcript
        /// lookups can address the child.
        agent_id: String,
        /// Final text aggregated from the child agent.
        output: String,
    },
}

/// Callback trait for resolving a skill by name and returning the
/// invocation outcome.
///
/// All methods are async and must be cancellation-aware. Long-running
/// forked skill execution should honor the parent turn's cancellation
/// token (threaded via `ToolUseContext`).
#[async_trait]
pub trait SkillHandle: Send + Sync {
    /// Resolve `name`, expand args against the skill definition, and
    /// run it in whatever invocation mode the skill declares
    /// (inline / forked).
    ///
    /// `args` is the raw string the model passed through the
    /// `SkillTool` — the handle is responsible for parsing it into
    /// the shape each skill expects.
    async fn invoke_skill(
        &self,
        name: &str,
        args: &str,
    ) -> Result<SkillInvocationResult, SkillInvocationError>;
}

/// Shared handle type for `ToolUseContext`.
pub type SkillHandleRef = Arc<dyn SkillHandle>;

/// Failure modes for skill invocation.
///
/// Intentionally closed so callers can match exhaustively and the
/// runner can map each case onto a stable model-visible error
/// result.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SkillInvocationError {
    #[error("skill not found: {name}")]
    NotFound { name: String },
    #[error("skill is disabled: {name}")]
    Disabled { name: String },
    #[error("skill is hidden from model: {name}")]
    HiddenFromModel { name: String },
    #[error("argument expansion failed for {name}: {reason}")]
    Expansion { name: String, reason: String },
    #[error("forked skill execution failed: {reason}")]
    Forked { reason: String },
    #[error("remote skill invocation is not supported in this runtime")]
    RemoteUnsupported,
    #[error("skill runtime unavailable: {reason}")]
    Unavailable { reason: String },
}

/// No-op handle for contexts without a configured skill runtime
/// (unit tests, subagent sessions that inherit empty registries).
/// Every call returns `Unavailable`.
#[derive(Debug, Clone, Default)]
pub struct NoOpSkillHandle;

#[async_trait]
impl SkillHandle for NoOpSkillHandle {
    async fn invoke_skill(
        &self,
        _name: &str,
        _args: &str,
    ) -> Result<SkillInvocationResult, SkillInvocationError> {
        Err(SkillInvocationError::Unavailable {
            reason: "no skill runtime installed".into(),
        })
    }
}

#[cfg(test)]
#[path = "skill_handle.test.rs"]
mod tests;
