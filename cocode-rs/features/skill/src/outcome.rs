//! Skill loading outcome types.
//!
//! Loading skills uses a fail-open strategy: if one skill fails to load
//! (e.g., malformed TOML, missing prompt file), it is reported as a
//! [`SkillLoadOutcome::Failed`] but does not prevent other skills from
//! being loaded successfully.

use crate::command::SkillPromptCommand;
use crate::source::SkillSource;
use std::path::PathBuf;

/// The result of attempting to load a single skill.
///
/// This enum captures both success and failure cases to support the
/// fail-open loading strategy.
#[derive(Debug, Clone)]
pub enum SkillLoadOutcome {
    /// The skill was loaded and validated successfully.
    Success {
        /// The loaded skill command.
        skill: SkillPromptCommand,

        /// Where the skill was loaded from.
        source: SkillSource,
    },

    /// The skill failed to load.
    Failed {
        /// Path to the skill directory that failed.
        path: PathBuf,

        /// Human-readable error description.
        error: String,
    },
}

impl SkillLoadOutcome {
    /// Returns `true` if this outcome is a success.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Returns the skill name if this outcome is a success.
    pub fn skill_name(&self) -> Option<&str> {
        match self {
            Self::Success { skill, .. } => Some(&skill.name),
            Self::Failed { .. } => None,
        }
    }

    /// Converts a successful outcome into the skill command, or `None`.
    pub fn into_skill(self) -> Option<SkillPromptCommand> {
        match self {
            Self::Success { skill, .. } => Some(skill),
            Self::Failed { .. } => None,
        }
    }
}

#[cfg(test)]
#[path = "outcome.test.rs"]
mod tests;
