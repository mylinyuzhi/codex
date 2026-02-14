//! Command handler: executes an external process.
//!
//! The command receives the full `HookContext` as JSON on stdin and is expected to
//! return a JSON response on stdout. The response can be:
//!
//! 1. A `HookResult` (legacy format with `action` tag):
//!    ```json
//!    { "action": "continue" }
//!    { "action": "reject", "reason": "..." }
//!    { "action": "modify_input", "new_input": {...} }
//!    { "action": "continue_with_context", "additional_context": "..." }
//!    ```
//!
//! 2. A `HookOutput` (Claude Code v2.1.7 format):
//!    ```json
//!    { "continue_execution": true }
//!    { "continue_execution": false, "stop_reason": "..." }
//!    { "continue_execution": true, "updated_input": {...} }
//!    { "continue_execution": true, "additional_context": "..." }
//!    ```
//!
//! Environment variables set for the command:
//! - `CLAUDE_PROJECT_DIR` - Project root (working directory)
//! - `CLAUDE_SESSION_ID` - Current session ID
//! - `HOOK_EVENT` - Event type name (e.g., "pre_tool_use")
//! - `HOOK_TOOL_NAME` - Tool name (if applicable, otherwise empty)
//!
//! Exit code semantics (matching Claude Code v2.1.7):
//! - Exit code 0: Success, parse stdout for response
//! - Exit code 2: Block the action, stderr becomes the rejection reason
//! - Any other non-zero: Error (logged but not blocking, returns `Continue`)

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use tracing::debug;
use tracing::warn;

use crate::context::HookContext;
use crate::result::HookResult;

/// Claude Code v2.1.7 compatible hook output format.
///
/// This format is an alternative to `HookResult` that external commands can return.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookOutput {
    /// Whether execution should continue. If false, the action is blocked.
    pub continue_execution: bool,

    /// Reason for blocking (used when `continue_execution` is false).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,

    /// Replacement input (used to modify tool input before execution).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<Value>,

    /// Additional context to inject into the conversation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,

    /// If true, the hook is running asynchronously.
    ///
    /// When a command returns `{ "async": true }`, it indicates that the hook
    /// has spawned a background process that will complete later. The main
    /// execution continues immediately, and the async hook's result will be
    /// delivered via system reminders when it completes.
    #[serde(default, rename = "async")]
    pub is_async: bool,

    /// Permission decision override for PreToolUse hooks.
    ///
    /// When set to "allow", auto-approves the tool without user confirmation.
    /// When set to "deny", blocks the tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<String>,

    /// Decision field for PostToolUse/Stop events.
    /// When set to "block", blocks the action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,

    /// Hook-specific structured output wrapper.
    /// Claude Code v2.1.7 wraps certain outputs in this field.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "hookSpecificOutput"
    )]
    pub hook_specific_output: Option<Value>,
}

impl HookOutput {
    /// Converts this output to a HookResult, optionally with a hook name for async results.
    pub fn into_result(self, hook_name: Option<&str>) -> HookResult {
        if self.is_async {
            // Generate a unique task ID for async hooks
            let task_id = format!("async-{}", uuid::Uuid::new_v4());
            return HookResult::Async {
                task_id,
                hook_name: hook_name.unwrap_or("unknown").to_string(),
            };
        }

        // Permission decision override (PreToolUse hooks)
        if let Some(decision) = self.permission_decision {
            return HookResult::PermissionOverride {
                decision,
                reason: self.stop_reason,
            };
        }

        // Check decision field (PostToolUse/Stop events use "block")
        if let Some(ref decision) = self.decision
            && decision == "block"
        {
            return HookResult::Reject {
                reason: self
                    .stop_reason
                    .clone()
                    .unwrap_or_else(|| "Hook blocked execution (decision: block)".to_string()),
            };
        }

        if !self.continue_execution {
            return HookResult::Reject {
                reason: self
                    .stop_reason
                    .unwrap_or_else(|| "Hook blocked execution".to_string()),
            };
        }

        if let Some(new_input) = self.updated_input {
            return HookResult::ModifyInput { new_input };
        }

        if self.additional_context.is_some() {
            return HookResult::ContinueWithContext {
                additional_context: self.additional_context,
            };
        }

        HookResult::Continue
    }
}

