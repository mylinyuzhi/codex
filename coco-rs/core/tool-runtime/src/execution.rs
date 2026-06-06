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

use coco_messages::ToolResult;
use coco_types::ToolId;
use coco_types::ToolName;
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
#[tracing::instrument(
    skip_all,
    name = "tool_call",
    fields(
        tool_use_id = %tool_use_id,
        tool_name = %tool_name,
    ),
)]
pub async fn execute_tool_call(
    tool_use_id: &str,
    tool_name: &str,
    input: Value,
    tools: &ToolRegistry,
    ctx: &ToolUseContext,
) -> ToolExecutionResult {
    let start = Instant::now();

    // Step 1: Resolve tool
    let tool_id: ToolId = tool_name
        .parse()
        .unwrap_or_else(|_| ToolId::Custom(tool_name.to_string()));

    let tool = match tools.get(&tool_id) {
        Some(t) => t,
        None => {
            tracing::warn!(
                tool_use_id = %tool_use_id,
                tool_name = %tool_name,
                "tool not found in registry"
            );
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

    // Step 2: Validate raw model input
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
        tracing::warn!(
            tool_use_id = %tool_use_id,
            tool_name = %tool_name,
            validation = ?validation,
            "tool input validation failed"
        );
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

    // Step 3: Defense-in-depth strip of internal-only Bash fields.
    //
    // TS `services/tools/toolExecution.ts:756-773` strips
    // `_simulatedSedEdit` after validation and before permission /
    // execution. The field is internal and must only be injected by
    // trusted UI flows, never by model-controlled traffic.
    //
    // Underscore-prefixed convention: any input field on Bash whose
    // key starts with `_` is treated as internal and stripped here.
    let mut input = strip_internal_bash_fields(tool_name, input);

    // Step 3.5: Per-fork canUseTool callback gate.
    //
    // TS: services/tools/toolExecution.ts:706-748 — the callback runs
    // BEFORE `tool.check_permissions` so forks (promptSuggestion,
    // speculation, side_question, compact, extract / dream / session
    // memory, agent_summary, auto_dream) can deny / rewrite per-call
    // input without modifying the static rule pipeline.
    //
    // Decisions:
    // - Deny → short-circuit, surface the message as the synthesized
    //   tool_result content; the model sees a denial and can adapt.
    // - Allow{updated_input: Some(v)} → rewrite input, skip the
    //   tool's built-in check_permissions (callback is authoritative).
    //   Speculation overlay path-rewrite uses this.
    // - Allow{updated_input: None} → proceed unchanged but still
    //   skip the built-in check (callback's opinion is final).
    // - Ask → fall through to the tool's built-in check_permissions
    //   (callback abstains; e.g. session-mem only cares about Edit
    //   on memory_path, returns Ask for everything else).
    //
    // `NoOpCanUseToolHandle` returns Ask for every call, so non-fork
    // code paths see no behavior change when ctx.can_use_tool is
    // installed during tests.
    let mut skip_builtin_perms = false;
    if let Some(handle) = ctx.can_use_tool.clone() {
        let cb_ctx = crate::can_use_tool::CanUseToolCallContext {
            tool_use_id: tool_use_id.to_string(),
            abort: ctx.abort.turn_signal(),
            require_can_use_tool: ctx.require_can_use_tool,
            messages: ctx.messages.clone(),
        };
        match handle.check(tool_name, &input, &cb_ctx).await {
            crate::can_use_tool::CanUseToolDecision::Deny {
                message,
                decision_reason,
            } => {
                tracing::info!(
                    tool_use_id = %tool_use_id,
                    tool_name = %tool_name,
                    permission_decision = "fork_deny",
                    decision_reason = ?decision_reason,
                    "fork canUseTool denied call"
                );
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
            crate::can_use_tool::CanUseToolDecision::Allow {
                updated_input: Some(rewritten),
                decision_reason,
            } => {
                tracing::debug!(
                    tool_use_id = %tool_use_id,
                    tool_name = %tool_name,
                    permission_decision = "fork_allow_rewrite",
                    decision_reason = ?decision_reason,
                    "fork canUseTool allowed with input rewrite"
                );
                input = rewritten;
                skip_builtin_perms = true;
            }
            crate::can_use_tool::CanUseToolDecision::Allow {
                updated_input: None,
                decision_reason,
            } => {
                tracing::debug!(
                    tool_use_id = %tool_use_id,
                    tool_name = %tool_name,
                    permission_decision = "fork_allow",
                    decision_reason = ?decision_reason,
                    "fork canUseTool allowed call"
                );
                skip_builtin_perms = true;
            }
            crate::can_use_tool::CanUseToolDecision::Ask { decision_reason } => {
                tracing::debug!(
                    tool_use_id = %tool_use_id,
                    tool_name = %tool_name,
                    permission_decision = "fork_ask_passthrough",
                    decision_reason = ?decision_reason,
                    "fork canUseTool abstained; falling through to built-in check"
                );
            }
        }
    }

    // Step 4: Check permissions (tool-level opinion only).
    //
    // Production permission decisions go through
    // `app/query::tool_call_preparer::resolve_permission_decision`,
    // which combines this tool opinion with rule + mode-fallthrough
    // evaluation via `coco_permissions::PermissionEvaluator`. This
    // path is used by direct callers (tests + the legacy batch
    // entrypoint) that have already cleared the rule pipeline; it
    // honors the tool's own `Deny` / `Ask` opinions but treats
    // `Passthrough` and `Allow` as proceed-to-execute.
    //
    // Skipped when step 3.5's canUseTool callback explicitly returned
    // `Allow` — the callback's opinion is authoritative for the Allow
    // path (TS parity: toolExecution.ts:737-748).
    if !skip_builtin_perms {
        let decision = tool.check_permissions(&input, ctx).await;
        match decision {
            coco_types::ToolCheckResult::Deny { message } => {
                tracing::info!(
                    tool_use_id = %tool_use_id,
                    tool_name = %tool_name,
                    permission_decision = "deny",
                    "tool denied by permission check"
                );
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
            coco_types::ToolCheckResult::Ask { .. } => {
                // In auto mode, treat as allow (TUI handles interactive prompts)
                tracing::debug!(
                    tool_use_id = %tool_use_id,
                    tool_name = %tool_name,
                    permission_decision = "ask",
                    "tool requires permission ask (auto-mode allow)"
                );
            }
            coco_types::ToolCheckResult::Allow { .. }
            | coco_types::ToolCheckResult::Passthrough => {
                tracing::debug!(
                    tool_use_id = %tool_use_id,
                    tool_name = %tool_name,
                    permission_decision = "allow_or_passthrough",
                    "tool allowed (or no opinion) by check"
                );
            }
        }
    }

    // Step 5: Execute tool (with cancellation support)
    tracing::debug!(
        tool_use_id = %tool_use_id,
        tool_name = %tool_name,
        "tool execute begin"
    );
    let result = tokio::select! {
        r = tool.execute(input, ctx) => r,
        () = ctx.abort.cancelled() => Err(ToolError::Cancelled),
    };

    let duration_ms = start.elapsed().as_millis() as i64;
    let error_class = result.as_ref().err().map(classify_tool_error);

    match &result {
        Ok(_) => tracing::debug!(
            tool_use_id = %tool_use_id,
            tool_name = %tool_name,
            duration_ms,
            "tool execute ok"
        ),
        Err(e) => tracing::warn!(
            tool_use_id = %tool_use_id,
            tool_name = %tool_name,
            duration_ms,
            error_class = ?error_class,
            error = %e,
            "tool execute failed"
        ),
    }

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

#[cfg(test)]
#[path = "execution.test.rs"]
mod tests;
