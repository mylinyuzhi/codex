//! TaskOutput tool for retrieving output from background tasks.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

/// Tool for retrieving output from background tasks or agents.
pub struct TaskOutputTool;

impl TaskOutputTool {
    /// Create a new TaskOutput tool.
    pub fn new() -> Self {
        Self
    }
}

impl Default for TaskOutputTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for TaskOutputTool {
    fn name(&self) -> &str {
        "TaskOutput"
    }

    fn description(&self) -> &str {
        prompts::TASK_OUTPUT_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The task ID to get output from"
                },
                "block": {
                    "type": "boolean",
                    "description": "Whether to wait for completion",
                    "default": true
                },
                "timeout": {
                    "type": "integer",
                    "description": "Max wait time in ms",
                    "default": 30000
                }
            },
            "required": ["task_id"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let task_id = input["task_id"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "task_id must be a string",
            }
            .build()
        })?;
        let block = input["block"].as_bool().unwrap_or(true);
        let timeout_ms = input["timeout"].as_i64().unwrap_or(30_000);

        ctx.emit_progress(format!("Getting output for task {task_id}"))
            .await;

        // Check if task exists
        let is_running = ctx
            .shell_executor
            .background_registry
            .is_running(task_id)
            .await;

        if !is_running {
            // Check if we can still get output (task may have completed)
            if let Some(output) = ctx
                .shell_executor
                .background_registry
                .get_output(task_id)
                .await
            {
                return Ok(ToolOutput::text(format!(
                    "Task {task_id} (completed):\n{output}"
                )));
            }
            return Ok(ToolOutput::error(format!(
                "Task {task_id} not found. It may have been stopped or never started."
            )));
        }

        // Task is running
        if block {
            // Wait for completion with timeout
            let timeout_duration = std::time::Duration::from_millis(timeout_ms as u64);
            let start = std::time::Instant::now();

            loop {
                // Check if task completed
                if !ctx
                    .shell_executor
                    .background_registry
                    .is_running(task_id)
                    .await
                {
                    break;
                }

                // Check timeout
                if start.elapsed() >= timeout_duration {
                    // Return current output with timeout note
                    let output = ctx
                        .shell_executor
                        .background_registry
                        .get_output(task_id)
                        .await
                        .unwrap_or_default();
                    return Ok(ToolOutput::text(format!(
                        "Task {task_id} (still running, timeout after {timeout_ms}ms):\n{output}"
                    )));
                }

                // Wait a bit before checking again
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }

        // Return current output
        let output = ctx
            .shell_executor
            .background_registry
            .get_output(task_id)
            .await
            .unwrap_or_default();
        let status = if ctx
            .shell_executor
            .background_registry
            .is_running(task_id)
            .await
        {
            "running"
        } else {
            "completed"
        };
        Ok(ToolOutput::text(format!(
            "Task {task_id} ({status}):\n{output}"
        )))
    }
}

#[cfg(test)]
#[path = "task_output.test.rs"]
mod tests;