impl From<HookOutput> for HookResult {
    fn from(output: HookOutput) -> Self {
        output.into_result(None)
    }
}

/// Executes an external command as a hook handler.
pub struct CommandHandler;

impl CommandHandler {
    /// Runs the specified command, passing the full `HookContext` as JSON on stdin.
    ///
    /// Environment variables are set to provide context:
    /// - `CLAUDE_PROJECT_DIR` - Working directory / project root
    /// - `CLAUDE_SESSION_ID` - Current session ID
    /// - `HOOK_EVENT` - Event type (e.g., "pre_tool_use")
    /// - `HOOK_TOOL_NAME` - Tool name if applicable
    ///
    /// The process stdout is parsed as either `HookResult` (legacy) or `HookOutput`
    /// (Claude Code v2.1.7 format). On any error the handler falls back to `Continue`.
    pub async fn execute(command: &str, args: &[String], ctx: &HookContext) -> HookResult {
        let ctx_json = match serde_json::to_string(ctx) {
            Ok(j) => j,
            Err(e) => {
                warn!("Failed to serialize hook context: {e}");
                return HookResult::Continue;
            }
        };

        debug!(command, ?args, event_type = %ctx.event_type, "Executing command hook");

        let result = tokio::process::Command::new(command)
            .args(args)
            .current_dir(&ctx.working_dir)
            // Set environment variables for the command
            .env(
                "CLAUDE_PROJECT_DIR",
                ctx.working_dir.to_string_lossy().as_ref(),
            )
            .env("CLAUDE_SESSION_ID", &ctx.session_id)
            .env("HOOK_EVENT", ctx.event_type.as_str())
            .env("HOOK_TOOL_NAME", ctx.tool_name.as_deref().unwrap_or(""))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();

        let mut child = match result {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to spawn hook command '{command}': {e}");
                return HookResult::Continue;
            }
        };

        // Write full context JSON to stdin
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            if let Err(e) = stdin.write_all(ctx_json.as_bytes()).await {
                warn!("Failed to write to hook command stdin: {e}");
            }
            drop(stdin);
        }

        let output = match child.wait_with_output().await {
            Ok(o) => o,
            Err(e) => {
                warn!("Failed to wait for hook command: {e}");
                return HookResult::Continue;
            }
        };

        if !output.status.success() {
            let exit_code = output.status.code().unwrap_or(-1);
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Exit code 2 = block the action, stderr becomes Claude's feedback
            if exit_code == 2 {
                let reason = if stderr.trim().is_empty() {
                    "Hook blocked execution (exit code 2)".to_string()
                } else {
                    stderr.trim().to_string()
                };
                debug!(command, %reason, "Hook command blocked action (exit code 2)");
                return HookResult::Reject { reason };
            }

            // Any other non-zero exit = error, logged but not blocking
            warn!(
                command,
                exit_code,
                stderr = %stderr,
                "Hook command exited with error"
            );
            return HookResult::Continue;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() {
            return HookResult::Continue;
        }

        parse_hook_response(stdout.trim())
    }
}

/// Parses hook command output, supporting both `HookResult` and `HookOutput` formats.
fn parse_hook_response(stdout: &str) -> HookResult {
    // Try parsing as HookResult first (has "action" field)
    if let Ok(result) = serde_json::from_str::<HookResult>(stdout) {
        return result;
    }

    // Try parsing as HookOutput (Claude Code v2.1.7 format with "continue_execution" field)
    if let Ok(output) = serde_json::from_str::<HookOutput>(stdout) {
        return output.into();
    }

    warn!("Failed to parse hook command output as HookResult or HookOutput");
    HookResult::Continue
}

#[cfg(test)]
#[path = "command.test.rs"]
mod tests;
