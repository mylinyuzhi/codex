//! Queue types and interrupt constants for user input during agent execution.
//!
//! Queued commands (Enter while streaming) are consumed once in the agent
//! loop and injected as steering system-reminders that ask the model to
//! address each message (consume-then-remove pattern).
//!
//! Interrupt message constants match the convention from Claude Code:
//! synthetic tool_result messages with `is_error: true` are generated
//! for tool calls that were interrupted, keeping the model's causal chain
//! intact.

use serde::Deserialize;
use serde::Serialize;

/// Interrupt message injected as a tool_result for interrupted tool calls.
pub const INTERRUPTED_BY_USER: &str = "[Request interrupted by user]";

/// Interrupt message injected when tools were in progress.
pub const INTERRUPTED_FOR_TOOL_USE: &str = "[Request interrupted by user for tool use]";

/// A queued command (Enter during streaming).
///
/// These commands are shown in the UI and consumed once in the agent loop
/// as steering system-reminders (consume-then-remove pattern).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserQueuedCommand {
    /// Unique identifier for this command.
    pub id: String,
    /// The prompt/command text.
    pub prompt: String,
    /// Timestamp when queued (Unix milliseconds).
    pub queued_at: i64,
}

impl UserQueuedCommand {
    /// Create a new queued command.
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            prompt: prompt.into(),
            queued_at: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Get a preview of the command (first N chars).
    pub fn preview(&self, max_len: usize) -> String {
        if self.prompt.len() <= max_len {
            self.prompt.clone()
        } else {
            format!("{}...", &self.prompt[..max_len])
        }
    }
}

#[cfg(test)]
#[path = "queue.test.rs"]
mod tests;
