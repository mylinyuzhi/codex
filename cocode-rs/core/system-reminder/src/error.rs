//! Error types for system reminder operations.

use cocode_error::ErrorExt;
use cocode_error::Location;
use cocode_error::StatusCode;
use cocode_error::stack_trace_debug;
use snafu::Snafu;

/// System reminder errors.
#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub(crate)), module)]
pub enum SystemReminderError {
    /// Generator failed to produce output.
    #[snafu(display("Generator '{name}' failed: {message}"))]
    GeneratorFailed {
        name: String,
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Generator timed out.
    #[snafu(display("Generator '{name}' timed out after {timeout_ms}ms"))]
    GeneratorTimeout {
        name: String,
        timeout_ms: i64,
        #[snafu(implicit)]
        location: Location,
    },

    /// Invalid configuration.
    #[snafu(display("Invalid configuration: {message}"))]
    InvalidConfig {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// File operation failed.
    #[snafu(display("File operation failed: {message}"))]
    FileOperation {
        message: String,
        #[snafu(source)]
        source: std::io::Error,
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
}

impl ErrorExt for SystemReminderError {
    fn status_code(&self) -> StatusCode {
        match self {
            SystemReminderError::GeneratorFailed { .. } => StatusCode::Internal,
            SystemReminderError::GeneratorTimeout { .. } => StatusCode::Timeout,
            SystemReminderError::InvalidConfig { .. } => StatusCode::InvalidConfig,
            SystemReminderError::FileOperation { .. } => StatusCode::IoError,
            SystemReminderError::Internal { .. } => StatusCode::Internal,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Result type for system reminder operations.
pub type Result<T> = std::result::Result<T, SystemReminderError>;

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
