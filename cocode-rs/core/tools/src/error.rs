//! Error types for tool execution.

use cocode_error::ErrorExt;
use cocode_error::Location;
use cocode_error::StatusCode;
use cocode_error::stack_trace_debug;
use snafu::Snafu;

/// Tool execution errors.
#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub(crate)), module)]
pub enum ToolError {
    /// Tool not found in registry.
    #[snafu(display("Tool not found: {name}"))]
    NotFound {
        name: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Invalid input for tool.
    #[snafu(display("Invalid input: {message}"))]
    InvalidInput {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Tool execution failed.
    #[snafu(display("Execution failed: {message}"))]
    ExecutionFailed {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Permission denied for tool.
    #[snafu(display("Permission denied: {message}"))]
    PermissionDenied {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Tool execution timed out.
    #[snafu(display("Timeout after {timeout_secs}s"))]
    Timeout {
        timeout_secs: i64,
        #[snafu(implicit)]
        location: Location,
    },

    /// Tool execution was aborted.
    #[snafu(display("Aborted: {reason}"))]
    Aborted {
        reason: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// IO error during tool execution.
    #[snafu(display("IO error: {message}"))]
    Io {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Internal error.
    #[snafu(display("Internal error: {message}"))]
    Internal {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Tool call rejected by a hook.
    #[snafu(display("Hook rejected: {reason}"))]
    HookRejected {
        reason: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Tool execution was cancelled via CancellationToken.
    #[snafu(display("Cancelled"))]
    Cancelled {
        #[snafu(implicit)]
        location: Location,
    },
}

impl ToolError {
    /// Check if this is a retriable error.
    pub fn is_retriable(&self) -> bool {
        matches!(self, ToolError::Timeout { .. } | ToolError::Io { .. })
    }

    /// Check if this error is a cancellation.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, ToolError::Cancelled { .. })
    }

    /// Convert to tool output error message.
    pub fn to_output_message(&self) -> String {
        self.to_string()
    }
}

impl ErrorExt for ToolError {
    fn status_code(&self) -> StatusCode {
        match self {
            ToolError::NotFound { .. } => StatusCode::InvalidArguments, // Tool not found
            ToolError::InvalidInput { .. } => StatusCode::InvalidArguments,
            ToolError::ExecutionFailed { .. } => StatusCode::External, // External tool failure
            ToolError::PermissionDenied { .. } => StatusCode::PermissionDenied,
            ToolError::Timeout { .. } => StatusCode::Timeout,
            ToolError::Aborted { .. } => StatusCode::Cancelled,
            ToolError::Io { .. } => StatusCode::IoError,
            ToolError::Internal { .. } => StatusCode::Internal,
            ToolError::HookRejected { .. } => StatusCode::PermissionDenied, // Hook rejection is a form of denial
            ToolError::Cancelled { .. } => StatusCode::Cancelled,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl From<std::io::Error> for ToolError {
    fn from(err: std::io::Error) -> Self {
        tool_error::IoSnafu {
            message: err.to_string(),
        }
        .build()
    }
}

impl From<serde_json::Error> for ToolError {
    fn from(err: serde_json::Error) -> Self {
        tool_error::InvalidInputSnafu {
            message: format!("JSON error: {err}"),
        }
        .build()
    }
}

/// Result type for tool operations.
pub type Result<T> = std::result::Result<T, ToolError>;

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
