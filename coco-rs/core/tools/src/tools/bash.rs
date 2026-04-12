use coco_tool::BackgroundShellRequest;
use coco_tool::DescriptionOptions;
use coco_tool::Tool;
use coco_tool::ToolError;
use coco_tool::ToolProgress;
use coco_tool::ToolUseContext;
use coco_tool::ValidationResult;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

/// Default timeout: 2 minutes (120,000 ms).
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

/// Maximum timeout: 10 minutes (600,000 ms).
const MAX_TIMEOUT_MS: u64 = 600_000;

/// Maximum output size in bytes before truncation.
const MAX_OUTPUT_BYTES: usize = 100_000;

/// Bash tool -- executes shell commands via bash -c.
/// Captures stdout, stderr, and exit code.
///
/// Supports `run_in_background: true` to spawn the command as a background
/// task. The task ID is returned immediately and the model is notified
/// asynchronously when the command completes via task-notification XML.
pub struct BashTool;

#[async_trait::async_trait]
impl Tool for BashTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Bash)
    }

    fn name(&self) -> &str {
        ToolName::Bash.as_str()
    }

    fn description(&self, _input: &Value, _options: &DescriptionOptions) -> String {
        "Executes a given bash command and returns its output.".into()
    }

    fn input_schema(&self) -> ToolInputSchema {
        let mut props = HashMap::new();
        props.insert(
            "command".into(),
            serde_json::json!({
                "type": "string",
                "description": "The command to execute"
            }),
        );
        props.insert(
            "timeout".into(),
            serde_json::json!({
                "type": "number",
                "description": "Optional timeout in milliseconds (max 600000)"
            }),
        );
        props.insert(
            "description".into(),
            serde_json::json!({
                "type": "string",
                "description": "Clear description of what this command does"
            }),
        );
        props.insert(
            "run_in_background".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "Set to true to run this command in the background. You will be notified when it completes."
            }),
        );
        props.insert(
            "dangerouslyDisableSandbox".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "Set this to true to dangerously override sandbox mode and run commands without sandboxing."
            }),
        );
        ToolInputSchema { properties: props }
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        false
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    fn get_activity_description(&self, input: &Value) -> Option<String> {
        let command = input.get("command").and_then(|v| v.as_str())?;
        // Truncate long commands for display (char-safe)
        let truncated: String = command.chars().take(57).collect();
        let display = if truncated.len() < command.len() {
            format!("Running {truncated}...")
        } else {
            format!("Running {command}")
        };
        Some(display)
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input.get("command").and_then(|v| v.as_str()).is_none() {
            return ValidationResult::invalid("missing required field: command");
        }
        if let Some(timeout) = input.get("timeout").and_then(serde_json::Value::as_u64)
            && timeout > MAX_TIMEOUT_MS
        {
            return ValidationResult::invalid(format!(
                "timeout must not exceed {MAX_TIMEOUT_MS}ms"
            ));
        }
        ValidationResult::Valid
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput {
                message: "missing command".into(),
                error_code: None,
            })?;

        let timeout_ms = input
            .get("timeout")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .min(MAX_TIMEOUT_MS);

        let run_in_background = input
            .get("run_in_background")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        // Check for destructive commands before execution
        if let Some(warning) = coco_shell::destructive::get_destructive_warning(command) {
            return Err(ToolError::PermissionDenied { message: warning });
        }

        // Background execution: spawn task and return immediately
        if run_in_background {
            return execute_background(command, timeout_ms, ctx).await;
        }

        // Foreground execution
        execute_foreground(command, timeout_ms, ctx).await
    }
}

/// Execute a command in the background via TaskHandle.
///
/// TS: `spawnShellTask()` -- creates a background task, returns task ID immediately.
/// Model receives `<task-notification>` XML when the task completes.
async fn execute_background(
    command: &str,
    timeout_ms: u64,
    ctx: &ToolUseContext,
) -> Result<ToolResult<Value>, ToolError> {
    let task_handle = ctx
        .task_handle
        .as_ref()
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: "Background task execution is not available in this context.".into(),
            source: None,
        })?;

    let description = ctx.tool_use_id.as_deref().unwrap_or("bash").to_string();

    let task_id = task_handle
        .spawn_shell_task(BackgroundShellRequest {
            command: command.to_string(),
            timeout_ms: Some(timeout_ms as i64),
            description: Some(description),
        })
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("Failed to spawn background task: {e}"),
            source: None,
        })?;

    Ok(ToolResult {
        data: serde_json::json!({
            "task_id": task_id,
            "status": "background",
            "message": format!("Command is running in the background. Task ID: {task_id}. You will be notified when it completes.")
        }),
        new_messages: vec![],
    })
}

