//! Error types for the executor module.
//!
//! Provides unified error handling with status codes following the cocode-error pattern.

use cocode_error::BoxedError;
use cocode_error::BoxedErrorSource;
use cocode_error::ErrorExt;
use cocode_error::Location;
use cocode_error::StatusCode;
use cocode_error::stack_trace_debug;
use snafu::Snafu;

use crate::coordinator::lifecycle::AgentLifecycleStatus;

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
    #[snafu(display("Iteration execution failed"))]
    Execution {
        #[snafu(source(from(BoxedError, BoxedErrorSource::new)))]
        source: BoxedErrorSource,
        #[snafu(implicit)]
        location: Location,
    },

    /// Agent loop execution failed.
    #[snafu(display("Agent loop failed"))]
    Loop {
        #[snafu(source(from(BoxedError, BoxedErrorSource::new)))]
        source: BoxedErrorSource,
        #[snafu(implicit)]
        location: Location,
    },

    /// Context initialization failed.
    #[snafu(display("Context initialization failed: {message}"))]
    Context {
        message: String,
        #[snafu(source(from(BoxedError, BoxedErrorSource::new)))]
        source: BoxedErrorSource,
        #[snafu(implicit)]
        location: Location,
    },

    /// Coordinated agent not found.
    #[snafu(display("Agent not found: {agent_id}"))]
    AgentNotFound {
        agent_id: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Coordinated agent is not in a state that can accept input.
    #[snafu(display("Agent {agent_id} is not in a state that accepts input (status: {status:?})"))]
    AgentInvalidState {
        agent_id: String,
        status: AgentLifecycleStatus,
        #[snafu(implicit)]
        location: Location,
    },

    /// Coordinated agent completion channel closed unexpectedly.
    #[snafu(display("Agent completion channel closed unexpectedly (agent_id: {agent_id})"))]
    AgentCompletionChannelClosed {
        agent_id: String,
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
            Self::Execution { source, .. } => source.status_code(),
            Self::Loop { source, .. } => source.status_code(),
            Self::Context { source, .. } => source.status_code(),
            Self::AgentNotFound { .. } => StatusCode::InvalidArguments,
            Self::AgentInvalidState { .. } => StatusCode::InvalidArguments,
            Self::AgentCompletionChannelClosed { .. } => StatusCode::Internal,
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
