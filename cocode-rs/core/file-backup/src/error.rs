//! Error types for file-backup (rewind Tier 1 + snapshot management).

use std::path::PathBuf;

use cocode_error::ErrorExt;
use cocode_error::Location;
use cocode_error::StatusCode;
use cocode_error::stack_trace_debug;
use snafu::Snafu;

#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub(crate)), module)]
pub enum FileBackupError {
    #[snafu(display("IO error: {message}"))]
    Io {
        message: String,
        #[snafu(source)]
        error: std::io::Error,
        #[snafu(implicit)]
        location: Location,
    },

    #[snafu(display("JSON error: {message}"))]
    Json {
        message: String,
        #[snafu(source)]
        error: serde_json::Error,
        #[snafu(implicit)]
        location: Location,
    },

    #[snafu(display("Invalid state: {message}"))]
    InvalidState {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    #[snafu(display(
        "File too large to backup: {path:?} ({size_bytes} bytes > {max_bytes} bytes)"
    ))]
    FileTooLarge {
        path: PathBuf,
        size_bytes: u64,
        max_bytes: u64,
        #[snafu(implicit)]
        location: Location,
    },

    #[snafu(display("Task join failed: {message}"))]
    TaskJoin {
        message: String,
        #[snafu(source)]
        error: tokio::task::JoinError,
        #[snafu(implicit)]
        location: Location,
    },

    #[snafu(display("Git operation failed: {message}"))]
    Git {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },
}

impl ErrorExt for FileBackupError {
    fn status_code(&self) -> StatusCode {
        match self {
            FileBackupError::Io { .. } => StatusCode::IoError,
            FileBackupError::Json { .. } => StatusCode::ParseError,
            FileBackupError::InvalidState { .. } => StatusCode::InvalidArguments,
            FileBackupError::FileTooLarge { .. } => StatusCode::InvalidArguments,
            FileBackupError::Git { .. } => StatusCode::External,
            FileBackupError::TaskJoin { .. } => StatusCode::Internal,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub type Result<T> = std::result::Result<T, FileBackupError>;
