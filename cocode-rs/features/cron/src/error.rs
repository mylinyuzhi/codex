//! Error types for the cron crate.

use cocode_error::ErrorExt;
use cocode_error::Location;
use cocode_error::StatusCode;
use cocode_error::stack_trace_debug;
use snafu::Snafu;

/// Result type alias for cron operations.
pub type Result<T> = std::result::Result<T, CronError>;

/// Errors that can occur during cron operations.
#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub(crate)), module)]
pub enum CronError {
    /// Invalid cron expression.
    #[snafu(display("Invalid schedule: {message}"))]
    InvalidSchedule {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Maximum job limit reached.
    #[snafu(display("Maximum of {limit} active cron jobs reached"))]
    MaxJobsReached {
        limit: i32,
        #[snafu(implicit)]
        location: Location,
    },

    /// Job not found.
    #[snafu(display("Cron job '{id}' not found"))]
    JobNotFound {
        id: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Persistence I/O error.
    #[snafu(display("Persistence error: {message}"))]
    Persist {
        message: String,
        #[snafu(source)]
        error: std::io::Error,
        #[snafu(implicit)]
        location: Location,
    },

    /// Serialization/deserialization error.
    #[snafu(display("Serialization error: {message}"))]
    Serde {
        message: String,
        #[snafu(source)]
        error: serde_json::Error,
        #[snafu(implicit)]
        location: Location,
    },

    /// Lock acquisition failed.
    #[snafu(display("Lock error: {message}"))]
    Lock {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// File watcher error.
    #[snafu(display("Watcher error: {message}"))]
    Watcher {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },
}

impl ErrorExt for CronError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidSchedule { .. } | Self::MaxJobsReached { .. } => {
                StatusCode::InvalidArguments
            }
            Self::JobNotFound { .. } => StatusCode::FileNotFound,
            Self::Persist { .. } => StatusCode::IoError,
            Self::Serde { .. } => StatusCode::Internal,
            Self::Lock { .. } => StatusCode::IoError,
            Self::Watcher { .. } => StatusCode::IoError,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
