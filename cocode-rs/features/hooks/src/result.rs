//! Hook result types.
//!
//! After a hook executes, it produces a `HookResult` that determines how the
//! agent loop should proceed.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// The outcome of a single hook execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HookResult {
    /// Continue normal execution (hook did not intervene).
    Continue,

    /// Continue with additional context (e.g., from SessionStart hooks after compact).
    ContinueWithContext {
        /// Additional context to inject into the conversation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        additional_context: Option<String>,
    },

    /// Reject the current action.
    Reject {
        /// Human-readable reason for rejection.
        reason: String,
    },

    /// Modify the input before the action proceeds.
    ModifyInput {
        /// The replacement input.
        new_input: Value,
    },

    /// Hook is running asynchronously in the background.
    ///
    /// This result indicates the hook has spawned a background task and execution
    /// should continue immediately. The async hook's final result will be delivered
    /// via the `AsyncHookResponse` system reminder when it completes.
    Async {
        /// Unique identifier for the async task.
        task_id: String,
        /// Name of the hook running in the background.
        hook_name: String,
    },
}

/// A completed hook execution with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookOutcome {
    /// Name of the hook that ran.
    pub hook_name: String,

    /// The result produced by the hook.
    pub result: HookResult,

    /// Wall-clock duration of hook execution in milliseconds.
    pub duration_ms: i64,
}

#[cfg(test)]
#[path = "result.test.rs"]
mod tests;
