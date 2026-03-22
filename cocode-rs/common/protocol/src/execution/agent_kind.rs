//! Agent type identification for inference requests.

use serde::Deserialize;
use serde::Serialize;
use std::fmt;

/// Type of agent making an inference request.
///
/// `AgentKind` provides context about the caller for logging, telemetry,
/// and context-specific behavior during inference.
///
/// # Example
///
/// ```
/// use cocode_protocol::execution::AgentKind;
///
/// // Main conversation agent
/// let main = AgentKind::Main;
///
/// // Subagent spawned via Task tool
/// let subagent = AgentKind::Subagent {
///     parent_session_id: "session-123".to_string(),
///     agent_type: "explore".to_string(),
/// };
///
/// // Context compaction
/// let compact = AgentKind::Compaction;
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
#[derive(Default)]
pub enum AgentKind {
    /// Main conversation agent (primary user interaction).
    #[default]
    Main,

    /// Subagent spawned via Task tool.
    Subagent {
        /// Parent session ID for tracing.
        parent_session_id: String,
        /// Agent type identifier (e.g., "explore", "plan", "code-review").
        agent_type: String,
    },

    /// Session memory extraction agent.
    ///
    /// Used for extracting and summarizing conversation memory.
    Extraction,

    /// Context compaction agent.
    ///
    /// Used for compressing context when approaching token limits.
    Compaction,
}

impl AgentKind {
    /// Create a main agent kind.
    pub fn main() -> Self {
        Self::Main
    }

    /// Create a subagent kind.
    pub fn subagent(parent_session_id: impl Into<String>, agent_type: impl Into<String>) -> Self {
        Self::Subagent {
            parent_session_id: parent_session_id.into(),
            agent_type: agent_type.into(),
        }
    }

    /// Create an extraction agent kind.
    pub fn extraction() -> Self {
        Self::Extraction
    }

    /// Create a compaction agent kind.
    pub fn compaction() -> Self {
        Self::Compaction
    }

    /// Check if this is the main agent.
    pub fn is_main(&self) -> bool {
        matches!(self, Self::Main)
    }

    /// Check if this is a subagent.
    pub fn is_subagent(&self) -> bool {
        matches!(self, Self::Subagent { .. })
    }

    /// Check if this is an extraction agent.
    pub fn is_extraction(&self) -> bool {
        matches!(self, Self::Extraction)
    }

    /// Check if this is a compaction agent.
    pub fn is_compaction(&self) -> bool {
        matches!(self, Self::Compaction)
    }

    /// Get the agent type string for telemetry/logging.
    pub fn agent_type_str(&self) -> &str {
        match self {
            Self::Main => "main",
            Self::Subagent { agent_type, .. } => agent_type,
            Self::Extraction => "extraction",
            Self::Compaction => "compaction",
        }
    }

    /// Get the parent session ID if this is a subagent.
    pub fn parent_session_id(&self) -> Option<&str> {
        match self {
            Self::Subagent {
                parent_session_id, ..
            } => Some(parent_session_id),
            _ => None,
        }
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Main => write!(f, "main"),
            Self::Subagent { agent_type, .. } => write!(f, "subagent:{agent_type}"),
            Self::Extraction => write!(f, "extraction"),
            Self::Compaction => write!(f, "compaction"),
        }
    }
}

#[cfg(test)]
#[path = "agent_kind.test.rs"]
mod tests;
