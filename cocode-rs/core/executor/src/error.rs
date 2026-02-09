//! Error types for the executor module.
//!
//! Provides unified error handling with status codes following the cocode-error pattern.

use cocode_error::ErrorExt;
use cocode_error::Location;
use cocode_error::StatusCode;
use cocode_error::stack_trace_debug;
use snafu::Snafu;

/// Executor errors for iterative execution.
#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub(crate)), module)]
pub enum ExecutorError {
    /// Git operation failed (e.g., getting HEAD commit, committing changes).
    #[snafu(display("Git operation failed: {message}"))]
    Git {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Iteration execution failed.
    #[snafu(display("Iteration execution failed: {message}"))]
    Execution {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Context initialization failed.
    #[snafu(display("Context initialization failed: {message}"))]
    Context {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Summarization failed.
    #[snafu(display("Summarization failed: {message}"))]
    Summarization {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Spawn blocking task failed.
    #[snafu(display("Task spawn failed: {message}"))]
    TaskSpawn {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },
}

impl ErrorExt for ExecutorError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Git { .. } => StatusCode::IoError,
            Self::Execution { .. } => StatusCode::Internal,
            Self::Context { .. } => StatusCode::InvalidArguments,
            Self::Summarization { .. } => StatusCode::Internal,
            Self::TaskSpawn { .. } => StatusCode::Internal,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Result type for executor operations.
pub type Result<T> = std::result::Result<T, ExecutorError>;

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
