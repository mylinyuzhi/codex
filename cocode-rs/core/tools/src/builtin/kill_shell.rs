//! KillShell tool for stopping background tasks.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

/// Tool for stopping background shell processes or agents.
pub struct KillShellTool;

impl KillShellTool {
    /// Create a new KillShell tool.
    pub fn new() -> Self {
        Self
    }
}

impl Default for KillShellTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for KillShellTool {
    fn name(&self) -> &str {
        "TaskStop"
    }

    fn description(&self) -> &str {
        prompts::TASK_STOP_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The ID of the background task to stop"
                }
            },
            "required": ["task_id"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }

    fn is_read_only(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let task_id = input["task_id"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "task_id must be a string",
            }
            .build()
        })?;

        ctx.emit_progress(format!("Stopping task {task_id}")).await;

        // Try shell background registry first
        let final_output = ctx
            .shell_executor
            .background_registry
            .get_output(task_id)
            .await
            .unwrap_or_default();

        let was_running = ctx.shell_executor.background_registry.stop(task_id).await;

        if was_running {
            return Ok(ToolOutput::text(format!(
                "Task {task_id} stopped successfully.\n\nFinal output:\n{final_output}"
            )));
        }

        // Fall through to check background agents via cancel token registry
        {
            let mut tokens = ctx.agent_cancel_tokens.lock().await;
            if let Some(token) = tokens.remove(task_id) {
                token.cancel();
                return Ok(ToolOutput::text(format!(
                    "Agent {task_id} cancelled successfully."
                )));
            }
        }

        Ok(ToolOutput::error(format!(
            "Task {task_id} not found. It may have already completed or never started."
        )))
    }
}

#[cfg(test)]
#[path = "kill_shell.test.rs"]
mod tests;
