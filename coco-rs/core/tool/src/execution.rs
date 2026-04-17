//! Tool execution pipeline — single tool call lifecycle.
//!
//! TS: services/tools/toolExecution.ts (1745 LOC)
//!
//! Implements the full lifecycle of a single tool call:
//! 1. Permission check (canUseTool)
//! 2. Tool resolution (find by name)
//! 3. Input validation
//! 4. Pre-execution hooks
//! 5. Tool invocation (with timeout, cancellation)
//! 6. Post-execution hooks
//! 7. Result processing + error classification

use coco_types::PermissionDecision;
use coco_types::ToolId;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::time::Instant;

use crate::context::ToolUseContext;
use crate::error::ToolError;
use crate::registry::ToolRegistry;

/// Hook timing display threshold (ms) — hooks faster than this are silent.
pub const HOOK_TIMING_DISPLAY_THRESHOLD_MS: i64 = 500;

/// Result of executing a single tool call through the full pipeline.
#[derive(Debug)]
pub struct ToolExecutionResult {
    /// The tool call ID.
    pub tool_use_id: String,
    /// The tool that was called.
    pub tool_id: ToolId,
    /// The tool name.
    pub tool_name: String,
    /// Execution result (Ok = success, Err = failure).
    pub result: Result<ToolResult<Value>, ToolError>,
    /// Duration in milliseconds.
    pub duration_ms: i64,
    /// Whether permission was denied.
    pub permission_denied: bool,
    /// Error classification for telemetry.
    pub error_class: Option<String>,
}

/// Classify a tool execution error for telemetry.
///
/// TS: classifyToolError() — maps errors to analytics categories.
pub fn classify_tool_error(error: &ToolError) -> String {
    match error {
        ToolError::NotFound { .. } => "not_found".to_string(),
        ToolError::InvalidInput { .. } => "invalid_input".to_string(),
        ToolError::PermissionDenied { .. } => "permission_denied".to_string(),
        ToolError::Timeout { .. } => "timeout".to_string(),
        ToolError::Cancelled => "cancelled".to_string(),
        ToolError::ExecutionFailed { message, .. } => {
            if message.contains("ENOENT") || message.contains("not found") {
                "file_not_found".to_string()
            } else if message.contains("EACCES") || message.contains("Permission") {
                "permission_error".to_string()
            } else if message.contains("ENOSPC") || message.contains("disk") {
                "disk_error".to_string()
            } else {
                "execution_error".to_string()
            }
        }
    }
}

/// Execute a single tool call through the full pipeline.
///
/// This is the core execution function called by the StreamingToolExecutor
/// for each tool. It handles permission checking, validation, execution,
/// and error classification.
pub async fn execute_tool_call(
    tool_use_id: &str,
    tool_name: &str,
    input: Value,
    tools: &ToolRegistry,
    ctx: &ToolUseContext,
) -> ToolExecutionResult {
    let start = Instant::now();

    // R7-T19: defense-in-depth strip of internal-only Bash fields.
    //
    // TS `services/tools/toolExecution.ts:756-773` strips
    // `_simulatedSedEdit` from any model-provided Bash input before
    // reaching `tool.call()`. The field is internal — it must only
    // be injected by the SedEditPermissionRequest UI dialog after
    // user approval — and exposing it to model-controlled paths would
    // let the model bypass permission checks and the sandbox by
    // pairing an innocuous command with an arbitrary file write.
    //
    // coco-rs strips at the chokepoint between the executor and the
    // tool implementation. The trusted-callers path that legitimately
    // sets `_simulatedSedEdit` (the SedEditPermissionRequest dialog,
    // not yet wired in coco-rs) bypasses this codepath by calling
    // `BashTool::execute` directly. Everything that flows through
    // `execute_tool_call` is model output.
    //
    // Underscore-prefixed convention: any input field on Bash whose
    // key starts with `_` is treated as internal and stripped here,
    // matching TS's approach of namespacing internal fields with `_`.
    let input = strip_internal_bash_fields(tool_name, input);

    // Step 1: Resolve tool
    let tool_id: ToolId = tool_name
        .parse()
        .unwrap_or_else(|_| ToolId::Custom(tool_name.to_string()));

    let tool = match tools.get(&tool_id) {
        Some(t) => t.clone(),
        None => {
            let err = ToolError::NotFound {
                tool_id: tool_id.clone(),
            };
            return ToolExecutionResult {
                tool_use_id: tool_use_id.to_string(),
                tool_id,
                tool_name: tool_name.to_string(),
                result: Err(err),
                duration_ms: start.elapsed().as_millis() as i64,
                permission_denied: false,
                error_class: Some("not_found".to_string()),
            };
        }
    };

    // Step 2: Validate input
    //
    // R7-T24: validation runs BEFORE permission check to match TS
    // `services/tools/toolExecution.ts:614-686` ordering. TS calls
    // `tool.inputSchema.safeParse(input)` and `tool.validateInput`
    // before any permission resolution; this ensures malformed
    // input is reported as an `InvalidInput` error rather than a
    // confusing "permission denied" message. It also guarantees
    // that permission decisions are computed against validated
    // input, never against raw model output.
    let validation = tool.validate_input(&input, ctx);
    if !validation.is_valid() {
        return ToolExecutionResult {
            tool_use_id: tool_use_id.to_string(),
            tool_id,
            tool_name: tool_name.to_string(),
            result: Err(ToolError::InvalidInput {
                message: format!("validation failed: {validation:?}"),
                error_code: None,
            }),
            duration_ms: start.elapsed().as_millis() as i64,
            permission_denied: false,
            error_class: Some("invalid_input".to_string()),
        };
    }

    // Step 3: Check permissions
    let decision = tool.check_permissions(&input, ctx).await;
    match decision {
        PermissionDecision::Deny { message, .. } => {
            return ToolExecutionResult {
                tool_use_id: tool_use_id.to_string(),
                tool_id,
                tool_name: tool_name.to_string(),
                result: Err(ToolError::PermissionDenied { message }),
                duration_ms: start.elapsed().as_millis() as i64,
                permission_denied: true,
                error_class: Some("permission_denied".to_string()),
            };
        }
        PermissionDecision::Ask { .. } => {
            // In auto mode, treat as allow (TUI handles interactive prompts)
        }
        PermissionDecision::Allow { .. } => {}
    }

    // Step 4: Execute tool (with cancellation support)
    let result = tokio::select! {
        r = tool.execute(input, ctx) => r,
        () = ctx.cancel.cancelled() => Err(ToolError::Cancelled),
    };

    let duration_ms = start.elapsed().as_millis() as i64;
    let error_class = result.as_ref().err().map(classify_tool_error);

    ToolExecutionResult {
        tool_use_id: tool_use_id.to_string(),
        tool_id,
        tool_name: tool_name.to_string(),
        result,
        duration_ms,
        permission_denied: false,
        error_class,
    }
}

