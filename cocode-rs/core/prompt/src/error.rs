//! Error types for prompt generation.

use cocode_error::ErrorExt;
use cocode_error::Location;
use cocode_error::StatusCode;
use cocode_error::stack_trace_debug;
use snafu::Snafu;

/// Prompt generation errors.
#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub(crate)), module)]
pub enum PromptError {
    /// Template rendering error.
    #[snafu(display("Template error: {message}"))]
    Template {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Missing required context field.
    #[snafu(display("Missing context: {field}"))]
    MissingContext {
        field: String,
        #[snafu(implicit)]
        location: Location,
    },
}

impl ErrorExt for PromptError {
    fn status_code(&self) -> StatusCode {
        match self {
            PromptError::Template { .. } => StatusCode::Internal,
            PromptError::MissingContext { .. } => StatusCode::InvalidArguments,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Result type for prompt operations.
pub type Result<T> = std::result::Result<T, PromptError>;

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
