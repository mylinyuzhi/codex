//! Error types for the agent loop.

use cocode_api::ApiError;
use cocode_error::ErrorExt;
use cocode_error::Location;
use cocode_error::StatusCode;
use cocode_error::stack_trace_debug;
use cocode_protocol::StallRecovery;
use snafu::Snafu;

/// Agent loop errors.
#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub(crate)), module)]
pub enum AgentLoopError {
    /// Failed to prepare the main model for an API request.
    #[snafu(display("Failed to prepare main model"))]
    PrepareMainModel {
        #[snafu(source)]
        source: ApiError,
        #[snafu(implicit)]
        location: Location,
    },

    /// Failed to prepare the compact model for compaction.
    #[snafu(display("Failed to prepare compact model"))]
    PrepareCompactModel {
        #[snafu(source)]
        source: ApiError,
        #[snafu(implicit)]
        location: Location,
    },

    /// API stream creation failed.
    #[snafu(display("API stream error"))]
    ApiStream {
        #[snafu(source)]
        source: ApiError,
        #[snafu(implicit)]
        location: Location,
    },

    /// Stream stalled with a specific recovery strategy.
    #[snafu(display("Stream stalled for {timeout}, recovery: {strategy}"))]
    StreamStall {
        timeout: String,
        strategy: StallRecovery,
        #[snafu(implicit)]
        location: Location,
    },

    /// Error received during streaming.
    #[snafu(display("Stream error: {message}"))]
    StreamError {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Session memory extraction LLM request failed.
    #[snafu(display("Extraction LLM request failed"))]
    ExtractionLlmFailed {
        #[snafu(source)]
        source: ApiError,
        #[snafu(implicit)]
        location: Location,
    },

    /// Session memory extraction produced an empty summary.
    #[snafu(display("Empty summary generated"))]
    ExtractionEmptySummary {
        #[snafu(implicit)]
        location: Location,
    },

    /// Failed to write session memory summary file.
    #[snafu(display("Failed to write summary: {message}"))]
    ExtractionWriteFailed {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },
}

impl ErrorExt for AgentLoopError {
    fn status_code(&self) -> StatusCode {
        match self {
            AgentLoopError::PrepareMainModel { source, .. } => source.status_code(),
            AgentLoopError::PrepareCompactModel { source, .. } => source.status_code(),
            AgentLoopError::ApiStream { source, .. } => source.status_code(),
            AgentLoopError::StreamStall { .. } => StatusCode::Timeout,
            AgentLoopError::StreamError { .. } => StatusCode::StreamError,
            AgentLoopError::ExtractionLlmFailed { source, .. } => source.status_code(),
            AgentLoopError::ExtractionEmptySummary { .. } => StatusCode::Internal,
            AgentLoopError::ExtractionWriteFailed { .. } => StatusCode::IoError,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Result type for agent loop operations.
pub type Result<T> = std::result::Result<T, AgentLoopError>;
