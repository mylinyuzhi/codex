//! Skill interface definition (`SKILL.md` frontmatter schema).
//!
//! Each skill directory contains a `SKILL.md` file with YAML frontmatter
//! that describes the skill's metadata. The markdown body of the file
//! serves as the prompt content. This module defines the deserialization
//! target for the frontmatter.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

/// Metadata of a skill, as defined in `SKILL.md` YAML frontmatter.
///
/// A skill must have a `name` and `description`. The prompt content comes
/// from the markdown body of the `SKILL.md` file (not from the frontmatter).
///
/// # Example SKILL.md
///
/// ```markdown
/// ---
/// name: commit
/// description: Generate a commit message from staged changes
/// allowed-tools:
///   - Bash
///   - Read
/// model: sonnet
/// hooks:
///   PreToolUse:
///     - matcher: "Write|Edit"
///       command: npm run lint
///       once: true
/// ---
///
/// Look at staged changes and generate a commit message.
///
/// $ARGUMENTS
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SkillInterface {
    /// Unique skill name (used as slash-command identifier).
    pub name: String,

    /// Human-readable description.
    pub description: String,

    /// Tools the skill is allowed to invoke.
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,

    /// Guidance for the LLM on when to invoke this skill.
    #[serde(default)]
    pub when_to_use: Option<String>,

    /// Whether this skill can be invoked by the user as a `/command`.
    /// Defaults to `true` if not specified.
    #[serde(default)]
    pub user_invocable: Option<bool>,

    /// Whether to block the LLM from invoking this skill via the Skill tool.
    /// Defaults to `false` if not specified.
    #[serde(default)]
    pub disable_model_invocation: Option<bool>,

    /// Model override for this skill (e.g., "sonnet", "opus", "haiku", "inherit").
    #[serde(default)]
    pub model: Option<String>,

    /// Execution context: "main" (default) or "fork".
    #[serde(default)]
    pub context: Option<String>,

    /// Agent type to use when `context = "fork"`.
    #[serde(default)]
    pub agent: Option<String>,

    /// Usage hint shown in help output (e.g., "<pr-number>").
    #[serde(default)]
    pub argument_hint: Option<String>,

    /// Alternative command names for this skill.
    #[serde(default)]
    pub aliases: Option<Vec<String>>,

    /// Hooks that are registered when this skill starts and removed when it ends.
    ///
    /// The key is the event type (e.g., "PreToolUse", "PostToolUse"), and the
    /// value is a list of hook configurations for that event.
    ///
    /// These hooks are scoped to the skill and automatically cleaned up when
    /// the skill finishes executing.
    #[serde(default)]
    pub hooks: Option<HashMap<String, Vec<SkillHookConfig>>>,
}

/// Hook configuration within a skill's `SKILL.md` frontmatter.
///
/// This is a simplified hook definition that maps to [`HookDefinition`]
/// when the skill is loaded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillHookConfig {
    /// Optional matcher pattern to filter which tool calls trigger this hook.
    ///
    /// Supports three formats:
    /// - Pipe-separated exact values: `"Write|Edit"` → matches Write or Edit
    /// - Wildcard patterns: `"Bash*"` → glob-style matching
    /// - Plain string: `"Write"` → exact match
    #[serde(default)]
    pub matcher: Option<String>,

    /// Command to execute. The command receives hook context as JSON on stdin.
    #[serde(default)]
    pub command: Option<String>,

    /// Arguments for the command.
    #[serde(default)]
    pub args: Option<Vec<String>>,

    /// Timeout in seconds (default: 30).
    #[serde(default = "default_timeout")]
    pub timeout_secs: i32,

    /// If true, the hook is removed after its first successful execution.
    #[serde(default)]
    pub once: bool,
}

fn default_timeout() -> i32 {
    30
}

#[cfg(test)]
#[path = "interface.test.rs"]
mod tests;
