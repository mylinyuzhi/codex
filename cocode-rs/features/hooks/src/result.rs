//! Hook result types.
//!
//! After a hook executes, it produces a `HookResult` that determines how the
//! agent loop should proceed.

use std::collections::HashMap;

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

        /// Environment variables to set (from SessionStart COCODE_ENV_FILE).
        ///
        /// Instead of mutating global state with `set_var`, env vars are returned
        /// as data and propagated to ShellExecutor as an env overlay.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        env_vars: HashMap<String, String>,
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

    /// Override the permission decision for a PreToolUse hook.
    ///
    /// In Claude Code, a PreToolUse hook can return `permissionDecision: "allow"`
    /// to auto-approve a tool without user confirmation.
    PermissionOverride {
        /// The permission decision: "allow", "deny", or "ask".
        decision: String,
        /// Optional reason for the decision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    /// Emit a system message to the user (informational).
    SystemMessage {
        /// The message to display.
        message: String,
    },

    /// Replace the tool output (PostToolUse hooks only).
    ModifyOutput {
        /// The replacement tool output.
        new_output: Value,
    },

    /// Prevent the agent loop from continuing after this tool result (PostToolUse hooks only).
    ///
    /// In Claude Code v2.1.76+, a PostToolUse hook can return `{ preventContinuation: true }`
    /// to halt the agent loop after the current tool result is processed.
    /// The original tool output is preserved; only the loop continuation is affected.
    PreventContinuation {
        /// Optional reason for stopping the loop.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
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

    /// If true, suppress this hook's output from the UI.
    #[serde(default)]
    pub suppress_output: bool,
}

#[cfg(test)]
#[path = "result.test.rs"]
mod tests;
