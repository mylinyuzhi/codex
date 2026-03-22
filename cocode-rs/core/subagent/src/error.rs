//! Error types for subagent management.

use std::path::PathBuf;

use cocode_error::BoxedError;
use cocode_error::BoxedErrorSource;
use cocode_error::ErrorExt;
use cocode_error::Location;
use cocode_error::StatusCode;
use cocode_error::stack_trace_debug;
use snafu::Snafu;

#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub(crate)), module)]
pub enum SubagentError {
    #[snafu(display("IO error reading agent file {path:?}"))]
    IoReadFile {
        path: PathBuf,
        #[snafu(source)]
        error: std::io::Error,
        #[snafu(implicit)]
        location: Location,
    },

    #[snafu(display("Frontmatter parse error in {path:?}: {message}"))]
    FrontmatterParse {
        path: PathBuf,
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    #[snafu(display("YAML parse error in {path:?}"))]
    YamlParse {
        path: PathBuf,
        #[snafu(source)]
        error: serde_yml::Error,
        #[snafu(implicit)]
        location: Location,
    },

    #[snafu(display("Cannot determine agent name from file {path:?}"))]
    MissingAgentName {
        path: PathBuf,
        #[snafu(implicit)]
        location: Location,
    },

    #[snafu(display("Unknown agent type: {agent_type}"))]
    UnknownAgentType {
        agent_type: String,
        #[snafu(implicit)]
        location: Location,
    },

    #[snafu(display("Agent not found: {agent_id}"))]
    AgentNotFound {
        agent_id: String,
        #[snafu(implicit)]
        location: Location,
    },

    #[snafu(display("Agent {agent_id} is not backgrounded (status: {status})"))]
    AgentInvalidState {
        agent_id: String,
        status: String,
        #[snafu(implicit)]
        location: Location,
    },

    #[snafu(display("Subagent execution failed: {message}"))]
    Execute {
        message: String,
        #[snafu(source(from(BoxedError, BoxedErrorSource::new)))]
        source: BoxedErrorSource,
        #[snafu(implicit)]
        location: Location,
    },

    #[snafu(display("Background agent limit reached ({limit} concurrent background agents)"))]
    BackgroundLimit {
        limit: usize,
        #[snafu(implicit)]
        location: Location,
    },
}

impl ErrorExt for SubagentError {
    fn status_code(&self) -> StatusCode {
        match self {
            SubagentError::IoReadFile { .. } => StatusCode::IoError,
            SubagentError::FrontmatterParse { .. } => StatusCode::ParseError,
            SubagentError::YamlParse { .. } => StatusCode::ParseError,
            SubagentError::MissingAgentName { .. } => StatusCode::InvalidArguments,
            SubagentError::UnknownAgentType { .. } => StatusCode::InvalidArguments,
            SubagentError::AgentNotFound { .. } => StatusCode::InvalidArguments,
            SubagentError::AgentInvalidState { .. } => StatusCode::InvalidArguments,
            SubagentError::Execute { source, .. } => source.status_code(),
            SubagentError::BackgroundLimit { .. } => StatusCode::ResourcesExhausted,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub type Result<T> = std::result::Result<T, SubagentError>;
