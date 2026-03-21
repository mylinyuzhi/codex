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
        cocode_protocol::ToolName::TaskStop.as_str()
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
        let task_id = super::input_helpers::require_str(&input, "task_id")?;

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
        let maybe_token = ctx.agent_cancel_tokens.lock().await.remove(task_id);
        if let Some(token) = maybe_token {
            // Read partial output before cancellation (best-effort)
            let partial = read_agent_partial_output(task_id, ctx).await;
            token.cancel();

            // Record the kill so status is reported as Killed (not Failed)
            ctx.killed_agents.lock().await.insert(task_id.to_string());

            let msg = match partial {
                Some(output) => {
                    format!("Agent {task_id} cancelled.\n\nPartial output:\n{output}")
                }
                None => format!("Agent {task_id} cancelled successfully."),
            };
            return Ok(ToolOutput::text(msg));
        }

        Ok(ToolOutput::error(format!(
            "Task {task_id} not found. It may have already completed or never started."
        )))
    }
}

/// Best-effort read of an agent's partial output from its transcript file.
///
/// Checks candidate paths (agent_output_dir, session_dir sibling, temp_dir)
/// and parses JSONL entries, extracting output/text/message/error fields.
async fn read_agent_partial_output(task_id: &str, ctx: &ToolContext) -> Option<String> {
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

    for path in &candidate_paths {
        if let Ok(content) = tokio::fs::read_to_string(path).await {
            let mut parts = Vec::new();
            for line in content.lines().filter(|l| !l.trim().is_empty()) {
                if let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(v) = entry["output"]
                        .as_str()
                        .or_else(|| entry["text"].as_str())
                        .or_else(|| entry["message"].as_str())
                    {
                        parts.push(v.to_string());
                    } else if let Some(err) = entry["error"].as_str() {
                        parts.push(format!("[error] {err}"));
                    }
                }
            }
            if !parts.is_empty() {
                return Some(parts.join("\n"));
            }
        }
    }

    None
}

#[cfg(test)]
#[path = "kill_shell.test.rs"]
mod tests;
