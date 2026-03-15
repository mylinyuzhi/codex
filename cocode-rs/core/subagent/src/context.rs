use serde::Deserialize;
use serde::Serialize;

/// Context linking a child subagent session back to its parent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildToolUseContext {
    /// Session ID of the parent agent that spawned this child.
    pub parent_session_id: String,

    /// Session ID assigned to the child subagent.
    pub child_session_id: String,

    /// The turn number in the parent at which the child was forked.
    pub forked_from_turn: i32,
}

#[cfg(test)]
#[path = "context.test.rs"]
mod tests;