/// Execute a command in the foreground with continuous progress reporting.
///
/// TS: BashTool polls TaskOutput at ~1s intervals, sending progress updates
/// with elapsed time, total bytes, and output chunks. The TUI renders
/// these as a streaming spinner with timing info.
async fn execute_foreground(
    command: &str,
    timeout_ms: u64,
    ctx: &ToolUseContext,
) -> Result<ToolResult<Value>, ToolError> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/tmp"));
    let mut executor = coco_shell::ShellExecutor::new(&cwd);

    let opts = coco_shell::ExecOptions {
        timeout_ms: Some(timeout_ms as i64),
        ..Default::default()
    };

    // Use streaming execution with progress if progress channel is available
    let cmd_result = if let Some(progress_tx) = ctx.progress_tx.clone() {
        let tool_use_id: std::sync::Arc<str> = ctx.tool_use_id.clone().unwrap_or_default().into();

        // Initial "running" progress
        let _ = progress_tx.send(ToolProgress {
            tool_use_id: tool_use_id.to_string(),
            parent_tool_use_id: None,
            data: serde_json::json!({
                "type": "bash_progress",
                "status": "running",
                "command": command,
            }),
        });

        // Continuous progress via streaming executor (~1s interval)
        let progress_id = tool_use_id.clone();
        executor
            .execute_with_progress(command, &opts, move |progress| {
                let _ = progress_tx.send(ToolProgress {
                    tool_use_id: progress_id.to_string(),
                    parent_tool_use_id: None,
                    data: serde_json::json!({
                        "type": "bash_progress",
                        "status": "running",
                        "elapsedTimeSeconds": progress.elapsed_seconds,
                        "totalBytes": progress.total_bytes,
                    }),
                });
            })
            .await
    } else {
        executor.execute(command, &opts).await
    };

    let cmd_result = cmd_result.map_err(|e| ToolError::ExecutionFailed {
        message: format!("shell execution failed: {e}"),
        source: None,
    })?;

    if cmd_result.timed_out {
        return Err(ToolError::Timeout {
            timeout_ms: timeout_ms as i64,
        });
    }

    // Format output
    let stdout = truncate_output(cmd_result.stdout.as_bytes());
    let stderr = truncate_output(cmd_result.stderr.as_bytes());
    let exit_code = cmd_result.exit_code;

    let mut result_text = String::new();
    if !stdout.is_empty() {
        result_text.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !result_text.is_empty() {
            result_text.push('\n');
        }
        result_text.push_str(&stderr);
    }

    if exit_code != 0 {
        if !result_text.is_empty() {
            result_text.push('\n');
        }
        result_text.push_str(&format!("Exit code: {exit_code}"));
    }

    if result_text.is_empty() {
        result_text = "(no output)".to_string();
    }

    Ok(ToolResult {
        data: serde_json::json!(result_text),
        new_messages: vec![],
    })
}

/// Half the budget for first/last truncation.
const TRUNCATION_HALF: usize = MAX_OUTPUT_BYTES / 2;

/// Truncate output using first+last pattern.
///
/// TS: first 5K + "... [N truncated] ..." + last 5K.
/// Preserves both the beginning (setup/context) and end (result/error).
fn truncate_output(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    if s.len() > MAX_OUTPUT_BYTES {
        let first = &s[..TRUNCATION_HALF];
        let last = &s[s.len() - TRUNCATION_HALF..];
        let truncated_count = s.len() - MAX_OUTPUT_BYTES;
        format!("{first}\n... [{truncated_count} chars truncated] ...\n{last}")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
#[path = "bash.test.rs"]
mod tests;
