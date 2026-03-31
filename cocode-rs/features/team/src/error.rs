//! Error types for the team crate.

use cocode_error::ErrorExt;
use cocode_error::Location;
use cocode_error::StatusCode;
use cocode_error::stack_trace_debug;
use snafu::Snafu;

/// Result type alias for team operations.
pub type Result<T> = std::result::Result<T, TeamError>;

/// Errors that can occur during team operations.
#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub(crate)), module)]
pub enum TeamError {
    /// Team not found.
    #[snafu(display("Team '{name}' not found"))]
    TeamNotFound {
        name: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Team already exists.
    #[snafu(display("Team '{name}' already exists"))]
    TeamExists {
        name: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Agent not a member of the team.
    #[snafu(display("Agent '{agent_id}' is not a member of team '{team_name}'"))]
    NotAMember {
        agent_id: String,
        team_name: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Maximum team members reached.
    #[snafu(display("Team '{team_name}' has reached the maximum of {limit} members"))]
    MaxMembersReached {
        team_name: String,
        limit: usize,
        #[snafu(implicit)]
        location: Location,
    },

    /// Mailbox I/O error.
    #[snafu(display("Mailbox error: {message}"))]
    Mailbox {
        message: String,
        #[snafu(source)]
        error: std::io::Error,
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

    /// Shutdown timeout.
    #[snafu(display("Shutdown timeout for agent '{agent_id}'"))]
    ShutdownTimeout {
        agent_id: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Task not found in the ledger.
    #[snafu(display("Task '{id}' not found"))]
    TaskNotFound {
        id: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Task already claimed by another agent.
    #[snafu(display("Task '{id}' already claimed by '{owner}'"))]
    TaskAlreadyClaimed {
        id: String,
        owner: String,
        #[snafu(implicit)]
        location: Location,
    },
}

impl ErrorExt for TeamError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::TeamNotFound { .. } | Self::NotAMember { .. } => StatusCode::FileNotFound,
            Self::TeamExists { .. } | Self::MaxMembersReached { .. } => {
                StatusCode::InvalidArguments
            }
            Self::Mailbox { .. } | Self::Persist { .. } => StatusCode::IoError,
            Self::Serde { .. } => StatusCode::Internal,
            Self::ShutdownTimeout { .. } => StatusCode::Timeout,
            Self::TaskNotFound { .. } => StatusCode::FileNotFound,
            Self::TaskAlreadyClaimed { .. } => StatusCode::InvalidArguments,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
