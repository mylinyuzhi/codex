//! In-process teammate task state.
//!
//! TS: tasks/InProcessTeammateTask/types.ts
//!
//! Rich task state that tracks an in-process teammate's lifecycle including
//! identity, execution progress, plan approval, permission mode, messages,
//! idle state, and shutdown coordination.

use coco_types::PermissionMode;
use serde::Deserialize;
use serde::Serialize;

use super::swarm::TeammateIdentity;

/// Maximum messages kept in task.messages (UI mirror).
///
/// TS: `TEAMMATE_MESSAGES_UI_CAP = 50`
pub const TEAMMATE_MESSAGES_UI_CAP: usize = 50;

/// State for an in-process teammate task.
///
/// TS: `InProcessTeammateTaskState` in tasks/InProcessTeammateTask/types.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InProcessTeammateTaskState {
    // ── Identity ──
    /// Task type discriminator.
    #[serde(default = "default_task_type")]
    pub task_type: String,
    /// Task ID.
    pub task_id: String,
    /// Teammate identity (agentId, agentName, teamName, color, planModeRequired).
    pub identity: TeammateIdentity,

    // ── Execution ──
    /// Initial prompt/task for the teammate.
    pub prompt: String,
    /// Model override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    // ── Plan mode ──
    /// Whether teammate is awaiting plan approval from leader.
    #[serde(default)]
    pub awaiting_plan_approval: bool,

    // ── Permission mode ──
    /// Permission mode (cycled independently per teammate).
    #[serde(default)]
    pub permission_mode: PermissionMode,

    // ── State ──
    /// Error message if failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Final result output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,

    // ── Progress tracking ──
    /// Number of turns completed.
    #[serde(default)]
    pub turn_count: i32,
    /// Total input tokens consumed.
    #[serde(default)]
    pub input_tokens: i64,
    /// Total output tokens produced.
    #[serde(default)]
    pub output_tokens: i64,
    /// Number of tool uses.
    #[serde(default)]
    pub tool_use_count: i32,
    /// Start timestamp (ms since epoch).
    #[serde(default)]
    pub start_time: i64,

    // ── Conversation history (capped) ──
    /// Recent messages for zoomed view (capped at TEAMMATE_MESSAGES_UI_CAP).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<TaskMessage>,

    // ── Pending user messages ──
    /// Messages queued from the user (when viewing teammate).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_user_messages: Vec<String>,

    // ── UI rendering ──
    /// Current action verb (e.g. "Reading", "Editing").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spinner_verb: Option<String>,
    /// Past tense verb (e.g. "Read", "Edited") — shown when all idle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub past_tense_verb: Option<String>,

    // ── Lifecycle ──
    /// Whether the teammate is idle (waiting for work).
    #[serde(default)]
    pub is_idle: bool,
    /// Whether a shutdown has been requested.
    #[serde(default)]
    pub shutdown_requested: bool,

    // ── Counters for delta reporting ──
    /// Last reported tool count (for incremental updates).
    #[serde(default)]
    pub last_reported_tool_count: i32,
    /// Last reported token count.
    #[serde(default)]
    pub last_reported_token_count: i64,
}

fn default_task_type() -> String {
    "in_process_teammate".to_string()
}

/// A simplified message for the task's conversation mirror.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

impl InProcessTeammateTaskState {
    /// Create a new task state for an in-process teammate.
    pub fn new(task_id: String, identity: TeammateIdentity, prompt: String) -> Self {
        Self {
            task_type: default_task_type(),
            task_id,
            identity,
            prompt,
            model: None,
            awaiting_plan_approval: false,
            permission_mode: PermissionMode::Default,
            error: None,
            result: None,
            turn_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            tool_use_count: 0,
            start_time: std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
            messages: Vec::new(),
            pending_user_messages: Vec::new(),
            spinner_verb: None,
            past_tense_verb: None,
            is_idle: false,
            shutdown_requested: false,
            last_reported_tool_count: 0,
            last_reported_token_count: 0,
        }
    }

    /// Append a message, maintaining the cap.
    ///
    /// TS: `appendCappedMessage(prev, item)`
    pub fn append_message(&mut self, message: TaskMessage) {
        self.messages.push(message);
        if self.messages.len() > TEAMMATE_MESSAGES_UI_CAP {
            let drain_count = self.messages.len() - TEAMMATE_MESSAGES_UI_CAP;
            self.messages.drain(..drain_count);
        }
    }

    /// Elapsed time in milliseconds since the task started.
    pub fn elapsed_ms(&self) -> i64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        now - self.start_time
    }

    /// Total token count (input + output).
    pub fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens
    }
}

#[cfg(test)]
#[path = "swarm_task.test.rs"]
mod tests;
