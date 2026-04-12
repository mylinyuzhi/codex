use coco_tool::DescriptionOptions;
use coco_tool::Tool;
use coco_tool::ToolError;
use coco_tool::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;

// ── SleepTool ──

pub struct SleepTool;

#[async_trait::async_trait]
impl Tool for SleepTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Sleep)
    }
    fn name(&self) -> &str {
        ToolName::Sleep.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Sleep for a specified number of seconds.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "seconds".into(),
            serde_json::json!({"type": "number", "description": "Number of seconds to sleep"}),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let seconds = input
            .get("seconds")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(1.0);

        if seconds < 0.0 {
            return Err(ToolError::InvalidInput {
                message: "seconds must be non-negative".into(),
                error_code: None,
            });
        }

        // Cap at 5 minutes to prevent indefinite blocking
        let capped = seconds.min(300.0);
        let duration = std::time::Duration::from_secs_f64(capped);
        tokio::time::sleep(duration).await;

        Ok(ToolResult {
            data: serde_json::json!({
                "message": format!("Slept for {capped:.1} seconds"),
                "seconds": capped,
            }),
            new_messages: vec![],
        })
    }
}

// ── PowerShellTool ──

pub struct PowerShellTool;

#[async_trait::async_trait]
impl Tool for PowerShellTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::PowerShell)
    }
    fn name(&self) -> &str {
        ToolName::PowerShell.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Execute a PowerShell command (requires pwsh to be installed).".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "command".into(),
            serde_json::json!({"type": "string", "description": "The PowerShell command to execute"}),
        );
        p.insert(
            "timeout".into(),
            serde_json::json!({"type": "number", "description": "Timeout in milliseconds (default 120000)"}),
        );
        p.insert(
            "run_in_background".into(),
            serde_json::json!({"type": "boolean", "description": "Set to true to run in background."}),
        );
        ToolInputSchema { properties: p }
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let command = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
        let run_in_background = input
            .get("run_in_background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Background execution via task handle
        if run_in_background {
            if let Some(task_handle) = &ctx.task_handle {
                let task_id = task_handle
                    .spawn_shell_task(coco_tool::BackgroundShellRequest {
                        command: format!("pwsh -NoProfile -NonInteractive -Command {command:?}"),
                        timeout_ms: input.get("timeout").and_then(|v| v.as_i64()),
                        description: Some("PowerShell background task".into()),
                    })
                    .await
                    .map_err(|e| ToolError::ExecutionFailed {
                        message: format!("Failed to spawn background task: {e}"),
                        source: None,
                    })?;
                return Ok(ToolResult {
                    data: serde_json::json!({
                        "task_id": task_id,
                        "status": "background",
                        "message": format!("PowerShell command running in background. Task ID: {task_id}")
                    }),
                    new_messages: vec![],
                });
            }
            return Err(ToolError::ExecutionFailed {
                message: "Background execution not available in this context.".into(),
                source: None,
            });
        }

        if command.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "command parameter is required".into(),
                error_code: None,
            });
        }

        let timeout_ms = input
            .get("timeout")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(120_000) as u64;

        let child = tokio::process::Command::new("pwsh")
            .args(["-NoProfile", "-NonInteractive", "-Command", command])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!(
                    "Failed to start pwsh: {e}. Ensure PowerShell (pwsh) is installed."
                ),
                source: None,
            })?;

        let timeout_duration = std::time::Duration::from_millis(timeout_ms);
        let result = tokio::time::timeout(timeout_duration, child.wait_with_output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let exit_code = output.status.code().unwrap_or(-1);

                Ok(ToolResult {
                    data: serde_json::json!({
                        "stdout": stdout.as_ref(),
                        "stderr": stderr.as_ref(),
                        "exit_code": exit_code,
                    }),
                    new_messages: vec![],
                })
            }
            Ok(Err(e)) => Err(ToolError::ExecutionFailed {
                message: format!("PowerShell execution failed: {e}"),
                source: None,
            }),
            Err(_) => Err(ToolError::Timeout {
                timeout_ms: timeout_ms as i64,
            }),
        }
    }
}

// ── ReplTool ──

pub struct ReplTool;

#[async_trait::async_trait]
impl Tool for ReplTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Repl)
    }
    fn name(&self) -> &str {
        ToolName::Repl.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Start an interactive REPL session for a supported language.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "language".into(),
            serde_json::json!({"type": "string", "description": "Programming language for the REPL (e.g., python, node)"}),
        );
        p.insert(
            "command".into(),
            serde_json::json!({"type": "string", "description": "Command to execute in the REPL"}),
        );
        ToolInputSchema { properties: p }
    }

    fn is_transparent_wrapper(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        Err(ToolError::ExecutionFailed {
            message: "REPL tool is not available in this context. \
                      Use the Bash tool to run language-specific commands instead \
                      (e.g., `python3 -c \"...\"` or `node -e \"...\"`)."
                .into(),
            source: None,
        })
    }
}

// ── SyntheticOutputTool ──

pub struct SyntheticOutputTool;

#[async_trait::async_trait]
impl Tool for SyntheticOutputTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::SyntheticOutput)
    }
    fn name(&self) -> &str {
        ToolName::SyntheticOutput.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Emit synthetic output for SDK integrations. Returns the provided output text directly."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "output".into(),
            serde_json::json!({"type": "string", "description": "Output text to emit"}),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let output = input.get("output").and_then(|v| v.as_str()).unwrap_or("");

        Ok(ToolResult {
            data: serde_json::json!(output),
            new_messages: vec![],
        })
    }
}