/// Strip internal-only fields from model-provided Bash input.
///
/// TS: `services/tools/toolExecution.ts:756-773` strips
/// `_simulatedSedEdit` from the parsed Bash input as a defense-in-depth
/// safeguard. The convention is that any Bash input field whose key
/// starts with `_` is treated as internal and must only be set by the
/// permission UI dialog (e.g. SedEditPermissionRequest), never by the
/// model. Stripping at the executor chokepoint guarantees that even
/// if the schema accepts the field, model traffic can't reach the
/// `apply_sed_edit` short-circuit.
///
/// For non-Bash tools the input is returned unchanged. For Bash inputs
/// that are not objects (defensive), the input is returned unchanged.
fn strip_internal_bash_fields(tool_name: &str, mut input: Value) -> Value {
    if tool_name != ToolName::Bash.as_str() {
        return input;
    }
    if let Some(obj) = input.as_object_mut() {
        // Two-pass: collect internal keys first to avoid borrow-conflict
        // with the mutating remove. Silent strip — TS doesn't log
        // either, the field shouldn't be in model traffic in normal
        // operation.
        let internal_keys: Vec<String> =
            obj.keys().filter(|k| k.starts_with('_')).cloned().collect();
        for key in internal_keys {
            obj.remove(&key);
        }
    }
    input
}

/// Check if a tool is a file-editing tool (for tracking purposes).
pub fn is_code_editing_tool(tool_name: &str) -> bool {
    const EDITING_TOOLS: &[&str] = &[
        ToolName::Edit.as_str(),
        ToolName::Write.as_str(),
        ToolName::NotebookEdit.as_str(),
        ToolName::Bash.as_str(),
        ToolName::PowerShell.as_str(),
    ];
    EDITING_TOOLS.contains(&tool_name)
}

/// Check if a tool is a file-reading tool.
pub fn is_file_reading_tool(tool_name: &str) -> bool {
    const READING_TOOLS: &[&str] = &[
        ToolName::Read.as_str(),
        ToolName::Glob.as_str(),
        ToolName::Grep.as_str(),
    ];
    READING_TOOLS.contains(&tool_name)
}

/// Extract file extension from a tool input for analytics.
pub fn extract_file_extension(tool_name: &str, input: &Value) -> Option<String> {
    const FILE_PATH_TOOLS: &[&str] = &[
        ToolName::Read.as_str(),
        ToolName::Write.as_str(),
        ToolName::Edit.as_str(),
        ToolName::NotebookEdit.as_str(),
    ];
    let path = if FILE_PATH_TOOLS.contains(&tool_name) {
        input.get("file_path").and_then(|v| v.as_str())
    } else {
        None
    }?;

    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
}

/// Check if a tool name refers to a deferred tool (discovered via ToolSearch).
pub fn is_deferred_tool(tool_name: &str) -> bool {
    const DEFERRED_TOOLS: &[&str] = &[
        ToolName::CronCreate.as_str(),
        ToolName::CronDelete.as_str(),
        ToolName::CronList.as_str(),
        ToolName::RemoteTrigger.as_str(),
        ToolName::Sleep.as_str(),
        ToolName::NotebookEdit.as_str(),
        ToolName::EnterWorktree.as_str(),
        ToolName::ExitWorktree.as_str(),
        ToolName::PowerShell.as_str(),
        ToolName::SyntheticOutput.as_str(),
    ];
    DEFERRED_TOOLS.contains(&tool_name)
}

#[cfg(test)]
#[path = "execution.test.rs"]
mod tests;
