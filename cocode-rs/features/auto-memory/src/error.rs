//! Error types for auto memory.

use cocode_error::ErrorExt;
use cocode_error::Location;
use cocode_error::StatusCode;
use cocode_error::stack_trace_debug;
use snafu::Snafu;

/// Result type for auto memory operations.
pub type Result<T> = std::result::Result<T, AutoMemoryError>;

/// Errors that can occur during auto memory operations.
#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub(crate)), module)]
pub enum AutoMemoryError {
    /// Failed to create the memory directory.
    #[snafu(display("Failed to create memory directory at {path}"))]
    CreateDir {
        path: String,
        #[snafu(source)]
        error: std::io::Error,
        #[snafu(implicit)]
        location: Location,
    },

    /// Failed to read a memory file.
    #[snafu(display("Failed to read memory file at {path}"))]
    ReadFile {
        path: String,
        #[snafu(source)]
        error: std::io::Error,
        #[snafu(implicit)]
        location: Location,
    },

    /// Failed to list files in the memory directory.
    #[snafu(display("Failed to list memory directory at {path}"))]
    ListDir {
        path: String,
        #[snafu(source)]
        error: std::io::Error,
        #[snafu(implicit)]
        location: Location,
    },

    /// Configuration resolution failed.
    #[snafu(display("Auto memory config error: {message}"))]
    ConfigResolution {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },
}

impl ErrorExt for AutoMemoryError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::CreateDir { .. } | Self::ListDir { .. } => StatusCode::IoError,
            Self::ReadFile { .. } => StatusCode::IoError,
            Self::ConfigResolution { .. } => StatusCode::Internal,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
