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
        cocode_protocol::ToolName::TaskOutput.as_str()
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

    fn is_read_only(&self) -> bool {
        true
    }

    fn should_defer(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let task_id = input["task_id"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "task_id must be a string",
            }
            .build()
        })?;
        let block = super::input_helpers::bool_or(&input, "block", true);
        let timeout_ms = input["timeout"].as_i64().unwrap_or(30_000);

        ctx.emit_progress(format!("Getting output for task {task_id}"))
            .await;

        // Check if task exists in background shell registry
        let is_shell_running = ctx
            .shell_executor
            .background_registry
            .is_running(task_id)
            .await;

        // Resolve command name for richer headers
        let command_label = ctx
            .shell_executor
            .background_registry
            .get_command(task_id)
            .await;

        // Format header with optional command
        let header = |status: &str| -> String {
            match &command_label {
                Some(cmd) => format!("Task {task_id} ({status}): {cmd}"),
                None => format!("Task {task_id} ({status})"),
            }
        };

        if is_shell_running {
            // Shell task is running
            if block {
                let timeout_duration = std::time::Duration::from_millis(timeout_ms as u64);

                // Use Notify-based waiting instead of polling
                if let Some(notify) = ctx
                    .shell_executor
                    .background_registry
                    .get_completed_notify(task_id)
                    .await
                {
                    tokio::select! {
                        _ = notify.notified() => { /* completed */ }
                        _ = tokio::time::sleep(timeout_duration) => {
                            let output = ctx
                                .shell_executor
                                .background_registry
                                .get_output(task_id)
                                .await
                                .unwrap_or_default();
                            return Ok(ToolOutput::text(format!(
                                "{}:\n{output}",
                                header(&format!("still running, timeout after {timeout_ms}ms"))
                            )));
                        }
                        _ = ctx.cancel_token.cancelled() => {
                            let output = ctx
                                .shell_executor
                                .background_registry
                                .get_output(task_id)
                                .await
                                .unwrap_or_default();
                            return Ok(ToolOutput::text(format!(
                                "{}:\n{output}",
                                header("cancelled")
                            )));
                        }
                    }
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
            return Ok(ToolOutput::text(format!("{}:\n{output}", header(status))));
        }

        // Check if we can still get shell output (task may have completed/stopped)
        if let Some(output) = ctx
            .shell_executor
            .background_registry
            .get_output(task_id)
            .await
        {
            return Ok(ToolOutput::text(format!(
                "{}:\n{output}",
                header("completed")
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
                    if path.exists() {
                        return Ok(
                            read_agent_output_delta(task_id, path, &ctx.output_offsets).await
                        );
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
                if path.exists() {
                    return Ok(read_agent_output_delta(task_id, path, &ctx.output_offsets).await);
                }
            }
        }

        Ok(ToolOutput::error(format!(
            "Task {task_id} not found. It may have been stopped or never started."
        )))
    }
}

/// Read agent output incrementally using byte-offset delta tracking.
///
/// On the first call for a given task_id, reads from offset 0 (full content).
/// On subsequent calls, reads only new entries appended since the last read.
async fn read_agent_output_delta(
    task_id: &str,
    path: &std::path::Path,
    offsets: &std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<String, u64>>>,
) -> ToolOutput {
    let mut offsets = offsets.lock().await;
    let offset = offsets.get(task_id).copied().unwrap_or(0);
    match read_jsonl_from_offset(path, offset).await {
        Ok((entries, new_offset)) => {
            offsets.insert(task_id.to_string(), new_offset);
            format_delta_entries(task_id, &entries, offset > 0)
        }
        Err(e) => ToolOutput::text(format!("Agent {task_id} (error reading output: {e})")),
    }
}

/// Read JSONL entries from a file starting at a byte offset.
///
/// Returns parsed entries and the new byte offset for subsequent reads.
async fn read_jsonl_from_offset(
    path: &std::path::Path,
    byte_offset: u64,
) -> std::io::Result<(Vec<Value>, u64)> {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncSeekExt;

    let mut file = tokio::fs::File::open(path).await?;
    let file_len = file.metadata().await?.len();

    if byte_offset >= file_len {
        return Ok((Vec::new(), byte_offset));
    }

    file.seek(std::io::SeekFrom::Start(byte_offset)).await?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).await?;

    let entries: Vec<Value> = buf
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    Ok((entries, file_len))
}

/// Format delta entries from incremental reads.
fn format_delta_entries(task_id: &str, entries: &[Value], is_delta: bool) -> ToolOutput {
    if entries.is_empty() {
        return ToolOutput::text(format!("Agent {task_id}: no new output"));
    }

    let last_status = entries
        .iter()
        .rev()
        .find_map(|e| e["status"].as_str())
        .unwrap_or("running");

    let mut parts = Vec::new();
    for entry in entries {
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

    let combined = if parts.is_empty() {
        format!("{} entries", entries.len())
    } else {
        parts.join("\n")
    };

    let delta_label = if is_delta { " (new)" } else { "" };
    ToolOutput::text(format!(
        "Agent {task_id} ({last_status}){delta_label}:\n{combined}"
    ))
}

/// Parse agent JSONL content (potentially multi-line) and format as a ToolOutput.
///
/// The transcript file is in JSONL format: each line is a separate JSON entry.
/// This function handles both single-entry and multi-entry transcripts, extracting
/// the last status and combining output/error messages from all entries.
#[cfg(test)]
fn format_agent_output(task_id: &str, content: &str) -> ToolOutput {
    let entries: Vec<serde_json::Value> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    if entries.is_empty() {
        return ToolOutput::text(format!("Agent {task_id}:\n{content}"));
    }

    // Use the last entry's status as the overall status
    let last_status = entries
        .iter()
        .rev()
        .find_map(|e| e["status"].as_str())
        .unwrap_or("running");

    // Collect output/error parts from all entries
    let mut parts = Vec::new();
    for entry in &entries {
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

    let combined = if parts.is_empty() {
        content.to_string()
    } else {
        parts.join("\n")
    };
    ToolOutput::text(format!("Agent {task_id} ({last_status}):\n{combined}"))
}

#[cfg(test)]
#[path = "task_output.test.rs"]
mod tests;
