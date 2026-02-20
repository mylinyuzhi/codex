//! Error types for the skill system.

use cocode_error::ErrorExt;
use cocode_error::Location;
use cocode_error::StatusCode;
use cocode_error::stack_trace_debug;
use snafu::Snafu;
use std::any::Any;

/// Skill system error type.
///
/// Use snafu context selectors from `skill_error` module within the crate:
/// ```ignore
/// use crate::error::skill_error::*;
/// use snafu::ResultExt;
///
/// // Wrapping std::io::Error
/// fs::read(path).context(IoSnafu { message: "read SKILL.md" })?;
///
/// // For errors without a source, use .fail()
/// return ValidationSnafu { message: "name too long" }.fail();
/// ```
#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub(crate)), module)]
pub enum SkillError {
    /// I/O error (wraps std::io::Error).
    #[snafu(display("IO error: {message}"))]
    Io {
        message: String,
        #[snafu(source)]
        source: std::io::Error,
        #[snafu(implicit)]
        location: Location,
    },

    /// YAML parse error (wraps serde_yml::Error).
    #[snafu(display("YAML parse error in {file}: {source}"))]
    YamlParse {
        file: String,
        #[snafu(source)]
        source: serde_yml::Error,
        #[snafu(implicit)]
        location: Location,
    },

    /// Frontmatter parse error.
    #[snafu(display("Frontmatter parse error in {file}: {message}"))]
    FrontmatterParse {
        file: String,
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Validation error.
    #[snafu(display("Validation error: {message}"))]
    Validation {
        message: String,
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

    /// Skill not found.
    #[snafu(display("Skill not found: {name}"))]
    NotFound {
        name: String,
        #[snafu(implicit)]
        location: Location,
    },
}

impl ErrorExt for SkillError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Io { .. } => StatusCode::IoError,
            Self::YamlParse { .. } | Self::FrontmatterParse { .. } | Self::Validation { .. } => {
                StatusCode::InvalidConfig
            }
            Self::Internal { .. } => StatusCode::Internal,
            Self::NotFound { .. } => StatusCode::FileNotFound,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Result type alias for skill operations.
pub type Result<T> = std::result::Result<T, SkillError>;
