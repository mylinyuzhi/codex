//! Hook event types.
//!
//! Defines the lifecycle points at which hooks can be triggered.

use serde::Deserialize;
use serde::Serialize;

/// Type of hook event that triggers hook execution.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEventType {
    /// Before a tool is used.
    PreToolUse,
    /// After a tool completes successfully.
    PostToolUse,
    /// After a tool use fails.
    PostToolUseFailure,
    /// When the user submits a prompt.
    UserPromptSubmit,
    /// When a session starts.
    SessionStart,
    /// When a session ends.
    SessionEnd,
    /// When the agent stops.
    Stop,
    /// When a sub-agent starts.
    SubagentStart,
    /// When a sub-agent stops.
    SubagentStop,
    /// Before context compaction occurs.
    PreCompact,
    /// A notification event (informational, no blocking).
    Notification,
    /// When a permission is requested.
    PermissionRequest,
}

impl HookEventType {
    /// Returns the string representation of this event type.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PreToolUse => "pre_tool_use",
            Self::PostToolUse => "post_tool_use",
            Self::PostToolUseFailure => "post_tool_use_failure",
            Self::UserPromptSubmit => "user_prompt_submit",
            Self::SessionStart => "session_start",
            Self::SessionEnd => "session_end",
            Self::Stop => "stop",
            Self::SubagentStart => "subagent_start",
            Self::SubagentStop => "subagent_stop",
            Self::PreCompact => "pre_compact",
            Self::Notification => "notification",
            Self::PermissionRequest => "permission_request",
        }
    }
}

impl std::fmt::Display for HookEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<HookEventType> for cocode_protocol::HookEventType {
    fn from(event: HookEventType) -> Self {
        match event {
            HookEventType::PreToolUse => Self::PreToolUse,
            HookEventType::PostToolUse => Self::PostToolUse,
            HookEventType::PostToolUseFailure => Self::PostToolUseFailure,
            HookEventType::UserPromptSubmit => Self::UserPromptSubmit,
            HookEventType::SessionStart => Self::SessionStart,
            HookEventType::SessionEnd => Self::SessionEnd,
            HookEventType::Stop => Self::Stop,
            HookEventType::SubagentStart => Self::SubagentStart,
            HookEventType::SubagentStop => Self::SubagentStop,
            HookEventType::PreCompact => Self::PreCompact,
            HookEventType::Notification => Self::Notification,
            HookEventType::PermissionRequest => Self::PermissionRequest,
        }
    }
}

#[cfg(test)]
#[path = "event.test.rs"]
mod tests;
