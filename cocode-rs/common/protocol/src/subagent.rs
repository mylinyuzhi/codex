//! Subagent type definitions for the cocode ecosystem.
//!
//! Provides a type-safe enum for builtin subagent types. Custom agents
//! from plugins or user configurations are not represented here.

use serde::Deserialize;
use serde::Serialize;

/// Builtin subagent types with type-safe identifiers.
///
/// Each variant represents a known subagent type with its string identifier
/// accessible via `as_str()`. Custom agents from plugins or user configs
/// are not represented here - they use arbitrary string identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SubagentType {
    /// Codebase exploration (read-only).
    Explore,
    /// Implementation planning (read-only).
    Plan,
    /// Shell command execution.
    Bash,
    /// General-purpose coding.
    General,
    /// Guided reading/documentation.
    Guide,
    /// Status line configuration.
    Statusline,
    /// Code cleanup and refinement.
    CodeSimplifier,
}

impl SubagentType {
    /// Get the string identifier for this subagent type.
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            SubagentType::Explore => "explore",
            SubagentType::Plan => "plan",
            SubagentType::Bash => "bash",
            SubagentType::General => "general",
            SubagentType::Guide => "guide",
            SubagentType::Statusline => "statusline",
            SubagentType::CodeSimplifier => "code-simplifier",
        }
    }

    /// Parse from a string, returns None for unknown/custom types.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "explore" => Some(SubagentType::Explore),
            "plan" => Some(SubagentType::Plan),
            "bash" => Some(SubagentType::Bash),
            "general" => Some(SubagentType::General),
            "guide" => Some(SubagentType::Guide),
            "statusline" => Some(SubagentType::Statusline),
            "code-simplifier" => Some(SubagentType::CodeSimplifier),
            _ => None,
        }
    }

    /// Check if this type has a custom prompt template.
    ///
    /// Explore and Plan subagents have specialized system prompt templates.
    /// Other builtin subagents use the default prompt.
    pub const fn has_custom_prompt(&self) -> bool {
        matches!(self, SubagentType::Explore | SubagentType::Plan)
    }

    /// All builtin subagent types.
    pub const ALL: &[SubagentType] = &[
        SubagentType::Explore,
        SubagentType::Plan,
        SubagentType::Bash,
        SubagentType::General,
        SubagentType::Guide,
        SubagentType::Statusline,
        SubagentType::CodeSimplifier,
    ];
}

impl std::fmt::Display for SubagentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
#[path = "subagent.test.rs"]
mod tests;
