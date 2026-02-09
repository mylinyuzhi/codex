//! Large tool result persistence.
//!
//! When tools return very large results (>400K characters by default), this module
//! persists the full result to disk and returns a truncated preview to save
//! context window tokens.
//!
//! ## Usage
//!
//! ```ignore
//! use cocode_tools::result_persistence::persist_if_needed;
//!
//! let output = tool.execute(input, ctx).await?;
//! let output = persist_if_needed(
//!     output,
//!     &tool_call.id,
//!     session_dir,
//!     &tool_config,
//! ).await;
//! ```
//!
//! ## File Storage
//!
//! Large results are stored at:
//! `{session_dir}/tool-results/{tool_use_id}.txt`

use cocode_protocol::ToolConfig;
use cocode_protocol::ToolOutput;
use cocode_protocol::ToolResultContent;
use std::path::Path;
use tracing::warn;

/// XML-style tag for persisted output (matches Claude Code v2.1.7 format).
const PERSISTED_OUTPUT_START: &str = "<persisted-output>";
const PERSISTED_OUTPUT_END: &str = "</persisted-output>";

/// Persist large tool result to disk if needed.
///
/// If the result content exceeds `config.max_result_size`, saves the full result
/// to disk and returns a modified `ToolOutput` containing:
/// - The file path where the full result was saved
/// - A preview of the first `config.result_preview_size` characters
///
/// If persistence is disabled or the result is small enough, returns the
/// original output unchanged.
///
/// # Arguments
///
/// * `output` - The tool execution output
/// * `tool_use_id` - Unique identifier for this tool call (used as filename)
/// * `session_dir` - Session directory for storing results
/// * `config` - Tool configuration with size thresholds
///
/// # Returns
///
/// The (possibly modified) tool output. If persistence fails, returns the
/// original output unchanged with a warning logged.
pub async fn persist_if_needed(
    output: ToolOutput,
    tool_use_id: &str,
    session_dir: &Path,
    config: &ToolConfig,
) -> ToolOutput {
    // Early return if persistence is disabled
    if !config.enable_result_persistence {
        return output;
    }

    // Extract text content
    let content = match &output.content {
        ToolResultContent::Text(s) => s.clone(),
        ToolResultContent::Structured(v) => v.to_string(),
    };

    // Check if content exceeds threshold
    let max_size = config.max_result_size as usize;
    if content.len() <= max_size {
        return output;
    }

    // Create tool-results directory
    let results_dir = session_dir.join("tool-results");
    if let Err(e) = tokio::fs::create_dir_all(&results_dir).await {
        warn!(
            tool_use_id = %tool_use_id,
            error = %e,
            "Failed to create tool-results directory, returning original output"
        );
        return output;
    }

    // Write full content to file
    let file_path = results_dir.join(format!("{tool_use_id}.txt"));
    if let Err(e) = tokio::fs::write(&file_path, &content).await {
        warn!(
            tool_use_id = %tool_use_id,
            path = %file_path.display(),
            error = %e,
            "Failed to persist large result, returning original output"
        );
        return output;
    }

    // Create preview
    let preview_size = config.result_preview_size as usize;
    let preview = if content.len() > preview_size {
        // Find a safe truncation point (don't split UTF-8 chars)
        let truncate_at = content
            .char_indices()
            .take_while(|(i, _)| *i < preview_size)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(preview_size.min(content.len()));
        format!("{}...", &content[..truncate_at])
    } else {
        content.clone()
    };

    // Build the persisted output message
    let persisted_content = format!(
        "{PERSISTED_OUTPUT_START}
Output too large ({} characters). Full output saved to: {}

Preview (first {} chars):
{preview}
{PERSISTED_OUTPUT_END}",
        content.len(),
        file_path.display(),
        preview_size
    );

    tracing::debug!(
        tool_use_id = %tool_use_id,
        original_size = content.len(),
        preview_size = preview.len(),
        path = %file_path.display(),
        "Persisted large tool result to disk"
    );

    ToolOutput {
        content: ToolResultContent::Text(persisted_content),
        is_error: output.is_error,
        modifiers: output.modifiers,
    }
}

#[cfg(test)]
#[path = "result_persistence.test.rs"]
mod tests;
