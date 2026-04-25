use coco_types::ToolId;
use serde::Deserialize;
use serde::Serialize;
use std::fmt;

/// Errors that can occur during tool execution.
#[derive(Debug)]
pub enum ToolError {
    /// Tool not found in registry.
    NotFound { tool_id: ToolId },
    /// Invalid input (failed validation).
    InvalidInput {
        message: String,
        error_code: Option<String>,
    },
    /// Execution failed.
    ExecutionFailed {
        message: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
    /// Permission denied.
    PermissionDenied { message: String },
    /// Timed out.
    Timeout { timeout_ms: i64 },
    /// Cancelled by user or system.
    Cancelled,
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { tool_id } => write!(f, "tool not found: {tool_id}"),
            Self::InvalidInput { message, .. } => write!(f, "invalid input: {message}"),
            Self::ExecutionFailed { message, .. } => write!(f, "execution failed: {message}"),
            Self::PermissionDenied { message } => write!(f, "permission denied: {message}"),
            Self::Timeout { timeout_ms } => write!(f, "timed out after {timeout_ms}ms"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for ToolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ExecutionFailed {
                source: Some(e), ..
            } => Some(e.as_ref()),
            _ => None,
        }
    }
}

/// Format a tool error for model consumption. Truncated at 10,000 chars.
/// TS: formatToolError() — handles AbortError, ShellError, general errors.
pub fn format_tool_error(error: &ToolError) -> String {
    let msg = match error {
        ToolError::Cancelled => "The tool execution was interrupted.".to_string(),
        ToolError::Timeout { timeout_ms } => {
            format!("The tool execution timed out after {timeout_ms}ms.")
        }
        other => other.to_string(),
    };

    // TS: first 5k + "... [N truncated] ..." + last 5k
    if msg.len() > 10_000 {
        let first = &msg[..5_000];
        let last = &msg[msg.len() - 5_000..];
        let truncated = msg.len() - 10_000;
        format!("{first}\n... [{truncated} chars truncated] ...\n{last}")
    } else {
        msg
    }
}

/// Classify a tool error for OTel telemetry.
pub fn classify_tool_error(error: &ToolError) -> &'static str {
    match error {
        ToolError::NotFound { .. } => "not_found",
        ToolError::InvalidInput { .. } => "invalid_input",
        ToolError::ExecutionFailed { .. } => "execution_failed",
        ToolError::PermissionDenied { .. } => "permission_denied",
        ToolError::Timeout { .. } => "timeout",
        ToolError::Cancelled => "cancelled",
    }
}

/// Synthetic errors for interrupted tool execution.
/// TS: used when a tool call is cancelled due to sibling failure or user interrupt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyntheticToolError {
    /// A sibling tool failed, so this one was cancelled.
    SiblingError { failed_tool: String },
    /// User interrupted the conversation.
    UserInterrupted,
    /// Streaming fallback: stream failed, retrying without streaming.
    StreamingFallback,
}

impl fmt::Display for SyntheticToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SiblingError { failed_tool } => {
                write!(f, "cancelled due to sibling tool failure: {failed_tool}")
            }
            Self::UserInterrupted => write!(f, "user interrupted"),
            Self::StreamingFallback => write!(f, "streaming fallback"),
        }
    }
}

/// OTel telemetry event for tool execution completion.
/// TS: emitted after every tool call for analytics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseEvent {
    pub tool_id: ToolId,
    pub success: bool,
    pub duration_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_class: Option<String>,
    #[serde(default)]
    pub is_mcp: bool,
    #[serde(default)]
    pub is_concurrency_safe: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_chain_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_depth: Option<i32>,
}
