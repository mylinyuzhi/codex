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

        // Check if task exists in background shell registry
        let is_shell_running = ctx
            .shell_executor
            .background_registry
            .is_running(task_id)
            .await;

        if is_shell_running {
            // Shell task is running
            if block {
                let timeout_duration = std::time::Duration::from_millis(timeout_ms as u64);
                let start = std::time::Instant::now();

                loop {
                    if ctx.is_cancelled() {
                        let output = ctx
                            .shell_executor
                            .background_registry
                            .get_output(task_id)
                            .await
                            .unwrap_or_default();
                        return Ok(ToolOutput::text(format!(
                            "Task {task_id} (cancelled):\n{output}"
                        )));
                    }
                    if !ctx
                        .shell_executor
                        .background_registry
                        .is_running(task_id)
                        .await
                    {
                        break;
                    }
                    if start.elapsed() >= timeout_duration {
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
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }

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
            return Ok(ToolOutput::text(format!(
                "Task {task_id} ({status}):\n{output}"
            )));
        }

        // Check if we can still get shell output (task may have completed)
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

        // Not a shell task — check agent output files
        // Build candidate paths: agent_output_dir > session_dir sibling > temp_dir
        let agent_file_name = format!("{task_id}.jsonl");
        let mut candidate_paths = Vec::new();

        if let Some(ref dir) = ctx.agent_output_dir {
            candidate_paths.push(dir.join(&agent_file_name));
        }
        if let Some(ref session_dir) = ctx.session_dir {
            candidate_paths.push(
                session_dir
                    .parent()
                    .unwrap_or(session_dir)
                    .join("cocode-agents")
                    .join(&agent_file_name),
            );
        }
        candidate_paths.push(
            std::env::temp_dir()
                .join("cocode-agents")
                .join(&agent_file_name),
        );

        if block {
            // Poll for agent output file existence with timeout
            let timeout_duration = std::time::Duration::from_millis(timeout_ms as u64);
            let start = std::time::Instant::now();

            loop {
                if ctx.is_cancelled() {
                    return Ok(ToolOutput::text(format!(
                        "Agent {task_id} (cancelled while waiting for output)"
                    )));
                }
                for path in &candidate_paths {
                    if path.exists()
                        && let Ok(content) = tokio::fs::read_to_string(path).await
                    {
                        return Ok(format_agent_output(task_id, &content));
                    }
                }
                if start.elapsed() >= timeout_duration {
                    return Ok(ToolOutput::text(format!(
                        "Agent {task_id} (still running, timeout after {timeout_ms}ms)"
                    )));
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        } else {
            // Non-blocking: check once
            for path in &candidate_paths {
                if path.exists()
                    && let Ok(content) = tokio::fs::read_to_string(path).await
                {
                    return Ok(format_agent_output(task_id, &content));
                }
            }
        }

        Ok(ToolOutput::error(format!(
            "Task {task_id} not found. It may have been stopped or never started."
        )))
    }
}

/// Parse agent JSONL content and format as a ToolOutput.
fn format_agent_output(task_id: &str, content: &str) -> ToolOutput {
    if let Ok(entry) = serde_json::from_str::<serde_json::Value>(content) {
        let status = entry["status"].as_str().unwrap_or("unknown");
        let output = entry["output"]
            .as_str()
            .or_else(|| entry["error"].as_str())
            .unwrap_or(content);
        ToolOutput::text(format!("Agent {task_id} ({status}):\n{output}"))
    } else {
        ToolOutput::text(format!("Agent {task_id}:\n{content}"))
    }
}

#[cfg(test)]
#[path = "task_output.test.rs"]
mod tests;
