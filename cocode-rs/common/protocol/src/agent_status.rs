//! Agent status types for efficient status polling.
//!
//! These types enable the TUI to efficiently poll agent status without
//! receiving all events, using `tokio::sync::watch` channels.

use serde::Deserialize;
use serde::Serialize;

/// Current status of the agent.
///
/// This is broadcast via a watch channel to allow efficient polling
/// without processing the full event stream.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AgentStatus {
    /// Agent is idle, waiting for user input.
    #[default]
    Idle,

    /// Agent is streaming a response.
    Streaming {
        /// The turn ID being processed.
        turn_id: String,
    },

    /// Agent is executing tools.
    ExecutingTools {
        /// Number of tools currently pending execution.
        pending: i32,
        /// Number of tools completed in this batch.
        completed: i32,
    },

    /// Agent is waiting for user approval.
    WaitingApproval {
        /// The request ID awaiting approval.
        request_id: String,
    },

    /// Agent is performing context compaction.
    Compacting,

    /// Agent encountered an error.
    Error {
        /// Error message.
        message: String,
    },
}

impl AgentStatus {
    /// Create a new streaming status.
    pub fn streaming(turn_id: impl Into<String>) -> Self {
        Self::Streaming {
            turn_id: turn_id.into(),
        }
    }

    /// Create a new executing tools status.
    pub fn executing_tools(pending: i32, completed: i32) -> Self {
        Self::ExecutingTools { pending, completed }
    }

    /// Create a new waiting approval status.
    pub fn waiting_approval(request_id: impl Into<String>) -> Self {
        Self::WaitingApproval {
            request_id: request_id.into(),
        }
    }

    /// Create a new error status.
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }

    /// Check if the agent is busy (not idle).
    pub fn is_busy(&self) -> bool {
        !matches!(self, Self::Idle)
    }

    /// Check if the agent is streaming.
    pub fn is_streaming(&self) -> bool {
        matches!(self, Self::Streaming { .. })
    }

    /// Check if the agent is executing tools.
    pub fn is_executing_tools(&self) -> bool {
        matches!(self, Self::ExecutingTools { .. })
    }

    /// Check if the agent is waiting for approval.
    pub fn is_waiting_approval(&self) -> bool {
        matches!(self, Self::WaitingApproval { .. })
    }

    /// Check if the agent is compacting.
    pub fn is_compacting(&self) -> bool {
        matches!(self, Self::Compacting)
    }

    /// Check if the agent is in an error state.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::Streaming { turn_id } => write!(f, "Streaming (turn: {turn_id})"),
            Self::ExecutingTools { pending, completed } => {
                write!(
                    f,
                    "Executing tools ({completed}/{} done)",
                    pending + completed
                )
            }
            Self::WaitingApproval { request_id } => {
                write!(f, "Waiting approval (request: {request_id})")
            }
            Self::Compacting => write!(f, "Compacting context"),
            Self::Error { message } => write!(f, "Error: {message}"),
        }
    }
}

#[cfg(test)]
#[path = "agent_status.test.rs"]
mod tests;
